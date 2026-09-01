//! Project projection core: derives stable Project identity and indexes from
//! Herdr runtime state.
//!
//! The terminology asymmetry is deliberate:
//! - A Shardlane Workspace is a user grouping persisted in config.json (see
//!   workspace_config.rs);
//! - A Project is the Shardlane projection of a Herdr runtime Workspace.
//!
//! Herdr stays authoritative over runtime workspace IDs/tabs/panes/agents/
//! process lifecycle; this module only derives stable Project identity from
//! runtime state + persisted Task metadata.
//!
//! [INPUT]: depends on crate::herdr's HerdrState (the runtime snapshot type)
//! [OUTPUT]: exposes ProjectKey/ProjectProjection/ProjectIndex
//! (two-phase build: build_from_state + with_script_paths),
//! project_paths_match
//! [POS]: the projection core of shardlane-host; GUI workspace_model.rs and
//! shardlane-remote bootstrap.rs consume the same implementation (a parallel
//! second implementation is forbidden)

use crate::herdr::HerdrState;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum ProjectKey {
    Path(String),
    Runtime(String),
}

impl ProjectKey {
    pub fn from_project_path(path: &str) -> Option<Self> {
        normalized_project_path(path).map(Self::Path)
    }

    pub fn search_value(&self) -> &str {
        match self {
            Self::Path(path) | Self::Runtime(path) => path,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectProjection {
    pub key: ProjectKey,
    /// Herdr's technical runtime workspace identifier for this Project, when materialized.
    pub runtime_workspace_id: Option<String>,
    pub label: String,
    pub project_path: Option<String>,
    pub tab_ids: Vec<String>,
    pub pane_ids: Vec<String>,
    pub agent_terminal_ids: Vec<String>,
    pub script_ids: Vec<String>,
}

impl ProjectProjection {
    pub fn search_context(&self) -> String {
        [
            Some(self.label.as_str()),
            self.project_path.as_deref(),
            Some(self.key.search_value()),
            self.runtime_workspace_id.as_deref(),
        ]
        .into_iter()
        .flatten()
        .filter(|value| !value.trim().is_empty())
        .collect::<Vec<_>>()
        .join(" ")
    }
}

#[derive(Debug)]
struct ProjectDraft {
    runtime_workspace_id: String,
    explicit_label: Option<String>,
    number: Option<u32>,
    project_path: Option<String>,
    tab_ids: Vec<String>,
    pane_ids: Vec<String>,
    agent_terminal_ids: Vec<String>,
    script_ids: Vec<String>,
}

impl ProjectDraft {
    fn set_project_path_if_missing(&mut self, path: Option<&str>) {
        if self.project_path.is_none() {
            self.project_path = non_empty(path).map(str::to_string);
        }
    }

    fn into_projection(self) -> ProjectProjection {
        let key = self
            .project_path
            .as_deref()
            .and_then(ProjectKey::from_project_path)
            .unwrap_or_else(|| ProjectKey::Runtime(self.runtime_workspace_id.clone()));
        let label = self
            .explicit_label
            .or_else(|| self.project_path.as_deref().and_then(project_path_name))
            .or_else(|| self.number.map(|number| format!("Project {number}")))
            .unwrap_or_else(|| self.runtime_workspace_id.clone());
        ProjectProjection {
            key,
            runtime_workspace_id: Some(self.runtime_workspace_id),
            label,
            project_path: self.project_path,
            tab_ids: self.tab_ids,
            pane_ids: self.pane_ids,
            agent_terminal_ids: self.agent_terminal_ids,
            script_ids: self.script_ids,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct ProjectIndex {
    projects: Vec<ProjectProjection>,
    by_runtime_id: HashMap<String, usize>,
    by_project_key: HashMap<String, usize>,
}

impl ProjectIndex {
    /// Runtime-phase build: Herdr's full projection state → Project
    /// projections (no Task merging).
    pub fn build_from_state(state: &HerdrState) -> Self {
        let mut drafts = state
            .workspaces
            .iter()
            .map(|runtime_workspace| ProjectDraft {
                runtime_workspace_id: runtime_workspace.workspace_id.clone(),
                explicit_label: non_empty(runtime_workspace.label.as_deref()).map(str::to_string),
                number: runtime_workspace.number,
                project_path: non_empty(runtime_workspace.cwd.as_deref()).map(str::to_string),
                tab_ids: Vec::new(),
                pane_ids: Vec::new(),
                agent_terminal_ids: Vec::new(),
                script_ids: Vec::new(),
            })
            .collect::<Vec<_>>();
        let positions = drafts
            .iter()
            .enumerate()
            .map(|(index, draft)| (draft.runtime_workspace_id.clone(), index))
            .collect::<HashMap<_, _>>();

        for tab in &state.tabs {
            let Some(runtime_workspace_id) = tab.workspace_id.as_deref() else {
                continue;
            };
            if let Some(index) = positions.get(runtime_workspace_id).copied() {
                drafts[index].tab_ids.push(tab.tab_id.clone());
            }
        }
        for pane in &state.panes {
            let Some(runtime_workspace_id) = pane.workspace_id.as_deref() else {
                continue;
            };
            if let Some(index) = positions.get(runtime_workspace_id).copied() {
                drafts[index].pane_ids.push(pane.pane_id.clone());
                drafts[index].set_project_path_if_missing(pane.cwd.as_deref());
            }
        }
        for agent in &state.agents {
            let Some(runtime_workspace_id) = agent.workspace_id.as_deref() else {
                continue;
            };
            if let Some(index) = positions.get(runtime_workspace_id).copied() {
                drafts[index]
                    .agent_terminal_ids
                    .push(agent.terminal_id.clone());
                drafts[index].set_project_path_if_missing(
                    non_empty(agent.foreground_cwd.as_deref())
                        .or_else(|| non_empty(agent.cwd.as_deref())),
                );
            }
        }
        let mut projects = Vec::with_capacity(drafts.len());
        let mut by_runtime_id = HashMap::with_capacity(drafts.len());
        let mut by_project_key = HashMap::with_capacity(drafts.len());
        for draft in drafts {
            let project = draft.into_projection();
            let index = projects.len();
            if let Some(runtime_id) = &project.runtime_workspace_id {
                by_runtime_id.insert(runtime_id.clone(), index);
            }
            if let ProjectKey::Path(project_key) = &project.key {
                by_project_key.entry(project_key.clone()).or_insert(index);
            }
            projects.push(project);
        }

        Self {
            projects,
            by_runtime_id,
            by_project_key,
        }
    }

    /// Task-merging phase: Task ownership is always decided by the stable
    /// project_path; the Herdr workspace_id is only a one-off runtime
    /// association and never decides which Project owns a Task.
    /// Paths without a runtime counterpart are backfilled as path-key
    /// Projects (identical semantics for GUI/remote).
    pub fn with_script_paths<I>(mut self, scripts: I) -> Self
    where
        I: IntoIterator<Item = (String, String)>,
    {
        for (script_id, project_path_raw) in scripts {
            let Some(project_path) = non_empty(Some(project_path_raw.as_str())).map(str::to_string)
            else {
                continue;
            };
            let Some(ProjectKey::Path(project_key)) = ProjectKey::from_project_path(&project_path)
            else {
                continue;
            };
            if let Some(index) = self.by_project_key.get(&project_key).copied() {
                if !self.projects[index].script_ids.contains(&script_id) {
                    self.projects[index].script_ids.push(script_id.clone());
                }
                continue;
            }
            let label = project_path_name(&project_path).unwrap_or_else(|| project_path.clone());
            let index = self.projects.len();
            self.projects.push(ProjectProjection {
                key: ProjectKey::Path(project_key.clone()),
                runtime_workspace_id: None,
                label,
                project_path: Some(project_path),
                tab_ids: Vec::new(),
                pane_ids: Vec::new(),
                agent_terminal_ids: Vec::new(),
                script_ids: vec![script_id.clone()],
            });
            self.by_project_key.insert(project_key, index);
        }
        self
    }

    pub fn projects(&self) -> &[ProjectProjection] {
        &self.projects
    }

    pub fn for_runtime_id(&self, workspace_id: &str) -> Option<&ProjectProjection> {
        self.by_runtime_id
            .get(workspace_id)
            .copied()
            .map(|index| &self.projects[index])
    }

    pub fn for_project_path(&self, project_path: &str) -> Option<&ProjectProjection> {
        let ProjectKey::Path(project_key) = ProjectKey::from_project_path(project_path)? else {
            return None;
        };
        self.by_project_key
            .get(&project_key)
            .and_then(|index| self.projects.get(*index))
    }

    pub fn runtime_workspace_id_for_project_path(&self, project_path: &str) -> Option<&str> {
        self.for_project_path(project_path)
            .and_then(|project| project.runtime_workspace_id.as_deref())
    }
}

pub fn project_paths_match(left: &str, right: &str) -> bool {
    match (
        ProjectKey::from_project_path(left),
        ProjectKey::from_project_path(right),
    ) {
        (Some(left), Some(right)) => left == right,
        _ => left.trim() == right.trim(),
    }
}

pub fn non_empty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

/// Process-level memoization of canonicalization results: UI hot paths
/// (keystroke-level search / shortcut resolution / the settings page)
/// repeatedly canonicalize the same batch of project paths; without a cache
/// each keystroke costs dozens of syscalls, and network volume paths can
/// block for seconds. Within the TTL, key-conversion delays of up to TTL
/// from symlinks/newly created directories are accepted.
const NORMALIZED_PATH_CACHE_TTL: Duration = Duration::from_secs(1);
const NORMALIZED_PATH_CACHE_MAX_ENTRIES: usize = 1024;

fn normalized_path_cache() -> &'static Mutex<HashMap<String, (String, Instant)>> {
    static CACHE: OnceLock<Mutex<HashMap<String, (String, Instant)>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn normalized_project_path(path: &str) -> Option<String> {
    let path = path.trim();
    if path.is_empty() {
        return None;
    }
    if let Ok(cache) = normalized_path_cache().lock() {
        if let Some((normalized, cached_at)) = cache.get(path) {
            if cached_at.elapsed() < NORMALIZED_PATH_CACHE_TTL {
                return Some(normalized.clone());
            }
        }
    }
    let normalized = std::fs::canonicalize(path)
        .unwrap_or_else(|_| Path::new(path).components().collect::<PathBuf>());
    let normalized = normalized.to_string_lossy().into_owned();
    if let Ok(mut cache) = normalized_path_cache().lock() {
        if cache.len() >= NORMALIZED_PATH_CACHE_MAX_ENTRIES {
            cache.clear();
        }
        cache.insert(path.to_string(), (normalized.clone(), Instant::now()));
    }
    Some(normalized)
}

pub fn project_path_name(path: &str) -> Option<String> {
    Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::to_string)
        .filter(|name| !name.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::herdr::{Agent, Pane, Tab, Workspace};

    fn runtime_workspace(id: &str, cwd: Option<&str>) -> Workspace {
        Workspace::stub(id, cwd)
    }

    fn pane(id: &str, workspace_id: &str, cwd: Option<&str>) -> Pane {
        Pane {
            pane_id: id.into(),
            workspace_id: Some(workspace_id.into()),
            cwd: cwd.map(str::to_string),
            ..Pane::default()
        }
    }

    fn tab(id: &str, workspace_id: &str) -> Tab {
        Tab {
            tab_id: id.into(),
            workspace_id: Some(workspace_id.into()),
            focused: false,
            label: None,
            title: None,
            terminal_title: None,
            agent_status: None,
            pane_count: None,
        }
    }

    fn agent(terminal_id: &str, workspace_id: &str, cwd: Option<&str>) -> Agent {
        Agent {
            terminal_id: terminal_id.into(),
            workspace_id: Some(workspace_id.into()),
            cwd: cwd.map(str::to_string),
            ..Agent::default()
        }
    }

    #[test]
    fn runtime_projects_index_tabs_panes_and_agents() {
        let state = HerdrState {
            focused_workspace_id: None,
            focused_tab_id: None,
            focused_pane_id: None,
            workspaces: vec![
                runtime_workspace("w1", Some("/tmp/demo")),
                runtime_workspace("w2", None),
            ],
            tabs: vec![tab("t1", "w1"), tab("t2", "w2")],
            panes: vec![pane("p1", "w1", Some("/tmp/demo")), pane("p2", "w2", None)],
            agents: vec![agent("term-a", "w1", Some("/tmp/demo"))],
            layouts: Vec::new(),
            protocol: None,
            version: None,
        };
        let index = ProjectIndex::build_from_state(&state);
        let projects = index.projects();
        assert_eq!(projects.len(), 2);
        let demo = index
            .for_runtime_id("w1")
            .unwrap_or_else(|| panic!("w1 missing"));
        assert_eq!(demo.tab_ids, vec!["t1".to_string()]);
        assert_eq!(demo.pane_ids, vec!["p1".to_string()]);
        assert_eq!(demo.agent_terminal_ids, vec!["term-a".to_string()]);
        assert!(matches!(demo.key, ProjectKey::Path(_)));
    }

    #[test]
    fn script_paths_attach_to_matching_path_projects() {
        let state = HerdrState {
            focused_workspace_id: None,
            focused_tab_id: None,
            focused_pane_id: None,
            workspaces: vec![runtime_workspace("w1", Some("/tmp/demo"))],
            tabs: vec![],
            panes: vec![],
            agents: vec![],
            layouts: Vec::new(),
            protocol: None,
            version: None,
        };
        let index = ProjectIndex::build_from_state(&state).with_script_paths([
            ("script-1".to_string(), "/tmp/demo".to_string()),
            ("script-2".to_string(), "/tmp/other".to_string()),
        ]);
        let demo = index
            .for_project_path("/tmp/demo")
            .unwrap_or_else(|| panic!("demo missing"));
        assert_eq!(demo.script_ids, vec!["script-1".to_string()]);
        let other = index
            .for_project_path("/tmp/other")
            .unwrap_or_else(|| panic!("other missing"));
        assert!(other.runtime_workspace_id.is_none(), "path-only project");
    }

    #[test]
    fn project_paths_match_is_normalization_aware() {
        assert!(project_paths_match("/tmp/demo", "/tmp/demo/"));
        assert!(!project_paths_match("/tmp/demo", "/tmp/other"));
    }
}
