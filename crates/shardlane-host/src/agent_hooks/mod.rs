//! Agent Hook management and status sniffing layer.
//!
//! [INPUT]: AgentId definitions from shardlane-history, local Unix Domain Socket connections,
//! and inline OSC 1337 escape sequences.
//! [OUTPUT]: AgentHookRegistry, AgentHookIpcServer, AgentHookReport, status
//! audit, and the Hook Adapter Layer (adapter: strict identity + journal).
//! [POS]: Autonomous Hook management and status sniffing across Herdr, tmux, and uuyc.
//! [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md

pub mod adapter;
pub mod ipc;
pub mod registry;
pub mod sniffing;

pub use ipc::{parse_osc_agent_status, AgentHookIpcServer, AgentHookReport, DEFAULT_SOCKET_NAME};
pub use registry::{
    AgentHookMeta, AgentHookRegistry, HookActionOutcome, HookError, HookInstallStatus,
    CURRENT_HOOK_VERSION,
};
pub use sniffing::sniff_agent_from_process_and_title;
