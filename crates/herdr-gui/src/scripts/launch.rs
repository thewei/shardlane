//! [INPUT]: Main-crate imports and sibling-module shared items passed through the scripts module root (super) via the `use super::*` chain
//! [OUTPUT]: Provides script runtime launching (launch target resolution, shell-ready waiting, pane/tab creation orchestration)
//! [POS]: The launch slice of the scripts module, mechanically split out of scripts.rs
#[cfg(test)]
use super::observe::script_foreground_pids;
use super::*;

const TASK_SHELL_READY_TIMEOUT: Duration = Duration::from_secs(5);

const TASK_SHELL_READY_POLL: Duration = Duration::from_millis(50);

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
) -> Result<(String, String, String), String> {
    let (workspace_id, tab_id, pane_id) = match target {
        ScriptProjectLaunchTarget::Existing(workspace_id) => {
            // Scripts are Project-scoped shortcuts, so prefer materializing a dedicated Pane in
            // an existing Project Tab instead of creating a parallel BackgroundJob-only Tab. This keeps
            // the runtime hierarchy aligned with the Sidebar's Project -> Tab -> Pane model.
            let panes = client
                .workspace_panes(&workspace_id)
                .map_err(|error| error.to_string())?;
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
                    .map_err(|error| error.to_string())?;
                let tab_id = created
                    .tab_id
                    .clone()
                    .or_else(|| target_pane.tab_id.clone())
                    .ok_or_else(|| "BackgroundJob Pane has no Tab identity".to_string())?;
                (workspace_id, tab_id, created.pane_id)
            } else {
                let created = client
                    .create_tab(&shardlane_host::mux::CreateTab {
                        workspace_id: Some(&workspace_id),
                        cwd: Some(script.project_path.as_str()),
                        focus: true,
                    })
                    .map_err(|error| error.to_string())?;
                (workspace_id, created.tab.tab_id, created.root_pane.pane_id)
            }
        }
        ScriptProjectLaunchTarget::CreateAt(project_path) => {
            let created_workspace = client
                .create_workspace(&shardlane_host::mux::CreateWorkspace {
                    cwd: Some(&project_path),
                    focus: true,
                })
                .map_err(|error| error.to_string())?;
            (
                created_workspace.workspace.workspace_id,
                created_workspace.tab.tab_id,
                created_workspace.root_pane.pane_id,
            )
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
        return Err(error);
    }
    let command = format!(
        "cd {} && {}\n",
        shardlane_history::resume::posix_quote(&script.project_path),
        script.assembled_command()
    );
    if let Err(error) = client.send_text(&pane_id, &command) {
        let _ = client.close_pane(&pane_id);
        return Err(error.to_string());
    }
    Ok((workspace_id, tab_id, pane_id))
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
}
