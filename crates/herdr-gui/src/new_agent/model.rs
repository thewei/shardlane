//! [INPUT]: The main-crate namespace and sibling-module public surface forwarded by the new_agent module root (super).
//! [OUTPUT]: Provides New Agent's composer state model: project/branch selection, mode/permission enums, UI state (including the script_delete_armed_id two-click arming state) and launch intents, and selection-list derivation helpers (focused index / project choices).
//! [POS]: The `crates/herdr-gui` new_agent submodule (mechanically split out of new_agent.rs), cooperating isomorphically with sibling submodules, exported via the root re-export.
use super::reference_index::{CommandCatalog, ProjectFileIndex};
use super::*;
use std::sync::Arc;

#[derive(Clone, Debug)]
pub(crate) struct NewAgentProjectChoice {
    pub runtime_workspace_id: String,
    pub label: String,
    pub project_path: String,
}

impl SelectItem for NewAgentProjectChoice {
    type Value = String;

    fn title(&self) -> SharedString {
        SharedString::from(format!("{} · {}", self.label, self.project_path))
    }

    fn value(&self) -> &Self::Value {
        &self.runtime_workspace_id
    }
}

#[derive(Clone, Debug)]
pub(super) struct NewAgentBranchChoice {
    pub(super) name: String,
    pub(super) label: String,
}

impl SelectItem for NewAgentBranchChoice {
    type Value = String;

    fn title(&self) -> SharedString {
        SharedString::from(self.label.clone())
    }

    fn value(&self) -> &Self::Value {
        &self.name
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum NewTabKind {
    #[default]
    Agent,
    Terminal,
    Command,
}

/// Codex-style dual mode: Plan produces an approach first and then acts; Build
/// goes straight into implementation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum NewAgentMode {
    #[default]
    Plan,
    Build,
}

impl NewAgentMode {
    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Plan => "Plan",
            Self::Build => "Build",
        }
    }

    /// Map the picker's mode choice onto the Host launch transaction's typed
    /// mode. Plan-mode expression (CLI flags / verified keys / prompt prefix)
    /// is Host launch policy.
    pub(super) fn host_mode(self) -> shardlane_host::AgentLaunchMode {
        match self {
            Self::Plan => shardlane_host::AgentLaunchMode::Plan,
            Self::Build => shardlane_host::AgentLaunchMode::Build,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum NewAgentPermission {
    #[default]
    AskApproval,
    AutoApprove,
    FullAccess,
}

impl NewAgentPermission {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::AskApproval => "Ask for approval",
            Self::AutoApprove => "Auto-approve edits",
            Self::FullAccess => "Full access",
        }
    }

    /// Map the picker's permission choice onto the Host launch transaction's
    /// typed permission. Startup argv derivation lives in the Host.
    pub(super) fn host_permission(self) -> shardlane_host::AgentPermission {
        match self {
            Self::AskApproval => shardlane_host::AgentPermission::AskApproval,
            Self::AutoApprove => shardlane_host::AgentPermission::AutoApprove,
            Self::FullAccess => shardlane_host::AgentPermission::FullAccess,
        }
    }
}

pub(crate) struct NewAgentUiState {
    pub(super) tab: NewTabKind,
    pub(super) prompt: Entity<InputState>,
    /// Command input box for Terminal mode (independent from the agent prompt).
    pub(super) terminal_input: Entity<InputState>,
    pub(super) project: Entity<SelectState<SearchableVec<NewAgentProjectChoice>>>,
    pub(super) branch: Entity<SelectState<SearchableVec<NewAgentBranchChoice>>>,
    pub(super) mode: NewAgentMode,
    pub(super) permission: NewAgentPermission,
    pub(super) agent: AgentId,
    /// `None` means the CLI availability precheck is still running in the
    /// background; used only to annotate the menu, never blocks submission.
    pub(super) agent_availability: Option<HashSet<AgentId>>,
    pub(super) attachments: Vec<PathBuf>,
    /// Reference catalog snapshot (@ file index and slash-command catalog);
    /// built by a background task and read synchronously by the provider during
    /// completion queries. Rebuilt on project/agent switches.
    pub(super) file_index: Option<Arc<ProjectFileIndex>>,
    pub(super) command_catalog: Option<Arc<CommandCatalog>>,
    /// Catalog scan task handle (only a cancellation handle: dropped when the
    /// composer closes).
    pub(super) _reference_scan: Option<BackgroundJob<()>>,
    pub(super) branch_project_id: Option<String>,
    pub(super) branch_loading: bool,
    /// Mirror for Branch pill menu rendering; the SelectState entity remains the
    /// sole source of truth for the submission path.
    pub(super) branch_choices: Vec<NewAgentBranchChoice>,
    pub(super) submitting: bool,
    /// Two-click arming state of the inline delete button (P12-1):
    /// Some(script_id) means the row is armed; the deletion executes only on a
    /// second click. Naturally released when the composer closes
    /// (new_agent_ui = None).
    pub(super) script_delete_armed_id: Option<String>,
    /// Background CLI precheck task; exists only as a cancellation handle
    /// (dropped/cancelled when the composer closes).
    pub(super) _agent_scan: Option<BackgroundJob<()>>,
    pub(super) _subscriptions: Vec<Subscription>,
}

#[derive(Clone, Debug)]
pub(super) struct BranchSnapshot {
    pub(super) choices: Vec<NewAgentBranchChoice>,
    pub(super) selected_index: usize,
}

pub(super) fn composer_project_choices_from_visible(
    visible: &[VisibleSidebarProject],
) -> Vec<NewAgentProjectChoice> {
    visible
        .iter()
        .map(|project| NewAgentProjectChoice {
            runtime_workspace_id: project.runtime_workspace_id.clone(),
            label: project.label.clone(),
            project_path: project.project_path.clone().unwrap_or_default(),
        })
        .collect()
}

pub(super) fn focused_new_agent_project_index(
    projects: &[NewAgentProjectChoice],
    focused_workspace_id: Option<&str>,
) -> Option<usize> {
    focused_workspace_id.and_then(|focused| {
        projects
            .iter()
            .position(|project| project.runtime_workspace_id == focused)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn composer_defaults_to_plan_mode_and_first_agent() {
        assert_eq!(NewAgentMode::default(), NewAgentMode::Plan);
        assert_eq!(NewAgentMode::Build.label(), "Build");
        assert_eq!(AgentId::ALL[0], AgentId::ClaudeCode);
    }

    #[test]
    fn fresh_composer_aligns_with_focused_runtime_project() {
        let projects = vec![
            NewAgentProjectChoice {
                runtime_workspace_id: "w2".into(),
                label: "First".into(),
                project_path: "/work/first".into(),
            },
            NewAgentProjectChoice {
                runtime_workspace_id: "w1".into(),
                label: "Previously Focused".into(),
                project_path: "/work/second".into(),
            },
        ];
        assert_eq!(
            focused_new_agent_project_index(&projects, Some("w1")),
            Some(1)
        );
        assert_eq!(focused_new_agent_project_index(&projects, None), None);
        assert_eq!(focused_new_agent_project_index(&projects, Some("w9")), None);
        assert_eq!(focused_new_agent_project_index(&[], Some("w1")), None);
    }
}
