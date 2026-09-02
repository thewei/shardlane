//! Bootstrap projection: config.json membership + Herdr full projected state
//! → HostBootstrap DTO.
//!
//! [INPUT]: Depends on shardlane_host (ProjectIndex/project_id
//! codecs/DTOs/host_agent_status/workspace_config/herdr types) and
//! crate::state (RemoteState: settings path and Herdr connection)
//! [OUTPUT]: Exposes build_bootstrap/project_summary_for_id/
//! connect_herdr (a side-effect-free connect)
//! [POS]: The read-projection core of shardlane-remote; shares the same
//! ProjectIndex implementation as the desktop (a parallel second projection
//! is forbidden); read-only over config.json — the writer/owner is always
//! GUI Settings

use crate::error::ApiError;
use crate::state::RemoteState;
use shardlane_host::herdr::{HerdrClient, HerdrError};
use shardlane_host::mux::{InstanceRef, MultiplexerConnection, MuxCapabilities, MuxError};
use shardlane_host::project_index::ProjectProjection;
use shardlane_host::workspace_config::{WorkspaceConfig, WorkspacesConfig};
use shardlane_host::{
    project_id_for_path, project_id_for_runtime_workspace, AgentRef, HostBootstrap,
    HostCapabilities, HostInfo, PaneId, PaneSummary, ProjectId, ProjectSummary, TabId, TabSummary,
    WorkspaceId, WorkspaceSummary, HOST_API_VERSION,
};
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug)]
pub enum BootstrapError {
    /// The instance is unreachable (missing socket / failed ping / protocol
    /// mismatch / unknown backend).
    Unavailable(MuxError),
    /// The instance answered connect but an RPC failed midway.
    Runtime(MuxError),
}

/// Single BootstrapError → ApiError envelope mapping (used by the blocking
/// projection wrapper in server.rs and the v2 Conversation adapters); arms
/// must not drift between call sites.
pub(crate) fn map_bootstrap_error(error: BootstrapError, request_id: String) -> ApiError {
    match error {
        BootstrapError::Unavailable(error) => ApiError::host_unavailable(
            format!("herdr runtime is not reachable: {error}"),
            request_id,
        ),
        BootstrapError::Runtime(error) => ApiError::runtime_unavailable(
            format!("herdr runtime failed to answer: {error}"),
            request_id,
        ),
    }
}

/// Single authority for the advertised Host capability set: `hello` and the
/// bootstrap projection must declare byte-identical values (the mobile entry
/// gate reads the bootstrap copy), so both construct through this function
/// instead of maintaining twin literals. The values are derived from the
/// registry's backend capabilities (the Herdr builtin advertises all-true,
/// keeping the pinned wire values).
pub(crate) fn host_capabilities_for(state: &RemoteState) -> HostCapabilities {
    let caps = state
        .mux_registry
        .backend("herdr")
        .map(|backend| backend.capabilities())
        .unwrap_or(MuxCapabilities {
            agents: false,
            server_admin: false,
            shared_tui: false,
            pane_history_read: false,
            cross_workspace_tab_move: false,
            events_push: false,
        });
    host_capabilities_from(caps)
}

/// Capability mapping used by [`host_capabilities_for`]; free-standing so the
/// pinned contract test can exercise it without a registry.
pub(crate) fn host_capabilities_from(caps: MuxCapabilities) -> HostCapabilities {
    HostCapabilities {
        agent_control: caps.agents,
        scripts: false,
        // The semantic History list/search/transcript is provided by the
        // v2 Conversation service; the mobile entry gate reads this value.
        history: caps.agents,
        terminal_text: caps.pane_history_read,
        terminal_stream: caps.shared_tui,
        conversation_view: caps.agents,
        conversation_live: caps.agents,
        history_continue: caps.agents,
        // One Host-owned shared/global backend TUI session. Client-local
        // focus/resize isolation is intentionally not implied; focus and
        // resize effects are visible to other attached clients.
        herdr_tui: caps.shared_tui,
    }
}

