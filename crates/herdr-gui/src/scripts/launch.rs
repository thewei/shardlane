//! [INPUT]: Main-crate imports and sibling-module shared items passed through the scripts module root (super) via the `use super::*` chain
//! [OUTPUT]: Provides script runtime launching (launch target resolution, pinned-Tab resolution + occupancy refusal, shell-ready waiting, pane/tab creation orchestration)
//! [POS]: The launch slice of the scripts module, mechanically split out of scripts.rs
use super::observe::script_foreground_pids;
use super::*;

const TASK_SHELL_READY_TIMEOUT: Duration = Duration::from_secs(5);

const TASK_SHELL_READY_POLL: Duration = Duration::from_millis(50);

/// Typed launch outcome. Occupancy is a pure refusal (the runtime is left
/// untouched), distinct from failures that may leave pieces behind.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum ScriptLaunchError {
    Occupied(String),
    Failed(String),
}

pub(super) fn launch_failed(error: impl std::fmt::Display) -> ScriptLaunchError {
    ScriptLaunchError::Failed(error.to_string())
}

/// Resolution of a Script's pinned Tab against the live instance snapshot.
/// The label is the semantic anchor (tab_id correlation is only a fast path);
/// a missing match materializes the Tab on run.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum PinnedTabMatch {
    Correlated(String),
    ByLabel(String),
    Missing,
}

pub(super) fn tab_display_label(tab: &Tab) -> &str {
    tab.label
        .as_deref()
        .or(tab.title.as_deref())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or_default()
}

pub(super) fn resolve_pinned_tab(
    pinned_label: &str,
    correlated_tab_id: Option<&str>,
    tabs: &[Tab],
    workspace_id: &str,
) -> PinnedTabMatch {
    if let Some(tab_id) = correlated_tab_id {
        if tabs
            .iter()
            .any(|tab| tab.tab_id == tab_id && tab.workspace_id.as_deref() == Some(workspace_id))
        {
            return PinnedTabMatch::Correlated(tab_id.to_string());
        }
    }
    if let Some(tab) = tabs.iter().find(|tab| {
        tab.workspace_id.as_deref() == Some(workspace_id) && tab_display_label(tab) == pinned_label
    }) {
        return PinnedTabMatch::ByLabel(tab.tab_id.clone());
    }
    PinnedTabMatch::Missing
}

/// Everything known about one Tab's business, gathered by the caller from the
/// same Herdr primitives the monitor already uses (process_info + agents).
#[derive(Clone, Debug, Default)]
pub(super) struct TabOccupancyProbe {
    pub pane_infos: Vec<PaneProcessInfo>,
    pub agents: Vec<Agent>,
}

/// Human-readable occupants of a Tab: foreground processes beyond the shell
/// plus live Agents whose tab_id matches. Empty means the Tab is free.
pub(super) fn tab_occupant_descriptions(probe: &TabOccupancyProbe, tab_id: &str) -> Vec<String> {
    let mut occupants = Vec::new();
    for info in &probe.pane_infos {
        let busy = script_foreground_pids(info);
        let Some(pid) = busy.first().copied() else {
            continue;
        };
        let process = info
            .foreground_processes
            .iter()
            .find(|process| process.pid == pid);
        let command = process
            .and_then(|process| {
                process
                    .cmdline
                    .clone()
                    .or_else(|| {
                        process
                            .argv
                            .as_ref()
                            .filter(|argv| !argv.is_empty())
                            .map(|argv| argv.join(" "))
                    })
                    .or_else(|| process.argv0.clone())
            })
            .unwrap_or_else(|| {
                process
                    .map(|process| process.name.clone())
                    .unwrap_or_else(|| "process".to_string())
            });
        occupants.push(format!(
            "{} (pid {pid})",
            crate::ui_metrics::single_line_label(&command)
        ));
    }
    for agent in &probe.agents {
        if agent.tab_id.as_deref() != Some(tab_id) {
            continue;
        }
        let name = agent
            .name
            .as_deref()
            .or(agent.display_agent.as_deref())
            .or(agent.agent.as_deref())
            .or(agent.title.as_deref())
            .unwrap_or("agent");
        occupants.push(format!("agent \"{name}\""));
    }
    occupants
}

