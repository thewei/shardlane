//! History surface (module family root): session list/filtering/Continue orchestration +
//! modern card-stream transcript rendering (full-width message cards / built-in roles with a
//! single per-message timestamp / tool-call terminal cards / Continue via choosing a Provider
//! in the Composer — the dedicated Continue header button was removed / scroll-edge lazy
//! infinite paging / compact expand-collapse capsule; notate 2026-08-29: Copy Markdown/Search/
//! Refresh moved into the titlebar, the in-page 42px breadcrumb bar removed, detail titles
//! truncated to one line).
//! Composed of responsibility-domain shards under history/: formatting/windowing/state/
//! browsing/transcript_source/resume/export/sidebar_view/page_view/transcript_view (this root
//! holds the import surface, mod declarations, re-exports, and cross-domain integration tests).
//!
//! [INPUT]: Depends on shardlane-history's catalog/scan/resume/CachedTranscriptWindow,
//! and on main.rs's ShardlaneApp/HerdrClient runtime projection and content_surface_theme.
//! [OUTPUT]: Exposes the history_page render entry and Sidebar metadata bootstrap (pub(crate)),
//! plus path-stable re-exports of HistoryUiState/history_db_path/history_session_is_live.
//! [POS]: herdr-gui's read-only history surface; Continue always goes through Herdr — this module holds no runtime.
use super::*;
use crate::assets::agent_brand_icon;
use crate::interaction::InteractiveSurfaceExt as _;
use crate::ui_metrics::{CONTENT_INSET, INTERACTIVE_FOCUS_OPACITY, SIDEBAR_EDGE_INSET, SPACE_ICON};
use crate::workspace_model::history_session_project_path_display;
use ::gpui::{img, uniform_list, StatefulInteractiveElement, Styled};
use gpui_component::{button::DropdownButton, h_flex, menu::PopupMenu, tooltip::Tooltip, v_flex};
use shardlane_history::{
    scan, AgentId, CachedTranscriptWindow, ConversationMeta, ConversationRef, HistoryAdapterRoster,
    HistoryCatalog, HistoryWatcher, ParsedTranscript,
};
use std::{collections::HashMap, collections::HashSet, path::PathBuf};

mod browsing;
mod export;
mod formatting;
pub(crate) use export::history_export_cached_window_markdown;
pub(crate) use formatting::history_last_active_short;
mod insights_view;
mod page_view;
mod resume;
mod sidebar_view;
mod state;
pub(crate) mod transcript_source;
mod transcript_view;
use export::*;
use formatting::*;
use insights_view::insights_panel;
use page_view::*;
use resume::*;
pub(crate) use state::HistoryUiState;
use state::*;
pub(crate) use transcript_source::history_db_path;
use transcript_source::*;
use transcript_view::*;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings;

    fn history_meta() -> ConversationMeta {
        ConversationMeta {
            key: "claude-code:session-1".to_string(),
            id: "session-1".to_string(),
            agent: AgentId::ClaudeCode,
            title: "History".to_string(),
            project_path: "/work/demo".to_string(),
            project_name: "demo".to_string(),
            file_path: "/tmp/session.jsonl".to_string(),
            created_at: 1,
            updated_at: 2,
            message_count: 1,
            size_bytes: 10,
            git_branch: None,
            model: None,
            tokens_used: None,
            archived: false,
            source: None,
        }
    }

    #[test]
    fn history_database_is_client_owned() {
        assert_eq!(
            history_db_path(),
            settings::app_data_dir().join("history.sqlite3")
        );
    }

    #[test]
    fn history_surface_starts_closed_and_empty() {
        let state = HistoryUiState::default();
        assert!(!state.open);
        assert!(!state.detail_only);
        assert!(!state.search_open);
        assert!(state.sessions.is_empty());
        assert!(state.transcript.is_none());
    }

    #[test]
    fn history_filters_apply_agent_and_project_as_and_constraints() {
        let session = history_meta();
        assert!(history_session_matches_filters(
            &session,
            None,
            None,
            HistoryTimeFilter::Any
        ));
        assert!(history_session_matches_filters(
            &session,
            Some(AgentId::ClaudeCode),
            Some("/work/demo"),
            HistoryTimeFilter::Any
        ));
        assert!(!history_session_matches_filters(
            &session,
            Some(AgentId::Codex),
            Some("/work/demo"),
            HistoryTimeFilter::Any
        ));
        assert!(!history_session_matches_filters(
            &session,
            Some(AgentId::ClaudeCode),
            Some("/work/other"),
            HistoryTimeFilter::Any
        ));
        // created_at/updated_at = 1/2 (ancient) → filtered out by any rolling window.
        assert!(!history_session_matches_filters(
            &session,
            None,
            None,
            HistoryTimeFilter::Last30Days
        ));
    }
}
