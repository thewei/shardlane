//! Herdr backend adapter: implements the neutral [`crate::mux`] contract by
//! delegating to the concrete `HerdrClient` / `HerdrTuiSession` and the Herdr
//! session helper functions, which stay in `crate::herdr` unchanged. All
//! Herdr-specific knowledge relevant to the neutral seam lives here.

use std::path::PathBuf;
use std::sync::Arc;

use super::{
    CreateTab, CreateWorkspace, InstanceListing, InstanceRef, InstanceTarget, Multiplexer,
    MultiplexerConnection, MultiplexerServerAdmin, MultiplexerStream, MuxAgentRuntime,
    MuxCapabilities, MuxDirection, MuxError, PaneHistory, SplitDirection,
};
use crate::herdr::{
    delete_session, list_sessions, read_session_display_name, stop_session,
    write_session_display_name, HerdrClient, HerdrSessionListing,
};
use crate::runtime::AgentRuntime;
use crate::shared_tui::HerdrTuiSession;

/// Pure mapping from one Herdr session listing row to the neutral instance
/// listing (extracted so the kit can contract-test it without a live CLI).
pub(crate) fn instance_listing(backend: &str, session: HerdrSessionListing) -> InstanceListing {
    InstanceListing {
        backend: backend.to_string(),
        display_name: read_session_display_name(&session.name),
        name: session.name,
        running: session.running,
        is_default: session.is_default,
    }
}

/// Adapter-local literal mapping: Herdr's protocol-20 direction vocabulary is
/// lower-case `left|right|up|down`, matching [`MuxDirection::as_str`].
fn direction_literal(direction: MuxDirection) -> &'static str {
    direction.as_str()
}

// --- Backend facade ---

#[derive(Clone, Copy, Debug, Default)]
pub struct HerdrBackend {
    admin: HerdrServerAdmin,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct HerdrServerAdmin;

impl MultiplexerServerAdmin for HerdrServerAdmin {
    fn cli_path(&self) -> Option<PathBuf> {
        crate::herdr::herdr_cli_path()
    }

    fn installed_cli_version(&self) -> Option<String> {
        crate::herdr::installed_cli_version()
    }

    fn user_config_path(&self) -> PathBuf {
        crate::herdr::herdr_user_config_path()
    }
}

impl Multiplexer for HerdrBackend {
    fn id(&self) -> &'static str {
        "herdr"
    }

    fn capabilities(&self) -> MuxCapabilities {
        MuxCapabilities {
            agents: true,
            server_admin: true,
            shared_tui: true,
            pane_history_read: true,
            // Protocol gap: tab.move has no destination-workspace primitive.
            cross_workspace_tab_move: false,
            events_push: true,
        }
    }

    fn list_instances(&self) -> Option<Vec<InstanceListing>> {
        Some(
            list_sessions()?
                .into_iter()
                .map(|session| instance_listing(self.id(), session))
                .collect(),
        )
    }

    fn rename_instance(&self, instance: &str, display_name: &str) -> Result<(), MuxError> {
        write_session_display_name(instance, display_name).map_err(MuxError::Api)
    }

    fn stop_instance(&self, instance: &str) -> Result<(), MuxError> {
        stop_session(instance).map_err(MuxError::Api)
    }

    fn delete_instance(&self, instance: &str) -> Result<(), MuxError> {
        delete_session(instance).map_err(MuxError::Api)
    }

    fn open_instance(
        &self,
        reference: &InstanceRef,
    ) -> Result<Arc<dyn MultiplexerConnection>, MuxError> {
        let client = match &reference.target {
            InstanceTarget::Default => HerdrClient::bootstrap()?,
            InstanceTarget::Named(session) => HerdrClient::bootstrap_for_session(session)?,
            InstanceTarget::Socket(path) => HerdrClient::connect_to(path)?,
        };
        Ok(Arc::new(client))
    }

    fn connect_instance(
        &self,
        reference: &InstanceRef,
    ) -> Result<Arc<dyn MultiplexerConnection>, MuxError> {
        let client = match &reference.target {
            // Env routing (`HERDR_SOCKET_PATH`/`HERDR_SESSION`) is intentional
            // for the default target: isolated smokes depend on it.
            InstanceTarget::Default => HerdrClient::connect()?,
            InstanceTarget::Named(session) => {
                HerdrClient::connect_to(&HerdrClient::session_socket_path(session))?
            }
            InstanceTarget::Socket(path) => HerdrClient::connect_to(path)?,
        };
        Ok(Arc::new(client))
    }

