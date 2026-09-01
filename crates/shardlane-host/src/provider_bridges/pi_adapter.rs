use super::protocol::ProviderBridgeEnvelope;
use super::registry::{ParsedInteractionPayload, ProviderBridgeAdapter};
use crate::conversation_interactions::{
    ConversationInteractionAnchor, ConversationInteractionKind, InteractionChoice,
    InteractionResolution, InteractionResponse, PermissionScope,
};

/// Adapter for Pi Coding Agent extension bridge (`ctx.ui`, `session_start`, `tool_call`, `tool_result`).
pub struct PiBridgeAdapter;

impl ProviderBridgeAdapter for PiBridgeAdapter {
    fn provider_name(&self) -> &str {
        "pi"
    }

    fn parse_interaction_request(
        &self,
        envelope: &ProviderBridgeEnvelope,
    ) -> Result<ParsedInteractionPayload, String> {
        let event = envelope.event.as_str();

        if event == "select" || event == "confirm" || event == "input" {
            let prompt = envelope
                .payload
                .get("prompt")
                .or_else(|| envelope.payload.get("message"))
                .and_then(|v| v.as_str())
                .unwrap_or("Pi requires your input")
                .to_string();

            let mut choices = Vec::new();
            let mut multiple = false;

            if let Some(opts) = envelope.payload.get("options").and_then(|v| v.as_array()) {
                for opt in opts {
                    if let Some(text) = opt.as_str() {
                        choices.push(InteractionChoice {
                            id: text.to_string(),
                            label: text.to_string(),
                            description: None,
                            is_recommended: false,
                        });
                    } else if let Some(obj) = opt.as_object() {
                        let id = obj
                            .get("id")
                            .or_else(|| obj.get("value"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let label = obj
                            .get("label")
                            .or_else(|| obj.get("text"))
                            .and_then(|v| v.as_str())
                            .unwrap_or(&id)
                            .to_string();
                        if !id.is_empty() || !label.is_empty() {
                            choices.push(InteractionChoice {
                                id: if id.is_empty() { label.clone() } else { id },
                                label,
                                description: None,
                                is_recommended: false,
                            });
                        }
                    }
                }
            }

            if let Some(m) = envelope.payload.get("multiple").and_then(|v| v.as_bool()) {
                multiple = m;
            }

            let allow_custom = event == "input"
                || envelope
                    .payload
                    .get("allow_custom")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);

            return Ok(ParsedInteractionPayload {
                kind: ConversationInteractionKind::Question,
                prompt,
                choices,
                multiple,
                allow_custom_text: allow_custom,
                anchor: ConversationInteractionAnchor::Tail,
                allowed_scopes: vec![PermissionScope::Once],
                expires_at_ms: None,
            });
        }

        // Default to Permission request
        let prompt = envelope
            .payload
            .get("prompt")
            .and_then(|v| v.as_str())
            .unwrap_or("Pi tool execution requires permission")
            .to_string();

        Ok(ParsedInteractionPayload {
            kind: ConversationInteractionKind::Permission,
            prompt,
            choices: vec![],
            multiple: false,
            allow_custom_text: false,
            anchor: ConversationInteractionAnchor::Tail,
            allowed_scopes: vec![PermissionScope::Once, PermissionScope::Session],
            expires_at_ms: None,
        })
    }

    fn encode_resolution_response(
        &self,
        envelope: &ProviderBridgeEnvelope,
        resolution: &InteractionResolution,
    ) -> Result<serde_json::Value, String> {
        let event = envelope.event.as_str();

        match &resolution.response {
            InteractionResponse::Choice { option_id } => {
                if event == "confirm" {
                    let is_true = option_id == "true" || option_id == "yes" || option_id == "allow";
                    Ok(serde_json::json!({ "value": is_true }))
                } else {
                    Ok(serde_json::json!({ "value": option_id }))
                }
            }
            InteractionResponse::MultiChoice { option_ids } => {
                Ok(serde_json::json!({ "value": option_ids }))
            }
            InteractionResponse::Text { value } => Ok(serde_json::json!({ "value": value })),
            InteractionResponse::Allow { .. } => Ok(serde_json::json!({ "allowed": true })),
            InteractionResponse::Deny { reason } => {
                Ok(serde_json::json!({ "allowed": false, "reason": reason }))
            }
            InteractionResponse::DelegateToTerminal | InteractionResponse::Cancel => {
                Ok(serde_json::json!({ "cancelled": true }))
            }
        }
    }
}