/// Lenient path for reading settings JSON: missing file/bad JSON → the
/// default Workspace set (a legitimate state for freshly installed
/// machines); remote never rejects the whole bootstrap because config is
/// unreadable.
fn load_workspace_config(state: &RemoteState) -> (WorkspacesConfig, HashMap<String, String>) {
    let mut config = WorkspacesConfig::default();
    let mut overrides = HashMap::new();
    if let Ok(raw) = std::fs::read_to_string(&state.settings_path) {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(&raw) {
            if let Some(parsed) = value.get("workspaces") {
                if let Ok(workspaces) = serde_json::from_value::<WorkspacesConfig>(parsed.clone()) {
                    config = workspaces;
                }
            }
            if let Some(map) = value
                .get("project_path_overrides")
                .and_then(|v| serde_json::from_value::<HashMap<String, String>>(v.clone()).ok())
            {
                overrides = map;
            }
        }
    }
    if config.items.is_empty() {
        // Hand-written configs may yield empty items; fall back to the
        // Default Workspace so a fallback target always exists.
        config = WorkspacesConfig::default();
    }
    (config, overrides)
}

/// A Project's effective path: explicit discovery path first, then the
/// runtime override.
fn effective_project_path(
    projection: &ProjectProjection,
    overrides: &HashMap<String, String>,
) -> Option<String> {
    projection.project_path.clone().or_else(|| {
        projection
            .runtime_workspace_id
            .as_deref()
            .and_then(|runtime_id| overrides.get(runtime_id).cloned())
    })
}

fn workspace_for_project<'a>(
    config: &'a WorkspacesConfig,
    path: Option<&str>,
) -> &'a WorkspaceConfig {
    if let Some(path) = path {
        for workspace in &config.items {
            if workspace.project_paths.iter().any(|candidate| {
                shardlane_host::project_index::project_paths_match(candidate, path)
            }) {
                return workspace;
            }
        }
    }
    // Documented rule: an unassigned Project falls back to the first
    // Workspace (Default).
    &config.items[0]
}

// The instance scope of the current request (`?instance=<registry id>`).
// Middleware stamps it; every scoped client resolution reads it once before
// entering `spawn_blocking` (task-locals do not cross that boundary).
tokio::task_local! {
    static CURRENT_INSTANCE: Option<String>;
}

/// Middleware: stamps `?instance=<registry id>` into the request's task-local
/// scope so every client resolution in this request targets that instance.
pub async fn instance_scope_middleware(
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    // Registry ids are URL-safe ("default", "p-<uuid>"); a plain split avoids
    // a form-urlencoding dependency.
    let instance = request.uri().query().and_then(|query| {
        query.split('&').find_map(|pair| {
            let mut parts = pair.splitn(2, '=');
            match (parts.next(), parts.next()) {
                (Some("instance"), Some(value)) => Some(value.to_string()),
                _ => None,
            }
        })
    });
    in_instance_scope(instance, next.run(request)).await
}

/// Snapshot of the current request's instance scope (None = default instance).
pub fn current_instance_scope() -> Option<String> {
    CURRENT_INSTANCE.try_get().ok().flatten()
}

/// Runs `f` with the instance scope stamped from the request query.
pub async fn in_instance_scope(
    instance: Option<String>,
    f: impl std::future::Future<Output = axum::response::Response>,
) -> axum::response::Response {
    CURRENT_INSTANCE.scope(instance, f).await
}

/// Resolve the backend-neutral connection for an instance scope (multi-
/// instance). `None` = no instance scope given — fall back to the base
/// connection (side-effect-free; an explicit socket override wins). A scoped
/// instance is opened with persistent-instance semantics (its server is
/// started when not running — the same behavior as the desktop's
/// `bootstrap_for_session`).
pub fn connect_instance_for(
    state: &RemoteState,
    instance: Option<&str>,
) -> Result<Arc<dyn MultiplexerConnection>, MuxError> {
    let reference = match (instance, &state.herdr_socket_override) {
        (Some(session), _) => InstanceRef::named("herdr", session),
        (None, Some(path)) => InstanceRef::socket("herdr", path.clone()),
        (None, None) => InstanceRef::default_instance("herdr"),
    };
    if instance.is_some() {
        state.mux_registry.open_instance(&reference)
    } else {
        state.mux_registry.connect_instance(&reference)
    }
}

/// Resolve the concrete Herdr client for Herdr-product surfaces (Domain 7/9
/// services and the TUI protocol gate). Capability-gated: an instance served
/// by a non-Herdr backend is a typed error, never a silent fallback.
pub fn connect_herdr_for(
    state: &RemoteState,
    instance: Option<&str>,
) -> Result<HerdrClient, HerdrError> {
    let connection = connect_instance_for(state, instance).map_err(HerdrError::from)?;
    connection
        .as_herdr()
        .cloned()
        .ok_or_else(|| HerdrError::Api("this instance is not served by the herdr backend".into()))
}