fn pane_shell_ready(info: &PaneProcessInfo) -> bool {
    let Some(shell_pid) = info.shell_pid else {
        return false;
    };
    info.foreground_processes
        .iter()
        .all(|process| process.pid == shell_pid)
}

fn wait_for_script_shell(
    client: &dyn shardlane_host::mux::MultiplexerConnection,
    pane_id: &str,
) -> Result<(), String> {
    let started = Instant::now();
    loop {
        match client.pane_process_info(pane_id) {
            Ok(info) if pane_shell_ready(&info) => return Ok(()),
            Ok(_) => {}
            Err(error) if started.elapsed() >= TASK_SHELL_READY_TIMEOUT => {
                return Err(format!("script shell readiness failed: {error}"));
            }
            Err(_) => {}
        }
        if started.elapsed() >= TASK_SHELL_READY_TIMEOUT {
            return Err("script shell did not become ready within 5 seconds".to_string());
        }
        std::thread::sleep(TASK_SHELL_READY_POLL);
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum ScriptProjectLaunchTarget {
    Existing(String),
    CreateAt(String),
}

pub(super) fn resolve_script_project_launch_target(
    script: &ScriptRecord,
    project_index: &ProjectIndex,
) -> Result<ScriptProjectLaunchTarget, String> {
    let project_path = script.project_path.trim();
    if project_path.is_empty() {
        return Err("Script has no Project path".to_string());
    }
    if project_index
        .for_runtime_id(&script.workspace_id)
        .and_then(|project| project.project_path.as_deref())
        .is_some_and(|runtime_path| project_paths_match(runtime_path, project_path))
    {
        return Ok(ScriptProjectLaunchTarget::Existing(
            script.workspace_id.clone(),
        ));
    }
    if let Some(workspace_id) = project_index.runtime_workspace_id_for_project_path(project_path) {
        return Ok(ScriptProjectLaunchTarget::Existing(
            workspace_id.to_string(),
        ));
    }
    Ok(ScriptProjectLaunchTarget::CreateAt(
        project_path.to_string(),
    ))
}

pub(super) fn launch_script_runtime(
    client: &dyn shardlane_host::mux::MultiplexerConnection,
    script: &ScriptRecord,
    target: ScriptProjectLaunchTarget,
) -> Result<(String, String, String), ScriptLaunchError> {
    let pinned_label = script
        .pinned_tab_label
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let (workspace_id, tab_id, pane_id) = match target {
        ScriptProjectLaunchTarget::Existing(workspace_id) => {
            let panes = client
                .workspace_panes(&workspace_id)
                .map_err(launch_failed)?;
            match pinned_label {
                Some(label) => {
                    launch_pinned_tab_in_workspace(client, script, &workspace_id, label, panes)?
                }
                None => launch_auto_in_workspace(client, script, &workspace_id, panes)?,
            }
        }
        ScriptProjectLaunchTarget::CreateAt(project_path) => {
            let created_workspace = client
                .create_workspace(&shardlane_host::mux::CreateWorkspace {
                    cwd: Some(&project_path),
                    focus: true,
                })
                .map_err(launch_failed)?;
            let workspace_id = created_workspace.workspace.workspace_id;
            let tab_id = created_workspace.tab.tab_id;
            // A fresh workspace's root Tab becomes the pinned Tab; the rename is
            // best-effort (correlation still resolves until the Tab dies).
            if let Some(label) = pinned_label {
                if let Err(error) = client.rename_tab(&tab_id, label) {
                    lag_log(format_args!(
                        "script rename_tab failed tab={tab_id} label={label} error={error}"
                    ));
                }
            }
            (workspace_id, tab_id, created_workspace.root_pane.pane_id)
        }
    };

    // P12-4: a rename failure does not block the launch, but leaves a diagnostic
    // trail (same family of fix as new_agent/launch).
    if let Err(error) = client.rename_pane(&pane_id, &script.name) {
        lag_log(format_args!(
            "script rename_pane failed pane={pane_id} name={} error={error}",
            script.name
        ));
    }
    if let Err(error) = wait_for_script_shell(client, &pane_id) {
        let _ = client.close_pane(&pane_id);
        return Err(ScriptLaunchError::Failed(error));
    }
    let command = format!(
        "cd {} && {}\n",
        shardlane_history::resume::posix_quote(&script.project_path),
        script.assembled_command()
    );
    if let Err(error) = client.send_text(&pane_id, &command) {
        let _ = client.close_pane(&pane_id);
        return Err(launch_failed(error));
    }
    Ok((workspace_id, tab_id, pane_id))
}

// Scripts are Project-scoped shortcuts, so prefer materializing a dedicated Pane in
// an existing Project Tab instead of creating a parallel BackgroundJob-only Tab. This keeps
// the runtime hierarchy aligned with the Sidebar's Project -> Tab -> Pane model.
fn launch_auto_in_workspace(
    client: &dyn shardlane_host::mux::MultiplexerConnection,
    script: &ScriptRecord,
    workspace_id: &str,
    panes: Vec<Pane>,
) -> Result<(String, String, String), ScriptLaunchError> {
    if let Some(target_pane) = panes
        .iter()
        .find(|pane| pane.focused)
        .or_else(|| panes.first())
    {
        let created = client
            .split_pane(
                &target_pane.pane_id,
                shardlane_host::mux::SplitDirection::Down,
            )
            .map_err(launch_failed)?;
        let tab_id = created
            .tab_id
            .clone()
            .or_else(|| target_pane.tab_id.clone())
            .ok_or_else(|| ScriptLaunchError::Failed("Script Pane has no Tab identity".into()))?;
        Ok((workspace_id.to_string(), tab_id, created.pane_id))
    } else {
        let created = client
            .create_tab(&shardlane_host::mux::CreateTab {
                workspace_id: Some(workspace_id),
                cwd: Some(script.project_path.as_str()),
                focus: true,
            })
            .map_err(launch_failed)?;
        Ok((
            workspace_id.to_string(),
            created.tab.tab_id,
            created.root_pane.pane_id,
        ))
    }
}

/// Pinned placement: resolve the anchor Tab, refuse an occupied Tab without
/// touching the runtime, split into it, or materialize it when missing.
fn launch_pinned_tab_in_workspace(
    client: &dyn shardlane_host::mux::MultiplexerConnection,
    script: &ScriptRecord,
    workspace_id: &str,
    pinned_label: &str,
    panes: Vec<Pane>,
) -> Result<(String, String, String), ScriptLaunchError> {
    let tabs = client.navigation_state().map_err(launch_failed)?.tabs;
    match resolve_pinned_tab(pinned_label, script.tab_id.as_deref(), &tabs, workspace_id) {
        PinnedTabMatch::Correlated(tab_id) | PinnedTabMatch::ByLabel(tab_id) => {
            let tab_panes: Vec<&Pane> = panes
                .iter()
                .filter(|pane| pane.tab_id.as_deref() == Some(tab_id.as_str()))
                .collect();
            let mut pane_infos = Vec::new();
            for pane in &tab_panes {
                // A transient process_info failure skips that pane: occupancy is
                // best-effort (no reservation), the same race every multiplexer has.
                if let Ok(info) = client.pane_process_info(&pane.pane_id) {
                    pane_infos.push(info);
                }
            }
            let agents = client.agents().unwrap_or_default();
            let probe = TabOccupancyProbe { pane_infos, agents };
            let occupants = tab_occupant_descriptions(&probe, &tab_id);
            if !occupants.is_empty() {
                return Err(ScriptLaunchError::Occupied(format!(
                    "Pinned Tab \"{pinned_label}\" is occupied: {}",
                    occupants.join(" · ")
                )));
            }
            let target_pane = tab_panes
                .iter()
                .find(|pane| pane.focused)
                .or_else(|| tab_panes.first())
                .ok_or_else(|| {
                    ScriptLaunchError::Failed(format!("Pinned Tab \"{pinned_label}\" has no panes"))
                })?;
            let created = client
                .split_pane(
                    &target_pane.pane_id,
                    shardlane_host::mux::SplitDirection::Down,
                )
                .map_err(launch_failed)?;
            let tab_id = created
                .tab_id
                .clone()
                .or_else(|| target_pane.tab_id.clone())
                .ok_or_else(|| {
                    ScriptLaunchError::Failed("Script Pane has no Tab identity".into())
                })?;
            Ok((workspace_id.to_string(), tab_id, created.pane_id))
        }
        PinnedTabMatch::Missing => {
            let created = client
                .create_tab(&shardlane_host::mux::CreateTab {
                    workspace_id: Some(workspace_id),
                    cwd: Some(script.project_path.as_str()),
                    focus: true,
                })
                .map_err(launch_failed)?;
            // CreateTab carries no label (protocol gap): create + rename_tab is
            // the sanctioned two-step, same family as the best-effort rename_pane.
            if let Err(error) = client.rename_tab(&created.tab.tab_id, pinned_label) {
                lag_log(format_args!(
                    "script rename_tab failed tab={} label={pinned_label} error={error}",
                    created.tab.tab_id
                ));
            }
            Ok((
                workspace_id.to_string(),
                created.tab.tab_id,
                created.root_pane.pane_id,
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pane_shell_readiness_waits_for_startup_children_to_finish() {
        let busy = PaneProcessInfo {
            pane_id: "p1".into(),
            shell_pid: Some(10),
            tty: None,
            foreground_process_group_id: Some(10),
            foreground_processes: vec![
                crate::herdr::PaneProcessInfoProcess {
                    pid: 10,
                    name: "zsh".into(),
                    argv: None,
                    argv0: None,
                    cmdline: None,
                    cwd: None,
                },
                crate::herdr::PaneProcessInfoProcess {
                    pid: 11,
                    name: "omp".into(),
                    argv: None,
                    argv0: None,
                    cmdline: None,
                    cwd: None,
                },
            ],
        };
        assert!(!pane_shell_ready(&busy));
        assert_eq!(script_foreground_pids(&busy), vec![11]);

        let ready = PaneProcessInfo {
            foreground_processes: vec![crate::herdr::PaneProcessInfoProcess {
                pid: 10,
                name: "zsh".into(),
                argv: None,
                argv0: None,
                cmdline: None,
                cwd: None,
            }],
            ..busy
        };
        assert!(pane_shell_ready(&ready));
        assert!(script_foreground_pids(&ready).is_empty());
    }

    #[test]
    fn stale_script_runtime_workspace_id_resolves_through_stable_project_path() {
        let state = HerdrState {
            workspaces: vec![
                Workspace {
                    workspace_id: "w1".into(),
                    label: Some("Other".into()),
                    cwd: Some("/work/other".into()),
                    agent_status: None,
                    active_tab_id: None,
                    focused: false,
                    tab_count: Some(1),
                    pane_count: Some(1),
                    number: Some(1),
                },
                Workspace {
                    workspace_id: "w9".into(),
                    label: Some("Demo".into()),
                    cwd: Some("/work/demo".into()),
                    agent_status: None,
                    active_tab_id: None,
                    focused: true,
                    tab_count: Some(1),
                    pane_count: Some(1),
                    number: Some(2),
                },
            ],
            ..HerdrState::default()
        };
        let script = ScriptRecord {
            definition: ScriptDefinition {
                id: "script-stale".into(),
                project_path: "/work/demo/./".into(),
                ..ScriptDefinition::default()
            },
            workspace_id: "w1".into(),
            ..ScriptRecord::default()
        };
        let registry = ScriptRegistry {
            scripts: vec![script.clone()],
        };
        let index = build_project_index(&state, &registry);
        assert_eq!(
            resolve_script_project_launch_target(&script, &index),
            Ok(ScriptProjectLaunchTarget::Existing("w9".into()))
        );
    }

    #[test]
    fn script_without_live_workspace_recreates_stable_project_path() {
        let script = ScriptRecord {
            definition: ScriptDefinition {
                id: "script-offline".into(),
                project_path: "/work/demo".into(),
                ..ScriptDefinition::default()
            },
            workspace_id: "old-runtime".into(),
            ..ScriptRecord::default()
        };
        let registry = ScriptRegistry {
            scripts: vec![script.clone()],
        };
        let index = build_project_index(&HerdrState::default(), &registry);
        assert_eq!(
            resolve_script_project_launch_target(&script, &index),
            Ok(ScriptProjectLaunchTarget::CreateAt("/work/demo".into()))
        );
    }

    fn tab_fixture(tab_id: &str, workspace_id: &str, label: &str) -> Tab {
        serde_json::from_value(serde_json::json!({
            "tab_id": tab_id,
            "workspace_id": workspace_id,
            "label": label,
        }))
        .unwrap_or_else(|error| panic!("{error}"))
    }

    #[test]
    fn pinned_tab_resolution_prefers_live_correlation_then_label_then_creation() {
        let tabs = vec![
            tab_fixture("t-old", "w1", "old"),
            tab_fixture("t-dev", "w1", "dev"),
            tab_fixture("t-other", "w2", "dev"),
        ];
        // Live correlation wins even when a same-label Tab exists.
        assert_eq!(
            resolve_pinned_tab("dev", Some("t-old"), &tabs, "w1"),
            PinnedTabMatch::Correlated("t-old".into())
        );
        // Stale correlation falls to the label match within the same workspace.
        assert_eq!(
            resolve_pinned_tab("dev", Some("t-gone"), &tabs, "w1"),
            PinnedTabMatch::ByLabel("t-dev".into())
        );
        // Another workspace's same-label Tab never matches; missing pins create.
        assert_eq!(
            resolve_pinned_tab("dev", None, &tabs, "w9"),
            PinnedTabMatch::Missing
        );
    }

    fn process(
        pid: u32,
        name: &str,
        cmdline: Option<String>,
    ) -> crate::herdr::PaneProcessInfoProcess {
        crate::herdr::PaneProcessInfoProcess {
            pid,
            name: name.into(),
            argv: None,
            argv0: None,
            cmdline,
            cwd: None,
        }
    }

    #[test]
    fn tab_occupancy_reports_processes_and_agents_and_ignores_idle_panes() {
        let busy_shell_pid = 10;
        let busy = PaneProcessInfo {
            pane_id: "p1".into(),
            shell_pid: Some(busy_shell_pid),
            tty: None,
            foreground_process_group_id: Some(busy_shell_pid),
            foreground_processes: vec![
                process(busy_shell_pid, "zsh", None),
                process(11, "node", Some("pnpm dev".into())),
            ],
        };
        let idle = PaneProcessInfo {
            pane_id: "p1".into(),
            shell_pid: Some(busy_shell_pid),
            tty: None,
            foreground_process_group_id: Some(busy_shell_pid),
            foreground_processes: vec![process(busy_shell_pid, "zsh", None)],
        };
        let agent_here = serde_json::from_value::<Agent>(serde_json::json!({
            "terminal_id": "x", "tab_id": "t1", "name": "Claude"
        }))
        .unwrap_or_else(|error| panic!("{error}"));
        let agent_elsewhere = serde_json::from_value::<Agent>(serde_json::json!({
            "terminal_id": "y", "tab_id": "t2", "name": "Codex"
        }))
        .unwrap_or_else(|error| panic!("{error}"));
        let probe = TabOccupancyProbe {
            pane_infos: vec![busy, idle],
            agents: vec![agent_here, agent_elsewhere],
        };
        let occupants = tab_occupant_descriptions(&probe, "t1");
        assert_eq!(occupants.len(), 2);
        assert!(occupants[0].contains("pnpm dev"));
        assert!(occupants[0].contains("(pid 11)"));
        assert!(occupants[1].contains("agent \"Claude\""));

        let idle_only = PaneProcessInfo {
            pane_id: "p2".into(),
            shell_pid: Some(7),
            tty: None,
            foreground_process_group_id: Some(7),
            foreground_processes: vec![process(7, "zsh", None)],
        };
        let free = TabOccupancyProbe {
            pane_infos: vec![idle_only],
            agents: Vec::new(),
        };
        assert!(tab_occupant_descriptions(&free, "t1").is_empty());
    }
}
