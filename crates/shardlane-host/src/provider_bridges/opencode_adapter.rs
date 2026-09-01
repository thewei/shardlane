use super::protocol::ProviderBridgeEnvelope;
use super::registry::{ParsedInteractionPayload, ProviderBridgeAdapter};
use crate::conversation_interactions::{
    ConversationInteractionAnchor, ConversationInteractionKind, InteractionChoice,
    InteractionResolution, InteractionResponse, PermissionScope,
};

/// Adapter for OpenCode companion plugin (`permission.asked`, `session.status`, `tool.execute.before`).
pub struct OpenCodeBridgeAdapter;

impl ProviderBridgeAdapter for OpenCodeBridgeAdapter {
    fn provider_name(&self) -> &str {
        "opencode"
    }

    fn parse_interaction_request(
        &self,
        envelope: &ProviderBridgeEnvelope,
    ) -> Result<ParsedInteractionPayload, String> {
        let event = envelope.event.as_str();

        if event == "permission.asked" || event == "permission" {
            let prompt = envelope
                .payload
                .get("title")
                .or_else(|| envelope.payload.get("prompt"))
                .or_else(|| envelope.payload.get("message"))
                .and_then(|v| v.as_str())
                .unwrap_or("OpenCode permission required")
                .to_string();

            let allow_session = envelope
                .payload
                .get("allow_session")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);

            let mut scopes = vec![PermissionScope::Once];
            if allow_session {
                scopes.push(PermissionScope::Session);
            }

            return Ok(ParsedInteractionPayload {
                kind: ConversationInteractionKind::Permission,
                prompt,
                choices: vec![],
                multiple: false,
                allow_custom_text: false,
                anchor: ConversationInteractionAnchor::Tail,
                allowed_scopes: scopes,
                expires_at_ms: None,
            });
        }

        // Generic Question
        let prompt = envelope
            .payload
            .get("prompt")
            .or_else(|| envelope.payload.get("message"))
            .and_then(|v| v.as_str())
            .unwrap_or("OpenCode requires your input")
            .to_string();

        let mut choices = Vec::new();
        if let Some(opts) = envelope.payload.get("options").and_then(|v| v.as_array()) {
            for opt in opts {
                if let Some(id) = opt
                    .get("id")
                    .or_else(|| opt.get("value"))
                    .and_then(|v| v.as_str())
                {
                    let label = opt
                        .get("label")
                        .or_else(|| opt.get("text"))
                        .and_then(|v| v.as_str())
                        .unwrap_or(id)
                        .to_string();
                    choices.push(InteractionChoice {
                        id: id.to_string(),
                        label,
                        description: None,
                        is_recommended: false,
                    });
                }
            }
        }

        let multiple = envelope
            .payload
            .get("multiple")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        Ok(ParsedInteractionPayload {
            kind: ConversationInteractionKind::Question,
            prompt,
            choices,
            multiple,
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
                let scope_str = match scope {
                    PermissionScope::Once => "once",
                    PermissionScope::Session => "session",
                    PermissionScope::Directory(d) => d.as_str(),
                    PermissionScope::Workspace => "workspace",
                    PermissionScope::Custom(c) => c.as_str(),
                };
                Ok(serde_json::json!({
                    "decision": "allow",
                    "scope": scope_str
                }))
            }
            InteractionResponse::Deny { reason } => Ok(serde_json::json!({
                "decision": "deny",
                "reason": reason
            })),
            InteractionResponse::Choice { option_id } => Ok(serde_json::json!({
                "decision": "choice",
                "selected": option_id
            })),
            InteractionResponse::MultiChoice { option_ids } => Ok(serde_json::json!({
                "decision": "multi_choice",
                "selected": option_ids
            })),
            InteractionResponse::Text { value } => Ok(serde_json::json!({
                "decision": "text",
                "value": value
            })),
            InteractionResponse::DelegateToTerminal => Ok(serde_json::json!({
                "decision": "delegate_to_terminal"
            })),
            InteractionResponse::Cancel => Ok(serde_json::json!({
                "decision": "cancel"
            })),
        }
    }
}
