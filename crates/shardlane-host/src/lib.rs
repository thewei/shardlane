//! UI-independent Shardlane Host contracts and the typed Herdr runtime adapter.
//!
//! This crate is the product/application boundary shared by the native macOS client and
//! remote clients (loopback Remote API). It contains no GPUI, gpui-component, AppKit,
//! network listener, or Relay ownership. Herdr remains the runtime authority behind the
//! `herdr` module's typed RPC wrappers and the `AgentRuntime` SPI implementation.
//!

pub mod agent_hooks;
pub mod agent_insight;
pub mod agent_integrations;
pub mod agent_launch;
pub mod agent_service;
pub mod agent_titles;
pub mod context_transfer;
pub mod conversation_delivery;
pub mod conversation_interactions;
pub mod conversation_queue;
pub mod conversation_service;
pub mod conversation_sessions;
pub mod conversations;
pub mod diagnostics;
pub mod dto;
pub mod herdr;
pub mod history_continuation;
pub mod ids;
pub mod live_handoff;
/// Backend-neutral Multiplexer API (docs/multiplexer-api.md): the runtime seam
/// consumed by the macOS shell and the Remote API. Herdr is the first adapter;
/// assembly happens only in `mux::registry::MuxRegistry`.
pub mod mux;
pub mod project_index;
pub mod provider_bridges;
pub mod provider_capabilities;
pub mod runtime;
pub mod services;
pub mod shared_tui;
pub mod workspace_config;

pub use agent_hooks::{
    parse_osc_agent_status, sniff_agent_from_process_and_title, AgentHookIpcServer, AgentHookMeta,
    AgentHookRegistry, AgentHookReport, HookActionOutcome, HookError, HookInstallStatus,
    CURRENT_HOOK_VERSION, DEFAULT_SOCKET_NAME,
};
pub use agent_insight::*;
pub use agent_integrations::*;
pub use agent_launch::*;
pub use agent_service::*;
pub use agent_titles::*;
pub use context_transfer::*;
pub use conversation_delivery::*;
pub use conversation_interactions::*;
pub use conversation_queue::*;
pub use conversation_service::*;
pub use conversation_sessions::*;
pub use conversations::*;
pub use dto::*;
pub use history_continuation::*;
pub use ids::*;
pub use live_handoff::*;
pub use project_index::*;
pub use provider_bridges::{
    BridgeRuntimeDescriptor, ClaudeBridgeAdapter, CodexBridgeAdapter, InMemorySessionResolver,
    InteractionCapturePolicy, LocalProviderBridgeServer, OpenCodeBridgeAdapter,
    ParsedInteractionPayload, PiBridgeAdapter, ProviderBridgeAdapter, ProviderBridgeEnvelope,
    ProviderBridgeMode, ProviderBridgeRegistry, ProviderBridgeReply, ProviderOverlayStore,
    ProviderSemanticOverlay, ProviderSessionLocator, ResolvedLiveSession, SessionLocatorResolver,
    SyntheticBridgeAdapter, BRIDGE_PROTOCOL_VERSION, MAX_BRIDGE_FRAME_BYTES,
};
pub use provider_capabilities::*;
pub use runtime::*;
pub use services::*;
pub use shared_tui::{HerdrTuiSession, TuiError, TuiEvent, TuiManager};
pub use workspace_config::*;
