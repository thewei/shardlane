use super::protocol::ProviderBridgeEnvelope;
use super::registry::{ParsedInteractionPayload, ProviderBridgeAdapter};
use crate::conversation_interactions::{
    ConversationInteractionAnchor, ConversationInteractionKind, InteractionChoice,
    InteractionResolution, InteractionResponse, PermissionScope,
};

/// Adapter for Claude Code CLI hooks (`PreToolUse: AskUserQuestion`, `PermissionRequest`).
pub struct ClaudeBridgeAdapter;

impl ProviderBridgeAdapter for ClaudeBridgeAdapter {
    fn provider_name(&self) -> &str {
        "claude-code"
    }

    fn parse_interaction_request(
        &self,
        envelope: &ProviderBridgeEnvelope,
    ) -> Result<ParsedInteractionPayload, String> {
        let hook_event_name = envelope
            .payload
            .get("hook_event_name")
            .or_else(|| envelope.payload.get("hookEventName"))
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let tool_name = envelope
            .payload
            .get("tool_name")
            .or_else(|| envelope.payload.get("toolName"))
            .and_then(|v| v.as_str())
            .unwrap_or("");

        // Handle PreToolUse -> AskUserQuestion
        if tool_name == "AskUserQuestion" || hook_event_name == "PreToolUse" {
            let tool_input = envelope
                .payload
                .get("tool_input")
                .or_else(|| envelope.payload.get("toolInput"))
                .cloned()
                .unwrap_or_else(|| serde_json::json!({}));

            let mut prompt = String::new();
            let mut choices = Vec::new();
            let mut multiple = false;

            if let Some(questions) = tool_input.get("questions").and_then(|q| q.as_array()) {
                if let Some(first_q) = questions.first() {
                    prompt = first_q
                        .get("question")
                        .or_else(|| first_q.get("prompt"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();

                    multiple = first_q
                        .get("multiple")
                        .or_else(|| first_q.get("allow_multiple"))
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);

                    if let Some(opts) = first_q.get("options").and_then(|o| o.as_array()) {
                        for opt in opts {
                            let id = opt
                                .get("id")
                                .or_else(|| opt.get("value"))
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string();
                            let label = opt
                                .get("label")
                                .or_else(|| opt.get("text"))
                                .and_then(|v| v.as_str())
                                .unwrap_or(&id)
                                .to_string();
                            let description = opt
                                .get("description")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string());

                            if !id.is_empty() || !label.is_empty() {
                                choices.push(InteractionChoice {
                                    id: if id.is_empty() { label.clone() } else { id },
                                    label,
                                    description,
                                    is_recommended: false,
                                });
                            }
                        }
                    }
                }
            } else if let Some(p) = envelope.payload.get("prompt").and_then(|v| v.as_str()) {
                prompt = p.to_string();
            }

            if prompt.is_empty() {
                prompt = "Claude needs your input".to_string();
            }

            return Ok(ParsedInteractionPayload {
                kind: ConversationInteractionKind::Question,
                prompt,
                choices,
                multiple,
                allow_custom_text: true,
                anchor: ConversationInteractionAnchor::Tail,
                allowed_scopes: vec![PermissionScope::Once],
                expires_at_ms: None,
            });
        }

        // Handle PermissionRequest / other tool confirmation
        let prompt = envelope
            .payload
            .get("prompt")
            .and_then(|v| v.as_str())
            .unwrap_or_else(|| {
                if let Some(_tool) = envelope.payload.get("tool_name").and_then(|v| v.as_str()) {
                    "Tool approval required"
                } else {
                    "Approval required"
                }
            })
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
        let tool_name = envelope
            .payload
            .get("tool_name")
            .or_else(|| envelope.payload.get("toolName"))
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if tool_name == "AskUserQuestion" {
            let tool_input = envelope
                .payload
                .get("tool_input")
                .or_else(|| envelope.payload.get("toolInput"))
                .cloned()
                .unwrap_or_else(|| serde_json::json!({}));

            let mut answers_map = serde_json::Map::new();
            match &resolution.response {
                InteractionResponse::Choice { option_id } => {
                    answers_map.insert("answer".to_string(), serde_json::json!(option_id));
                    answers_map.insert("0".to_string(), serde_json::json!(option_id));
                }
                InteractionResponse::MultiChoice { option_ids } => {
                    answers_map.insert("answers".to_string(), serde_json::json!(option_ids));
                    answers_map.insert("0".to_string(), serde_json::json!(option_ids));
                }
                InteractionResponse::Text { value } => {
                    answers_map.insert("answer".to_string(), serde_json::json!(value));
                    answers_map.insert("0".to_string(), serde_json::json!(value));
                }
                _ => {}
            }

            let mut updated_input = tool_input.as_object().cloned().unwrap_or_default();
            updated_input.insert(
                "answers".to_string(),
                serde_json::Value::Object(answers_map),
            );

            Ok(serde_json::json!({
                "hookSpecificOutput": {
                    "hookEventName": "PreToolUse",
                    "permissionDecision": "allow",
                    "updatedInput": updated_input
                }
            }))
        } else {
            // General permission decision
            match &resolution.response {
                InteractionResponse::Allow { .. } => Ok(serde_json::json!({
                    "hookSpecificOutput": {
                        "permissionDecision": "allow"
                    }
                })),
                InteractionResponse::Deny { reason } => Ok(serde_json::json!({
                    "hookSpecificOutput": {
                        "permissionDecision": "deny",
                        "reason": reason
                    }
                })),
                InteractionResponse::DelegateToTerminal => Ok(serde_json::json!({
                    "hookSpecificOutput": {
                        "permissionDecision": "ask"
                    }
                })),
                _ => Ok(serde_json::json!({
                    "hookSpecificOutput": {
                        "permissionDecision": "allow"
                    }
                })),
            }
        }
    }
}
