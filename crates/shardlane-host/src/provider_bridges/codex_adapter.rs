use super::protocol::ProviderBridgeEnvelope;
use super::registry::{ParsedInteractionPayload, ProviderBridgeAdapter};
use crate::conversation_interactions::{
    ConversationInteractionAnchor, ConversationInteractionKind, InteractionChoice,
    InteractionResolution, InteractionResponse, PermissionScope,
};

/// Adapter for Codex CLI companion hook (`PreToolUse: PermissionRequest`, `request_user_input`).
pub struct CodexBridgeAdapter;

impl ProviderBridgeAdapter for CodexBridgeAdapter {
    fn provider_name(&self) -> &str {
        "codex"
    }

    fn parse_interaction_request(
        &self,
        envelope: &ProviderBridgeEnvelope,
    ) -> Result<ParsedInteractionPayload, String> {
        let event = envelope.event.as_str();

        if event == "PermissionRequest" || event == "pre_tool_use" {
            let prompt = envelope
                .payload
                .get("command")
                .or_else(|| envelope.payload.get("prompt"))
                .or_else(|| envelope.payload.get("message"))
                .and_then(|v| v.as_str())
                .unwrap_or("Codex permission required")
                .to_string();

            return Ok(ParsedInteractionPayload {
                kind: ConversationInteractionKind::Permission,
                prompt,
                choices: vec![],
                multiple: false,
                allow_custom_text: false,
                anchor: ConversationInteractionAnchor::Tail,
                allowed_scopes: vec![PermissionScope::Once, PermissionScope::Session],
                expires_at_ms: None,
            });
        }

        // Generic Question fallback for Codex request_user_input
        let prompt = envelope
            .payload
            .get("prompt")
            .or_else(|| envelope.payload.get("message"))
            .and_then(|v| v.as_str())
            .unwrap_or("Codex needs your input")
            .to_string();

        let mut choices = Vec::new();
        if let Some(opts) = envelope
            .payload
            .get("choices")
            .or_else(|| envelope.payload.get("options"))
            .and_then(|v| v.as_array())
        {
            for opt in opts {
                if let Some(text) = opt.as_str() {
                    choices.push(InteractionChoice {
                        id: text.to_string(),
                        label: text.to_string(),
                        description: None,
                        is_recommended: false,
                    });
                }
            }
        }

        Ok(ParsedInteractionPayload {
            kind: ConversationInteractionKind::Question,
            prompt,
            choices,
            multiple: false,
            allow_custom_text: true,
            anchor: ConversationInteractionAnchor::Tail,
            allowed_scopes: vec![PermissionScope::Once],
            expires_at_ms: None,
        })
    }

    fn encode_resolution_response(
        &self,
        _envelope: &ProviderBridgeEnvelope,
        resolution: &InteractionResolution,
    ) -> Result<serde_json::Value, String> {
        match &resolution.response {
            InteractionResponse::Allow { scope } => {
                let persistent = matches!(scope, PermissionScope::Session);
                Ok(serde_json::json!({
                    "decision": "allow",
                    "persistent": persistent
                }))
            }
            InteractionResponse::Deny { reason } => Ok(serde_json::json!({
                "decision": "deny",
                "reason": reason
            })),
            InteractionResponse::Choice { option_id } => Ok(serde_json::json!({
                "decision": "input",
                "value": option_id
            })),
            InteractionResponse::Text { value } => Ok(serde_json::json!({
                "decision": "input",
                "value": value
            })),
            InteractionResponse::DelegateToTerminal | InteractionResponse::Cancel => {
                Ok(serde_json::json!({
                    "decision": "delegate"
                }))
            }
            _ => Ok(serde_json::json!({
                "decision": "allow",
                "persistent": false
            })),
        }
    }
}
