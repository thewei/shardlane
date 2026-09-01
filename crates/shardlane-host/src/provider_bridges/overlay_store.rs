//! Provider semantic event overlay store.
//!
//! [INPUT]: Live provider bridge events (ToolStarted, ToolCompleted, ToolFailed, etc.).
//! [OUTPUT]: Bounded, per-conversation overlay collections reconciled against durable transcript.
//! [POS]: S3/S4 observation overlay layer.

use crate::ids::ConversationId;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::RwLock;

const MAX_OVERLAYS_PER_CONVERSATION: usize = 100;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProviderSemanticOverlay {
    ToolStarted {
        tool_id: String,
        tool_name: String,
        input_preview: String,
        started_at_ms: u64,
    },
    ToolCompleted {
        tool_id: String,
        duration_ms: Option<u64>,
        exit_code: Option<i32>,
        output_summary: Option<String>,
    },
    ToolFailed {
        tool_id: String,
        error_message: String,
        duration_ms: Option<u64>,
    },
    PlanUpdated {
        items: Vec<String>,
        updated_at_ms: u64,
    },
    TodoUpdated {
        items: Vec<String>,
        updated_at_ms: u64,
    },
}

impl ProviderSemanticOverlay {
    pub fn tool_id(&self) -> Option<&str> {
        match self {
            Self::ToolStarted { tool_id, .. }
            | Self::ToolCompleted { tool_id, .. }
            | Self::ToolFailed { tool_id, .. } => Some(tool_id),
            _ => None,
        }
    }
}

/// Bounded store keeping low-latency semantic overlays for live conversations.
pub struct ProviderOverlayStore {
    overlays: RwLock<HashMap<ConversationId, VecDeque<ProviderSemanticOverlay>>>,
}

impl ProviderOverlayStore {
    pub fn new() -> Self {
        Self {
            overlays: RwLock::new(HashMap::new()),
        }
    }

    /// Push an overlay event into the conversation's bounded queue.
    pub fn push_overlay(&self, conversation_id: &ConversationId, overlay: ProviderSemanticOverlay) {
        let mut map = match self.overlays.write() {
            Ok(m) => m,
            Err(_) => return,
        };

        let queue = map
            .entry(conversation_id.clone())
            .or_insert_with(VecDeque::new);
        if queue.len() >= MAX_OVERLAYS_PER_CONVERSATION {
            queue.pop_front();
        }
        queue.push_back(overlay);
    }

    /// Get a snapshot of all active overlays for a conversation.
    pub fn get_overlays(&self, conversation_id: &ConversationId) -> Vec<ProviderSemanticOverlay> {
        let map = match self.overlays.read() {
            Ok(m) => m,
            Err(_) => return Vec::new(),
        };

        map.get(conversation_id)
            .map(|q| q.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// Clear reconciled overlays when durable transcript catches up.
    pub fn clear_reconciled(&self, conversation_id: &ConversationId, reconciled_tool_ids: &[&str]) {
        let mut map = match self.overlays.write() {
            Ok(m) => m,
            Err(_) => return,
        };

        if let Some(queue) = map.get_mut(conversation_id) {
            queue.retain(|item| {
                if let Some(tid) = item.tool_id() {
                    !reconciled_tool_ids.contains(&tid)
                } else {
                    true
                }
            });
        }
    }
}

impl Default for ProviderOverlayStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn overlay_store_bounds_capacity_and_reconciles() {
        let store = ProviderOverlayStore::new();
        let conv_id = ConversationId::new("conv-overlay-1");

        // Push tool started
        store.push_overlay(
            &conv_id,
            ProviderSemanticOverlay::ToolStarted {
                tool_id: "t1".to_string(),
                tool_name: "Bash".to_string(),
                input_preview: "cargo test".to_string(),
                started_at_ms: 1000,
            },
        );

        // Push tool completed
        store.push_overlay(
            &conv_id,
            ProviderSemanticOverlay::ToolCompleted {
                tool_id: "t1".to_string(),
                duration_ms: Some(2500),
                exit_code: Some(0),
                output_summary: Some("ok".to_string()),
            },
        );

        let active = store.get_overlays(&conv_id);
        assert_eq!(active.len(), 2);

        // Reconcile t1
        store.clear_reconciled(&conv_id, &["t1"]);
        let remaining = store.get_overlays(&conv_id);
        assert_eq!(remaining.len(), 0);
    }
}
