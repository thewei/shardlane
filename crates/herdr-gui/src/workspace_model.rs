//! Project projection derived from Herdr runtime state.
//!
//! Product terminology is intentionally asymmetric here:
//! - Workspace/instance is Herdr's (enumerated live; the legacy client-side Workspace
//!   grouping registry was deleted with the 2026-09-01 multi-instance cleanup).
//! - Project is the Shardlane projection of a Herdr runtime `Workspace`.
//!
//! Herdr remains authoritative for runtime workspace IDs, tabs, panes, agents, and process
//! lifecycle. This module only derives stable Project identity/search context from runtime
//! state plus persisted Shardlane Script metadata.

use crate::herdr::HerdrState;
use crate::scripts::ScriptRegistry;
use std::path::Path;

// The Project projection core (ProjectKey/ProjectProjection/ProjectIndex/project_paths_match)
// moved into shardlane-host::project_index — desktop and remote bootstrap share one implementation.
// Re-exports here keep GUI-internal paths stable and provide a convenient builder adapted to ScriptRegistry.
pub(crate) use shardlane_host::project_index::{
    non_empty, project_paths_match, ProjectIndex, ProjectProjection,
};

/// GUI convenience builder for ProjectIndex: runtime projection merged with ScriptRegistry (id, path) pairs.
pub(crate) fn build_project_index(state: &HerdrState, scripts: &ScriptRegistry) -> ProjectIndex {
    ProjectIndex::build_from_state(state).with_script_paths(
        scripts
            .scripts
            .iter()
            .map(|script| (script.id.clone(), script.project_path.clone())),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::herdr::{Agent, Pane, Tab, Workspace};
    use crate::scripts::{ScriptDefinition, ScriptRecord};
    use shardlane_host::project_index::normalized_project_path;

    fn runtime_workspace(id: &str, cwd: Option<&str>) -> Workspace {
        Workspace {
            workspace_id: id.into(),
            label: None,
            cwd: cwd.map(str::to_string),
            agent_status: None,
            active_tab_id: None,
            focused: false,
            tab_count: None,
            pane_count: None,
            number: None,
        }
    }

    fn pane(id: &str, workspace_id: &str, cwd: Option<&str>) -> Pane {
        Pane {
            pane_id: id.into(),
            terminal_id: None,
            workspace_id: Some(workspace_id.into()),
            tab_id: None,
            label: None,
            title: None,
            terminal_title: None,
            cwd: cwd.map(str::to_string),
            agent_status: None,
            agent: None,
            focused: false,
            scroll: None,
        }
    }

    #[test]
    fn project_index_aggregates_herdr_runtime_children() {
        let state = HerdrState {
            workspaces: vec![runtime_workspace("w1", Some("/work/demo"))],
            tabs: vec![Tab {
                tab_id: "t1".into(),
                workspace_id: Some("w1".into()),
                label: None,
                title: None,
                terminal_title: None,
                agent_status: None,
                pane_count: None,
                focused: false,
            }],
            panes: vec![pane("p1", "w1", None)],
            agents: vec![Agent {
                terminal_id: "term-1".into(),
                workspace_id: Some("w1".into()),
                ..Agent::default()
            }],
            ..HerdrState::default()
        };
        let scripts = ScriptRegistry {
            scripts: vec![ScriptRecord {
                definition: ScriptDefinition {
                    id: "script-1".into(),
                    project_path: "/work/demo".into(),
                    name: "web".into(),
                    ..ScriptDefinition::default()
                },
                workspace_id: "w1".into(),
                ..ScriptRecord::default()
            }],
        };
        let index = build_project_index(&state, &scripts);
        let project = index
            .for_runtime_id("w1")
            .unwrap_or_else(|| panic!("missing project projection"));
        assert_eq!(project.label, "demo");
        assert_eq!(project.tab_ids, vec!["t1"]);
        assert_eq!(project.pane_ids, vec!["p1"]);
        assert_eq!(project.agent_terminal_ids, vec!["term-1"]);
        assert_eq!(project.script_ids, vec!["script-1"]);
    }

    #[test]
    fn persisted_script_without_live_runtime_keeps_project_searchable() {
        let scripts = ScriptRegistry {
            scripts: vec![ScriptRecord {
                definition: ScriptDefinition {
                    id: "script-old".into(),
                    project_path: "/work/demo".into(),
                    ..ScriptDefinition::default()
                },
                workspace_id: "stale-runtime-id".into(),
                ..ScriptRecord::default()
            }],
        };
        let index = build_project_index(&HerdrState::default(), &scripts);
        let project = index
            .for_project_path("/work/demo")
            .unwrap_or_else(|| panic!("missing project-only projection"));
        assert_eq!(project.runtime_workspace_id, None);
        assert_eq!(project.script_ids, vec!["script-old"]);
    }

    #[test]
    fn stale_script_runtime_id_rejoins_live_project_by_stable_path() {
        let state = HerdrState {
            workspaces: vec![runtime_workspace("w9", Some("/work/demo"))],
            ..HerdrState::default()
        };
        let scripts = ScriptRegistry {
            scripts: vec![ScriptRecord {
                definition: ScriptDefinition {
                    id: "script-old".into(),
                    project_path: "/work/demo".into(),
                    ..ScriptDefinition::default()
                },
                workspace_id: "w1".into(),
                ..ScriptRecord::default()
            }],
        };
        let index = build_project_index(&state, &scripts);
        assert_eq!(
            index.runtime_workspace_id_for_project_path("/work/demo"),
            Some("w9")
        );
        assert_eq!(
            index
                .for_runtime_id("w9")
                .map(|project| project.script_ids.as_slice()),
            Some(["script-old".to_string()].as_slice())
        );
    }

    #[test]
    fn reused_runtime_workspace_id_cannot_steal_script_from_stable_project_path() {
        let state = HerdrState {
            workspaces: vec![
                runtime_workspace("w1", Some("/work/other")),
                runtime_workspace("w9", Some("/work/demo")),
            ],
            ..HerdrState::default()
        };
        let scripts = ScriptRegistry {
            scripts: vec![ScriptRecord {
                definition: ScriptDefinition {
                    id: "script-old".into(),
                    project_path: "/work/demo".into(),
                    ..ScriptDefinition::default()
                },
                workspace_id: "w1".into(),
                ..ScriptRecord::default()
            }],
        };
        let index = build_project_index(&state, &scripts);
        assert!(index
            .for_runtime_id("w1")
            .is_some_and(|project| project.script_ids.is_empty()));
        assert_eq!(
            index
                .for_runtime_id("w9")
                .map(|project| project.script_ids.as_slice()),
            Some(["script-old".to_string()].as_slice())
        );
    }

    #[test]
    fn project_paths_use_stable_normalized_identity() {
        assert!(project_paths_match("/work/demo", "/work/./demo"));
        assert!(!project_paths_match("/work/demo", "/work/other"));
    }

    #[test]
    fn symlinked_project_paths_share_identity() {
        let temp = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
        let real = temp.path().join("real-project");
        std::fs::create_dir_all(&real).unwrap_or_else(|error| panic!("{error}"));
        let link = temp.path().join("linked-project");
        std::os::unix::fs::symlink(&real, &link).unwrap_or_else(|error| panic!("{error}"));
        // After canonicalize resolves the symlink, both paths must map to the same key; repeated calls cover the cache-hit path.
        assert!(project_paths_match(
            real.to_str().unwrap_or_default(),
            link.to_str().unwrap_or_default(),
        ));
        assert!(project_paths_match(
            link.to_str().unwrap_or_default(),
            real.to_str().unwrap_or_default(),
        ));
    }

    #[test]
    fn nonexistent_project_path_falls_back_to_component_identity() {
        // Nonexistent paths still return a deterministic identity (components collection) and are repeatable.
        assert_eq!(
            normalized_project_path("/work/does/not/exist"),
            Some("/work/does/not/exist".to_string())
        );
        assert_eq!(
            normalized_project_path("/work/does/not/exist"),
            Some("/work/does/not/exist".to_string())
        );
        assert_eq!(normalized_project_path("   "), None);
    }
}

fn resolve_visible_sidebar_project_path(
    workspace_cwd: Option<&str>,
    projected: Option<&ProjectProjection>,
) -> Option<String> {
    projected
        .and_then(|project| project.project_path.clone())
        .or_else(|| non_empty(workspace_cwd).map(str::to_string))
        .filter(|path| !path.trim().is_empty())
}

/// Visible Project snapshot shared by the sidebar Projects section and the New Agent composer (the single list source of truth).
#[derive(Clone, Debug)]
pub(crate) struct VisibleSidebarProject {
    pub runtime_workspace_id: String,
    pub label: String,
    pub project_path: Option<String>,
}

/// Same order as the sidebar `Projects` list: the bound instance's Herdr runtime workspaces.
pub(crate) fn visible_sidebar_projects(
    state: &HerdrState,
    scripts: &ScriptRegistry,
) -> Vec<VisibleSidebarProject> {
    let project_index = build_project_index(state, scripts);
    visible_sidebar_projects_with_index(state, &project_index)
}

pub(crate) fn visible_sidebar_projects_with_index(
    state: &HerdrState,
    project_index: &ProjectIndex,
) -> Vec<VisibleSidebarProject> {
    state
        .workspaces
        .iter()
        .map(|workspace| {
            let runtime_workspace_id = workspace.workspace_id.clone();
            let projected = project_index.for_runtime_id(&runtime_workspace_id);
            let project_path =
                resolve_visible_sidebar_project_path(workspace.cwd.as_deref(), projected);
            let label = workspace
                .label
                .clone()
                .or_else(|| projected.map(|project| project.label.clone()))
                .or_else(|| {
                    project_path.as_deref().and_then(|path| {
                        Path::new(path)
                            .file_name()
                            .and_then(|name| name.to_str())
                            .map(str::to_string)
                    })
                })
                .or_else(|| {
                    workspace.cwd.as_deref().and_then(|cwd| {
                        Path::new(cwd)
                            .file_name()
                            .and_then(|name| name.to_str())
                            .map(str::to_string)
                    })
                })
                .unwrap_or_else(|| runtime_workspace_id.clone());
            VisibleSidebarProject {
                runtime_workspace_id,
                label,
                project_path,
            }
        })
        .collect()
    // Sorting source of truth = the authoritative order of Herdr `workspace.list`. Changes
    // to Herdr's own ordering are projected verbatim into the App; Shardlane's drag ordering
    // must write back via `workspace.move` and then wait for Herdr's order to flow back.
}

pub(crate) fn visible_sidebar_project_by_runtime_id<'a>(
    projects: &'a [VisibleSidebarProject],
    runtime_workspace_id: &str,
) -> Option<&'a VisibleSidebarProject> {
    projects
        .iter()
        .find(|project| project.runtime_workspace_id == runtime_workspace_id)
}