    fn server_admin(&self) -> Option<&dyn MultiplexerServerAdmin> {
        Some(&self.admin)
    }
}

// --- Per-instance connection ---

impl MultiplexerConnection for HerdrClient {
    fn capabilities(&self) -> MuxCapabilities {
        HerdrBackend::default().capabilities()
    }

    fn ping(&self) -> Result<(), MuxError> {
        HerdrClient::ping(self).map_err(MuxError::from)
    }

    fn protocol(&self) -> Option<u32> {
        HerdrClient::protocol(self)
    }

    fn server_started_with_supplied_config(&self) -> bool {
        HerdrClient::server_started_with_supplied_config(self)
    }

    fn navigation_state(&self) -> Result<crate::herdr::NavigationState, MuxError> {
        HerdrClient::navigation_state(self).map_err(MuxError::from)
    }

    fn visible_state(&self) -> Result<super::MuxState, MuxError> {
        HerdrClient::visible_state(self).map_err(MuxError::from)
    }

    fn host_bootstrap_state(&self) -> Result<super::MuxState, MuxError> {
        HerdrClient::host_bootstrap_state(self).map_err(MuxError::from)
    }

    fn workspace_state(&self) -> Result<super::MuxState, MuxError> {
        HerdrClient::workspace_state(self).map_err(MuxError::from)
    }

    fn workspace_panes(&self, workspace_id: &str) -> Result<Vec<crate::herdr::Pane>, MuxError> {
        HerdrClient::workspace_panes(self, workspace_id).map_err(MuxError::from)
    }

    fn tab_surface_state(
        &self,
        workspace_id: &str,
        tab_id: &str,
    ) -> Result<crate::herdr::TabSurfaceState, MuxError> {
        HerdrClient::tab_surface_state(self, workspace_id, tab_id).map_err(MuxError::from)
    }

    fn pane_layout(&self, pane_id: &str) -> Result<crate::herdr::PaneLayout, MuxError> {
        HerdrClient::pane_layout(self, pane_id).map_err(MuxError::from)
    }

    fn agents(&self) -> Result<Vec<crate::herdr::Agent>, MuxError> {
        HerdrClient::agents(self).map_err(MuxError::from)
    }

    fn subscribe_events(&self) -> Result<async_channel::Receiver<super::MuxEvent>, MuxError> {
        HerdrClient::subscribe_events(self).map_err(MuxError::from)
    }

    fn subscribe_pane_events(
        &self,
        pane_ids: &[String],
    ) -> Result<async_channel::Receiver<super::MuxEvent>, MuxError> {
        HerdrClient::subscribe_pane_events(self, pane_ids).map_err(MuxError::from)
    }

    fn create_workspace(
        &self,
        params: &CreateWorkspace<'_>,
    ) -> Result<crate::herdr::WorkspaceCreatedResult, MuxError> {
        HerdrClient::create_workspace_at_with_focus(self, params.cwd, params.focus)
            .map_err(MuxError::from)
    }

    fn close_workspace(&self, workspace_id: &str) -> Result<(), MuxError> {
        HerdrClient::close_workspace(self, workspace_id).map_err(MuxError::from)
    }

    fn rename_workspace(&self, workspace_id: &str, label: &str) -> Result<(), MuxError> {
        HerdrClient::rename_workspace(self, workspace_id, label).map_err(MuxError::from)
    }

    fn move_workspace(&self, workspace_id: &str, insert_index: usize) -> Result<(), MuxError> {
        HerdrClient::move_workspace(self, workspace_id, insert_index).map_err(MuxError::from)
    }

    fn move_workspace_before(
        &self,
        workspace_id: &str,
        before_workspace_id: &str,
    ) -> Result<(), MuxError> {
        HerdrClient::move_workspace_before(self, workspace_id, before_workspace_id)
            .map_err(MuxError::from)
    }

    fn workspace_focus(&self, workspace_id: &str) -> Result<(), MuxError> {
        HerdrClient::workspace_focus(self, workspace_id).map_err(MuxError::from)
    }

    fn create_tab(
        &self,
        params: &CreateTab<'_>,
    ) -> Result<crate::herdr::TabCreatedResult, MuxError> {
        HerdrClient::create_tab_at_with_focus(self, params.workspace_id, params.cwd, params.focus)
            .map_err(MuxError::from)
    }

