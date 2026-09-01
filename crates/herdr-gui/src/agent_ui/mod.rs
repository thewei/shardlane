//! Shared Agent UI presentation primitives (agent_ui).
//!
//! The Agent conversation presentation layer shared by New Agent / History
//! Detail / Live Chat: `composer` is the single Composer card implementation,
//! extracted bottom-up from the New Agent page. This module owns presentation
//! primitives only — no Herdr calls, no provider lifecycle, no New Agent launch
//! orchestration, no Chat semantic source, no History catalog/paging, or any
//! other lifecycle.

pub mod activity;
pub mod composer;
pub mod conversation;
pub mod conversation_surface;
pub mod conversation_view;
pub mod find;
pub mod interaction_view;
pub mod markdown;
pub mod scrollbar;

pub use composer::composer_send_state;
pub(crate) use composer::AgentComposer;