pub(crate) fn sidebar_project_path_for_context(
    projects: &[VisibleSidebarProject],
    context_runtime_id: Option<&str>,
    focused_runtime_id: Option<&str>,
) -> Option<String> {
    context_runtime_id
        .or(focused_runtime_id)
        .and_then(|runtime_id| {
            visible_sidebar_project_by_runtime_id(projects, runtime_id)
                .and_then(|project| project.project_path.clone())
        })
        .filter(|path| !path.trim().is_empty())
}

/// Project path resolution shared by the History session list/detail (catalog → ProjectIndex).
pub(crate) fn resolve_history_session_project_path(
    catalog_project_path: &str,
    catalog_project_name: &str,
    state: &HerdrState,
    scripts: &ScriptRegistry,
) -> Option<String> {
    let trimmed = catalog_project_path.trim();
    if !trimmed.is_empty() {
        return Some(trimmed.to_string());
    }
    let name = catalog_project_name.trim();
    if name.is_empty() {
        return None;
    }
    let index = build_project_index(state, scripts);
    for project in index.projects() {
        if project.label == name {
            return project
                .project_path
                .as_deref()
                .filter(|path| !path.trim().is_empty())
                .map(str::to_string);
        }
    }
    None
}

pub(crate) fn history_session_project_path_display(
    catalog_project_path: &str,
    catalog_project_name: &str,
    state: &HerdrState,
    scripts: &ScriptRegistry,
) -> String {
    resolve_history_session_project_path(catalog_project_path, catalog_project_name, state, scripts)
        .unwrap_or_else(|| "Unknown project".to_string())
}