    fn close_tab(&self, tab_id: &str) -> Result<(), MuxError> {
        HerdrClient::close_tab(self, tab_id).map_err(MuxError::from)
    }

    fn rename_tab(&self, tab_id: &str, label: &str) -> Result<(), MuxError> {
        HerdrClient::rename_tab(self, tab_id, label).map_err(MuxError::from)
    }

    fn move_tab(&self, tab_id: &str, insert_index: usize) -> Result<(), MuxError> {
        HerdrClient::move_tab(self, tab_id, insert_index).map_err(MuxError::from)
    }

    fn tab_focus(&self, tab_id: &str) -> Result<(), MuxError> {
        HerdrClient::tab_focus(self, tab_id).map_err(MuxError::from)
    }

    fn split_pane(
        &self,
        pane_id: &str,
        direction: SplitDirection,
    ) -> Result<crate::herdr::Pane, MuxError> {
        match direction {
            SplitDirection::Right => HerdrClient::split_right(self, pane_id),
            SplitDirection::Down => HerdrClient::split_down(self, pane_id),
        }
        .map_err(MuxError::from)
    }

    fn close_pane(&self, pane_id: &str) -> Result<(), MuxError> {
        HerdrClient::close_pane(self, pane_id).map_err(MuxError::from)
    }

    fn rename_pane(&self, pane_id: &str, label: &str) -> Result<(), MuxError> {
        HerdrClient::rename_pane(self, pane_id, label).map_err(MuxError::from)
    }

    fn swap_pane(
        &self,
        pane_id: &str,
        direction: MuxDirection,
    ) -> Result<crate::herdr::PaneLayoutActionResult, MuxError> {
        HerdrClient::swap_pane(self, pane_id, direction_literal(direction)).map_err(MuxError::from)
    }

    fn resize_pane(
        &self,
        pane_id: &str,
        direction: MuxDirection,
    ) -> Result<crate::herdr::PaneLayoutActionResult, MuxError> {
        HerdrClient::resize_pane(self, pane_id, direction_literal(direction))
            .map_err(MuxError::from)
    }

    fn toggle_pane_zoom(
        &self,
        pane_id: &str,
    ) -> Result<crate::herdr::PaneLayoutActionResult, MuxError> {
        HerdrClient::toggle_pane_zoom(self, pane_id).map_err(MuxError::from)
    }

    fn pane_focus(&self, pane_id: &str) -> Result<(), MuxError> {
        HerdrClient::pane_focus(self, pane_id).map_err(MuxError::from)
    }

    fn move_pane_to_tab(
        &self,
        pane_id: &str,
        tab_id: &str,
    ) -> Result<crate::herdr::PaneMoveResult, MuxError> {
        HerdrClient::move_pane_to_tab(self, pane_id, tab_id).map_err(MuxError::from)
    }

    fn move_pane_to_new_tab(
        &self,
        pane_id: &str,
        workspace_id: &str,
    ) -> Result<crate::herdr::PaneMoveResult, MuxError> {
        HerdrClient::move_pane_to_new_tab(self, pane_id, workspace_id).map_err(MuxError::from)
    }

    fn set_split_ratio(&self, tab_id: &str, path: &[bool], ratio: f64) -> Result<(), MuxError> {
        HerdrClient::set_split_ratio(self, tab_id, path, ratio).map_err(MuxError::from)
    }

    fn send_text(&self, pane_id: &str, text: &str) -> Result<(), MuxError> {
        HerdrClient::send_text(self, pane_id, text).map_err(MuxError::from)
    }

    fn send_keys(&self, pane_id: &str, keys: &[String]) -> Result<(), MuxError> {
        HerdrClient::send_keys(self, pane_id, keys).map_err(MuxError::from)
    }

    fn pane_process_info(&self, pane_id: &str) -> Result<crate::herdr::PaneProcessInfo, MuxError> {
        HerdrClient::pane_process_info(self, pane_id).map_err(MuxError::from)
    }

    fn read_pane_history(&self, pane_id: &str, lines: u32) -> Result<PaneHistory, MuxError> {
        let text =
            HerdrClient::read_pane_recent_ansi(self, pane_id, lines).map_err(MuxError::from)?;
        Ok(PaneHistory {
            pane_id: pane_id.to_string(),
            text,
        })
    }

    fn reload_config(&self) -> Result<(), MuxError> {
        HerdrClient::reload_config(self).map_err(MuxError::from)
    }

    fn agent_runtime(&self) -> Option<&dyn MuxAgentRuntime> {
        Some(self)
    }

