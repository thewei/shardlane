//! [INPUT]: The main-crate namespace and sibling-module public surface forwarded by the new_agent module root (super).
//! [OUTPUT]: Provides New Agent's GUI-side leftovers: display wrappers over the Host provider
//! capability projection, explicit terminal-command launches (`launch_terminal_command`, not an
//! Agent lifecycle), and the `ensure_agent_integration` shared with the history resume dead-session
//! path. Regular Agent launches have converged into `shardlane_host::run_agent_launch` (M2) and no
//! longer go through this file.
//! [POS]: The `crates/herdr-gui` new_agent submodule; the PTY-typing Agent launch path has been deleted.
use super::*;

const AGENT_SHELL_READY_TIMEOUT: Duration = Duration::from_secs(5);

const AGENT_READY_POLL: Duration = Duration::from_millis(60);

pub(super) fn agent_supports_native_plan_mode(agent: AgentId) -> bool {
    matches!(
        shardlane_host::provider_plan_support(agent),
        shardlane_host::ProviderPlanSupport::NativeCli
            | shardlane_host::ProviderPlanSupport::PostLaunchKeys
    )
}

pub(super) fn agent_supports_permission_flags(agent: AgentId) -> bool {
    shardlane_host::provider_permission_modes(agent)
}

fn wait_for_shell_ready(client: &HerdrClient, pane_id: &str) -> Result<(), String> {
    let started = Instant::now();
    loop {
        if let Ok(info) = client.pane_process_info(pane_id) {
            if let Some(shell_pid) = info.shell_pid {
                if info
                    .foreground_processes
                    .iter()
                    .all(|process| process.pid == shell_pid)
                {
                    return Ok(());
                }
            }
        }
        if started.elapsed() >= AGENT_SHELL_READY_TIMEOUT {
            return Err("Agent shell did not become ready within 5 seconds".to_string());
        }
        std::thread::sleep(AGENT_READY_POLL);
    }
}

/// Explicit terminal-command launch: this is NOT an Agent lifecycle path. The
/// Agent launch transaction lives in `shardlane_host::run_agent_launch`.
pub(super) fn launch_terminal_command(
    client: &HerdrClient,
    workspace_id: &str,
    cwd: &str,
    command: &str,
) -> Result<(TabCreatedResult, PaneLayout, String), String> {
    let created = client
        .create_tab_at(Some(workspace_id), Some(cwd))
        .map_err(|error| error.to_string())?;
    let tab_id = created.tab.tab_id.clone();
    let pane_id = created.root_pane.pane_id.clone();
    let cleanup_client = client.clone();
    let result = (|| {
        wait_for_shell_ready(client, &pane_id)?;
        client
            .send_text(&pane_id, &format!("{command}\n"))
            .map_err(|error| error.to_string())?;
        client
            .pane_layout(&pane_id)
            .map(|layout| (created, layout, workspace_id.to_string()))
            .map_err(|error| error.to_string())
    })();
    if result.is_err() {
        let _ = cleanup_client.close_tab(&tab_id);
    }
    result
}