#[cfg(test)]
mod visible_sidebar_project_tests {
    use super::*;
    use crate::herdr::Workspace;
    use shardlane_host::project_index::normalized_project_path;

    fn runtime_workspace_with_label(id: &str, label: &str, cwd: Option<&str>) -> Workspace {
        Workspace {
            workspace_id: id.into(),
            label: Some(label.into()),
            cwd: cwd.map(str::to_string),
            agent_status: None,
            active_tab_id: None,
            focused: false,
            tab_count: None,
            pane_count: None,
            number: None,
        }
    }

    fn runtime_workspace_with_number(
        id: &str,
        label: &str,
        cwd: Option<&str>,
        number: u32,
    ) -> Workspace {
        let mut workspace = runtime_workspace_with_label(id, label, cwd);
        workspace.number = Some(number);
        workspace
    }

    #[test]
    fn visible_projects_resolve_paths_from_runtime_identity_only() {
        let temp = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
        let project_a = temp.path().join("project-a");
        std::fs::create_dir_all(&project_a).unwrap_or_else(|error| panic!("{error}"));
        let path_a = project_a.to_string_lossy().into_owned();
        let state = HerdrState {
            focused_workspace_id: Some("w1".into()),
            workspaces: vec![
                runtime_workspace_with_label("w1", "project-a", Some(&path_a)),
                runtime_workspace_with_label("w2", "project-b", None),
            ],
            ..HerdrState::default()
        };
        let scripts = ScriptRegistry::default();
        let projects = visible_sidebar_projects(&state, &scripts);

        assert_eq!(projects.len(), 2);
        let w1 = projects
            .iter()
            .find(|project| project.runtime_workspace_id == "w1")
            .unwrap_or_else(|| panic!("missing w1"));
        assert_eq!(
            normalized_project_path(w1.project_path.as_deref().unwrap_or_default()),
            normalized_project_path(&path_a)
        );
        // Without cwd/script/label evidence, a runtime workspace stays runtime-only
        // (no path is invented for it).
        let w2 = projects
            .iter()
            .find(|project| project.runtime_workspace_id == "w2")
            .unwrap_or_else(|| panic!("missing w2"));
        assert_eq!(w2.project_path, None);
        assert_eq!(
            sidebar_project_path_for_context(&projects, Some("w2"), None),
            None
        );
        assert_eq!(
            sidebar_project_path_for_context(&projects, Some("w1"), Some("w2")),
            Some(path_a.clone())
        );
    }