/// The instance scope for blocking work: read it on the async side.
pub fn scope_for_blocking() -> Option<String> {
    current_instance_scope()
}

/// Connect to the default instance's concrete Herdr client (side-effect-free
/// connect: does not start the server; a missing socket is an error).
pub fn connect_herdr(state: &RemoteState) -> Result<HerdrClient, HerdrError> {
    connect_herdr_for(state, None)
}

pub fn build_bootstrap(state: &RemoteState) -> Result<HostBootstrap, BootstrapError> {
    build_bootstrap_for(state, None)
}

/// Instance-aware variant: `instance` is resolved on the async side (task
/// locals do not survive `spawn_blocking`) and threaded in explicitly.
pub fn build_bootstrap_for(
    state: &RemoteState,
    instance: Option<&str>,
) -> Result<HostBootstrap, BootstrapError> {
    let connection = connect_instance_for(state, instance).map_err(BootstrapError::Unavailable)?;
    let runtime_state = connection
        .host_bootstrap_state()
        .map_err(BootstrapError::Runtime)?;
    let index = shardlane_host::project_index::ProjectIndex::build_from_state(&runtime_state);
    let (workspace_config, overrides) = load_workspace_config(state);

    // Runtime index: tabs/panes looked up directly by id; agents attributed
    // by pane (AgentRef is pane-backed, matching the AgentRuntime SPI
    // encoding).
    let tabs_by_id: HashMap<&str, &shardlane_host::herdr::Tab> = runtime_state
        .tabs
        .iter()
        .map(|tab| (tab.tab_id.as_str(), tab))
        .collect();
    let panes_by_id: HashMap<&str, &shardlane_host::herdr::Pane> = runtime_state
        .panes
        .iter()
        .map(|pane| (pane.pane_id.as_str(), pane))
        .collect();
    let agent_pane_ids: Vec<&str> = runtime_state
        .agents
        .iter()
        .filter_map(|agent| agent.pane_id.as_deref())
        .filter(|id| !id.is_empty())
        .collect();

    let mut projects_by_workspace: HashMap<String, Vec<ProjectId>> = HashMap::new();
    let mut projects = Vec::new();
    let mut tabs = Vec::new();
    let mut panes = Vec::new();

    for projection in index.projects() {
        let path = effective_project_path(projection, &overrides);
        let workspace = workspace_for_project(&workspace_config, path.as_deref());
        let project_id = match &projection.runtime_workspace_id {
            Some(runtime_id) => project_id_for_runtime_workspace(runtime_id),
            None => project_id_for_path(path.as_deref().unwrap_or("shardlane://orphan")),
        };

        let mut project_tab_ids = Vec::new();
        for tab_id in &projection.tab_ids {
            let tab = tabs_by_id.get(tab_id.as_str());
            let pane_ids: Vec<PaneId> = runtime_state
                .panes
                .iter()
                .filter(|pane| pane.tab_id.as_deref() == Some(tab_id.as_str()))
                .filter(|pane| {
                    projection
                        .pane_ids
                        .iter()
                        .any(|p| p == pane.pane_id.as_str())
                })
                .map(|pane| PaneId::new(pane.pane_id.clone()))
                .collect();
            tabs.push(TabSummary {
                id: TabId::new(tab_id.clone()),
                project_id: project_id.clone(),
                label: tab.and_then(|tab| tab.label.clone()),
                title: tab.and_then(|tab| tab.title.clone()),
                pane_ids,
            });
            project_tab_ids.push(TabId::new(tab_id.clone()));
        }
        for pane_id in &projection.pane_ids {
            let Some(pane) = panes_by_id.get(pane_id.as_str()) else {
                continue;
            };
            // Orphan pane (no tab ownership and the Project has no tabs)
            // does not enter the remote projection.
            let Some(tab_id) = pane
                .tab_id
                .clone()
                .or_else(|| projection.tab_ids.first().cloned())
            else {
                continue;
            };
            let has_agent = agent_pane_ids.contains(&pane_id.as_str());
            panes.push(PaneSummary {
                id: PaneId::new(pane_id.clone()),
                tab_id: TabId::new(tab_id),
                title: pane.title.clone().or_else(|| pane.terminal_title.clone()),
                cwd: pane.cwd.clone(),
                agent_ref: has_agent.then(|| AgentRef::new(pane_id.clone())),
            });
        }

        let agent_refs = agent_pane_ids
            .iter()
            .filter(|pane_id| projection.pane_ids.iter().any(|p| p == *pane_id))
            .map(|pane_id| AgentRef::new((*pane_id).to_string()))
            .collect::<Vec<_>>();

        projects.push(ProjectSummary {
            id: project_id.clone(),
            workspace_id: WorkspaceId::new(workspace.id.clone()),
            label: projection.label.clone(),
            project_path: path,
            runtime_available: projection.runtime_workspace_id.is_some(),
            tab_ids: project_tab_ids,
            agent_refs,
        });
        projects_by_workspace
            .entry(workspace.id.clone())
            .or_default()
            .push(project_id);
    }

    let workspaces = workspace_config
        .items
        .iter()
        .map(|workspace| WorkspaceSummary {
            id: WorkspaceId::new(workspace.id.clone()),
            name: workspace.name.clone(),
            color: workspace.color.clone(),
            active: workspace.id == workspace_config.active_id,
            project_ids: projects_by_workspace
                .remove(&workspace.id)
                .unwrap_or_default(),
        })
        .collect::<Vec<_>>();

    // The Host Agent service owns this runtime-state → AgentSummary projection;
    // bootstrap and product Agent callers must not maintain parallel mappings.
    let agents = shardlane_host::agent_summaries_from_state(&runtime_state);

    let conversations = agents
        .iter()
        .filter_map(|summary| {
            // `conversation_id` is set only for typed, resolvable session
            // locators; metadata-only agents must not become Conversations.
            summary.conversation_id.as_ref()?;
            let runtime_agent = runtime_state
                .agents
                .iter()
                .find(|agent| agent.pane_id.as_deref() == Some(summary.pane_id.as_str()))?;
            let session = runtime_agent.agent_session.as_ref()?;
            Some(shardlane_host::live_summary(
                summary.id.clone(),
                Some(session),
                summary.project_id.clone(),
                session.agent.clone(),
                summary
                    .title
                    .clone()
                    .or_else(|| summary.name.clone())
                    .unwrap_or_else(|| "Live conversation".to_string()),
                summary.status,
                summary.revision,
            ))
        })
        .collect();

    Ok(HostBootstrap {
        host: HostInfo {
            host_id: state.host_id().to_string(),
            name: state.host_name.clone(),
            version: state.host_version.clone(),
            api_version: HOST_API_VERSION,
        },
        capabilities: host_capabilities_for(state),
        workspaces,
        projects,
        tabs,
        panes,
        agents,
        conversations,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_fallback_targets_first_workspace() {
        let config = WorkspacesConfig::default();
        let matched = workspace_for_project(&config, Some("/not/assigned"));
        assert_eq!(matched.id, "workspace-main");
    }

    #[test]
    fn advertised_capabilities_are_pinned_from_the_single_constructor() {
        // hello and bootstrap must stay byte-identical; this pins the values
        // so a change here is a deliberate contract change, not drift.
        let herdr_caps = MuxCapabilities {
            agents: true,
            server_admin: true,
            shared_tui: true,
            pane_history_read: true,
            cross_workspace_tab_move: false,
            events_push: true,
        };
        let json = serde_json::to_value(host_capabilities_from(herdr_caps)).unwrap_or_default();
        assert_eq!(json["agent_control"], true);
        assert_eq!(json["scripts"], false);
        assert_eq!(json["history"], true);
        assert_eq!(json["terminal_text"], true);
        assert_eq!(json["terminal_stream"], true);
        assert_eq!(json["conversation_view"], true);
        assert_eq!(json["conversation_live"], true);
        assert_eq!(json["history_continue"], true);
        assert_eq!(json["herdr_tui"], true);
    }

    #[test]
    fn bootstrap_error_maps_to_distinct_unavailable_envelopes() {
        let id = "req-x".to_string();
        let unavailable = map_bootstrap_error(
            BootstrapError::Unavailable(MuxError::SocketUnavailable(
                "/tmp/h.sock".into(),
                "no such file".into(),
            )),
            id.clone(),
        );
        assert_eq!(
            unavailable.status,
            axum::http::StatusCode::SERVICE_UNAVAILABLE
        );
        assert_eq!(unavailable.code, "host_unavailable");
        let runtime =
            map_bootstrap_error(BootstrapError::Runtime(MuxError::Api("boom".into())), id);
        assert_eq!(runtime.status, axum::http::StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(runtime.code, "runtime_unavailable");
    }
}
