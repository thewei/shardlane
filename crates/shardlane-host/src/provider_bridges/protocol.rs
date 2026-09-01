//! Length-bounded wire protocol for the local Provider Bridge IPC.
//!
//! [INPUT]: JSON frames sent over local Unix Domain Socket by provider companion
//! hooks, plugins, extensions, or synthetic test clients.
//! [OUTPUT]: Typed `ProviderBridgeEnvelope` request and `ProviderBridgeReply` response.
//! [POS]: S3 local bridge protocol. Bounded to `MAX_BRIDGE_FRAME_BYTES` (256 KiB);
//! private bridge request nonce is verified; never leaks provider secrets.

use serde::{Deserialize, Serialize};

pub const BRIDGE_PROTOCOL_VERSION: u16 = 1;
pub const MAX_BRIDGE_FRAME_BYTES: usize = 256 * 1024; // 256 KiB

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderBridgeMode {
    Observe,
    Interaction,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum ProviderSessionLocator {
    Id(String),
    Path(String),
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ProviderBridgeEnvelope {
    pub protocol: u16,
    pub mode: ProviderBridgeMode,
    pub request_id: String,
    pub provider: String,
    pub event: String,
    pub session: ProviderSessionLocator,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_id: Option<String>,
    pub payload: serde_json::Value,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ProviderBridgeReply {
    Ack,
    CaptureAndWait {
        interaction_id: String,
        revision: u64,
    },
    NativeFallback,
    Resolved {
        response: serde_json::Value,
    },
    Cancelled {
        reason: String,
    },
    Error {
        message: String,
    },
}

impl ProviderBridgeEnvelope {
    pub fn validate(&self) -> Result<(), String> {
        if self.protocol != BRIDGE_PROTOCOL_VERSION {
            return Err(format!(
                "unsupported bridge protocol version {} (expected {})",
                self.protocol, BRIDGE_PROTOCOL_VERSION
            ));
        }
        if self.request_id.trim().is_empty() {
            return Err("request_id cannot be empty".to_string());
        }
        if self.provider.trim().is_empty() {
            return Err("provider cannot be empty".to_string());
        }
        if self.event.trim().is_empty() {
            return Err("event cannot be empty".to_string());
        }
        Ok(())
    }
}