    fn as_herdr(&self) -> Option<&HerdrClient> {
        Some(self)
    }
}

// --- Domain 7 — agent runtime facet ---

impl MuxAgentRuntime for HerdrClient {
    fn start_runtime_agent(
        &self,
        request: &crate::runtime::RuntimeAgentStartRequest,
    ) -> Result<crate::runtime::RuntimeAgent, MuxError> {
        AgentRuntime::start_runtime_agent(self, request).map_err(MuxError::from)
    }

    fn prompt_runtime_agent(
        &self,
        request: &crate::runtime::RuntimeAgentPromptRequest,
    ) -> Result<crate::runtime::RuntimeAgent, MuxError> {
        AgentRuntime::prompt_runtime_agent(self, request).map_err(MuxError::from)
    }

    fn read_runtime_agent(
        &self,
        request: &crate::runtime::RuntimeAgentReadRequest,
    ) -> Result<crate::runtime::RuntimeAgentRead, MuxError> {
        AgentRuntime::read_runtime_agent(self, request).map_err(MuxError::from)
    }

    fn wait_runtime_agent(
        &self,
        request: &crate::runtime::RuntimeAgentWaitRequest,
    ) -> Result<crate::runtime::RuntimeAgentWait, MuxError> {
        AgentRuntime::wait_runtime_agent(self, request).map_err(MuxError::from)
    }

    fn send_runtime_agent_keys(
        &self,
        agent_id: &crate::ids::AgentRef,
        keys: &[String],
    ) -> Result<(), MuxError> {
        AgentRuntime::send_runtime_agent_keys(self, agent_id, keys).map_err(MuxError::from)
    }

    fn report_pane_agent(&self, pane_id: &str, agent: &str) -> Result<(), MuxError> {
        HerdrClient::report_pane_agent(self, pane_id, agent).map_err(MuxError::from)
    }

    fn clear_pane_agent_authority(&self, pane_id: &str) -> Result<(), MuxError> {
        HerdrClient::clear_pane_agent_authority(self, pane_id).map_err(MuxError::from)
    }

    fn agent_focus(&self, target: &str) -> Result<(), MuxError> {
        HerdrClient::agent_focus(self, target).map_err(MuxError::from)
    }
}

// --- Domain 6 — terminal byte stream facet ---

impl MultiplexerStream for HerdrTuiSession {
    fn id(&self) -> &str {
        HerdrTuiSession::id(self)
    }

    fn summary(&self) -> super::StreamSummary {
        HerdrTuiSession::summary(self)
    }

    fn is_running(&self) -> bool {
        HerdrTuiSession::is_running(self)
    }

    fn viewer_count(&self) -> usize {
        HerdrTuiSession::viewer_count(self)
    }

    fn subscribe(&self) -> tokio::sync::broadcast::Receiver<super::StreamEvent> {
        HerdrTuiSession::subscribe(self)
    }

    fn subscribe_with_startup_replay(
        &self,
    ) -> (
        tokio::sync::broadcast::Receiver<super::StreamEvent>,
        Vec<u8>,
    ) {
        HerdrTuiSession::subscribe_with_startup_replay(self)
    }

    fn wake_subscribers(&self) {
        HerdrTuiSession::wake_subscribers(self)
    }

    fn send_bytes(&self, data: &[u8]) -> Result<(), super::StreamError> {
        HerdrTuiSession::send_bytes(self, data)
    }

    fn send_bytes_traced(
        &self,
        data: &[u8],
        trace_id: u64,
        coalescible: bool,
    ) -> Result<(), super::StreamError> {
        HerdrTuiSession::send_bytes_with_trace_kind(self, data, trace_id, coalescible)
    }

    fn resize(&self, cols: u16, rows: u16) -> Result<super::StreamSummary, super::StreamError> {
        HerdrTuiSession::resize(self, cols, rows)
    }

    fn force_redraw(&self) -> Result<(), super::StreamError> {
        HerdrTuiSession::force_redraw(self)
    }

    fn stop(&self) -> bool {
        HerdrTuiSession::stop(self)
    }

    fn stop_with_reap(&self, reap_deadline: Option<std::time::Duration>) -> bool {
        HerdrTuiSession::stop_with_reap(self, reap_deadline)
    }

    fn wait_for_exit(&self, timeout: std::time::Duration) -> bool {
        HerdrTuiSession::wait_for_exit(self, timeout)
    }
}
