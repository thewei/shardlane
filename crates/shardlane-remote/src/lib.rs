//! Loopback Remote API: exposes the Shardlane Host boundary to remote
//! clients (the mobile side).
//!
//! [INPUT]: Depends on shardlane-host (DTOs/contracts + the Herdr runtime
//! adapter herdr.rs), axum/tokio (HTTP service on a dedicated thread),
//! serde (wire formats), subtle (constant-time token comparison), and uuid
//! (host_id/access_token generation)
//! [OUTPUT]: Exposes RemoteConfig (config model + identity generation),
//! spawn_remote_server (thread + dedicated Tokio runtime + 127.0.0.1
//! listener), and RemoteServerHandle (graceful shutdown)
//! [POS]: shardlane-remote is the network adapter for Host contracts; it owns
//! no runtime state and never touches GPUI; herdr-gui main (B6 wiring)
//! spawns/stops it per settings.
//! The single `/api/v2` surface (hello/bootstrap, Herdr workspace/tab/pane
//! control re-homed from the retired v1 compatibility routes, the events
//! WebSocket, semantic Conversation, shared Herdr TUI routes) is mounted in
//! server.rs; the TUI submodule holds only one Host-owned shared slot.

pub mod auth;
pub mod bootstrap;
pub mod config;
pub mod conversations;
pub mod cors;
pub mod error;
pub mod events;
pub mod idempotency;
pub mod instances;
pub mod server;
pub mod state;
pub mod tui;
pub(crate) mod tui_input;

pub use config::{ListenerMode, RemoteConfig, DEFAULT_REMOTE_PORT};
pub use error::ApiError;
pub use server::{
    hello_response, spawn_remote_server, HelloResponse, RemoteServerError, RemoteServerHandle,
    RemoteServerOptions,
};
pub use state::{RemoteState, WebConnection};