    #[test]
    fn active_sidebar_project_path_follows_context_over_focus() {
        let temp = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
        let project_a = temp.path().join("a");
        let project_b = temp.path().join("b");
        std::fs::create_dir_all(&project_a).unwrap_or_else(|error| panic!("{error}"));
        std::fs::create_dir_all(&project_b).unwrap_or_else(|error| panic!("{error}"));
        let path_a = project_a.to_string_lossy().into_owned();
        let path_b = project_b.to_string_lossy().into_owned();

        let state = HerdrState {
            focused_workspace_id: Some("w1".into()),
            workspaces: vec![
                runtime_workspace_with_label("w1", "a", Some(&path_a)),
                runtime_workspace_with_label("w2", "b", Some(&path_b)),
            ],
            ..HerdrState::default()
        };
        let scripts = ScriptRegistry::default();
        let projects = visible_sidebar_projects(&state, &scripts);

        assert_eq!(
            sidebar_project_path_for_context(&projects, Some("w2"), Some("w1")),
            Some(path_b)
        );
    }

    #[test]
    fn history_session_project_path_resolves_from_project_name() {
        let temp = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
        let path = temp.path().join("named");
        std::fs::create_dir_all(&path).unwrap_or_else(|error| panic!("{error}"));
        let path_str = path.to_string_lossy().into_owned();
        let state = HerdrState {
            workspaces: vec![runtime_workspace_with_label("w1", "named", Some(&path_str))],
            ..HerdrState::default()
        };
        let scripts = ScriptRegistry::default();

        assert_eq!(
            history_session_project_path_display("", "named", &state, &scripts),
            path_str
        );
    }

    #[test]
    fn visible_projects_order_follows_herdr_runtime_order() {
        let temp = tempfile::tempdir().unwrap_or_else(|error| panic!("{error}"));
        let project_a = temp.path().join("a");
        let project_b = temp.path().join("b");
        std::fs::create_dir_all(&project_a).unwrap_or_else(|error| panic!("{error}"));
        std::fs::create_dir_all(&project_b).unwrap_or_else(|error| panic!("{error}"));
        let path_a = project_a.to_string_lossy().into_owned();
        let path_b = project_b.to_string_lossy().into_owned();

        let state = HerdrState {
            focused_workspace_id: Some("w1".into()),
            workspaces: vec![
                runtime_workspace_with_label("w1", "a", Some(&path_a)),
                runtime_workspace_with_label("w2", "b", Some(&path_b)),
                runtime_workspace_with_label("w3", "orphan", None),
            ],
            ..HerdrState::default()
        };
        let scripts = ScriptRegistry::default();
        let projects = visible_sidebar_projects(&state, &scripts);

        // The visible order must preserve Herdr `workspace.list`'s [w1, w2, w3] item by item.
        let order: Vec<&str> = projects
            .iter()
            .map(|project| project.runtime_workspace_id.as_str())
            .collect();
        assert_eq!(order, vec!["w1", "w2", "w3"]);
    }

    #[test]
    fn runtime_only_project_order_tracks_herdr_projection_reordering() {
        let scripts = ScriptRegistry::default();
        let project_order = |runtime_workspaces: Vec<Workspace>| {
            let state = HerdrState {
                workspaces: runtime_workspaces,
                ..HerdrState::default()
            };
            visible_sidebar_projects(&state, &scripts)
                .into_iter()
                .map(|project| project.runtime_workspace_id)
                .collect::<Vec<_>>()
        };

        let before = project_order(vec![
            runtime_workspace_with_number("w1", "one", None, 1),
            runtime_workspace_with_number("w2", "two", None, 2),
        ]);
        let after = project_order(vec![
            runtime_workspace_with_number("w2", "two", None, 2),
            runtime_workspace_with_number("w1", "one", None, 1),
        ]);
        assert_eq!(before, vec!["w1", "w2"]);
        assert_eq!(after, vec!["w2", "w1"]);
    }
}
