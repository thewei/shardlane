//! Provider Bridge Registry and request/response dispatching.
//!
//! [INPUT]: Incoming `ProviderBridgeEnvelope` frames, capture interest configuration,
//! and provider-specific adapter mappings.
//! [OUTPUT]: Normalized semantic events and `ConversationInteractionBroker` dispatches.
//! [POS]: S3 provider bridge registry / S4 Claude bridge.

use super::protocol::{
    ProviderBridgeEnvelope, ProviderBridgeMode, ProviderBridgeReply, ProviderSessionLocator,
};
use crate::conversation_interactions::{
    BridgeResolutionDisposition, ConversationInteractionAnchor, ConversationInteractionBroker,
    ConversationInteractionKind, InteractionChoice, InteractionResolution, InteractionResponse,
    PermissionScope, ProviderInteractionRequest,
};
use crate::ids::{AgentRef, BridgeRequestId, ConversationId};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// Desktop/Remote client surface interest for interaction capture.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InteractionCapturePolicy {
    /// Chat surface is actively focused for this exact conversation -> capture is eligible.
    ChatActive(ConversationId),
    /// Terminal surface is active -> delegate immediately to native provider UI.
    TerminalActive,
    /// No interactive client attached -> fall back to native.
    None,
}

/// Session mapping resolver translating provider session locators into Host live identity.
pub trait SessionLocatorResolver: Send + Sync {
    fn resolve_locator(
        &self,
        provider: &str,
        session: &ProviderSessionLocator,
    ) -> Option<ResolvedLiveSession>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedLiveSession {
    pub conversation_id: ConversationId,
    pub agent_ref: AgentRef,
    pub occupant_fingerprint: String,
}

/// Simple in-memory session locator resolver for tests and direct bindings.
#[derive(Default)]
pub struct InMemorySessionResolver {
    sessions: RwLock<HashMap<(String, String), ResolvedLiveSession>>,
}

impl InMemorySessionResolver {
    pub fn new() -> Self {
        Self {
            sessions: RwLock::new(HashMap::new()),
        }
    }

    pub fn register(
        &self,
        provider: impl Into<String>,
        session_locator_key: impl Into<String>,
        resolved: ResolvedLiveSession,
    ) {
        if let Ok(mut lock) = self.sessions.write() {
            lock.insert((provider.into(), session_locator_key.into()), resolved);
        }
    }
}

impl SessionLocatorResolver for InMemorySessionResolver {
    fn resolve_locator(
        &self,
        provider: &str,
        session: &ProviderSessionLocator,
    ) -> Option<ResolvedLiveSession> {
        let key = match session {
            ProviderSessionLocator::Id(id) => id.clone(),
            ProviderSessionLocator::Path(path) => path.clone(),
        };
        let lock = self.sessions.read().ok()?;
        lock.get(&(provider.to_string(), key)).cloned()
    }
}

/// Provider-specific hook adapter for decoding requests and encoding responses.
pub trait ProviderBridgeAdapter: Send + Sync {
    fn provider_name(&self) -> &str;

    fn parse_interaction_request(
        &self,
        envelope: &ProviderBridgeEnvelope,
    ) -> Result<ParsedInteractionPayload, String>;

    fn encode_resolution_response(
        &self,
        envelope: &ProviderBridgeEnvelope,
        resolution: &InteractionResolution,
    ) -> Result<serde_json::Value, String>;
}

pub struct ParsedInteractionPayload {
    pub kind: ConversationInteractionKind,
    pub prompt: String,
    pub choices: Vec<InteractionChoice>,
    pub multiple: bool,
    pub allow_custom_text: bool,
    pub anchor: ConversationInteractionAnchor,
    pub allowed_scopes: Vec<PermissionScope>,
    pub expires_at_ms: Option<u64>,
}

/// Synthetic adapter used for bridge testing.
pub struct SyntheticBridgeAdapter;

impl ProviderBridgeAdapter for SyntheticBridgeAdapter {
    fn provider_name(&self) -> &str {
        "synthetic"
    }

    fn parse_interaction_request(
        &self,
        envelope: &ProviderBridgeEnvelope,
    ) -> Result<ParsedInteractionPayload, String> {
        let prompt = envelope
            .payload
            .get("prompt")
            .and_then(|v| v.as_str())
            .unwrap_or("Synthetic Question")
            .to_string();

        let mut choices = Vec::new();
        if let Some(arr) = envelope.payload.get("choices").and_then(|v| v.as_array()) {
            for item in arr {
                if let Some(id) = item.get("id").and_then(|v| v.as_str()) {
                    let label = item
                        .get("label")
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

        let allow_custom_text = envelope
            .payload
            .get("allow_custom_text")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

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
            allow_custom_text,
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
            InteractionResponse::Allow { scope } => Ok(serde_json::json!({
                "decision": "allow",
                "scope": scope
            })),
            InteractionResponse::Deny { reason } => Ok(serde_json::json!({
                "decision": "deny",
                "reason": reason
            })),
            InteractionResponse::Cancel => Ok(serde_json::json!({
                "decision": "cancel"
            })),
            InteractionResponse::DelegateToTerminal => Ok(serde_json::json!({
                "decision": "delegate_to_terminal"
            })),
        }
    }
}

/// Central registry managing capture interest and provider adapter dispatch.
pub struct ProviderBridgeRegistry {
    broker: Arc<ConversationInteractionBroker>,
    session_resolver: Arc<dyn SessionLocatorResolver>,
    adapters: RwLock<HashMap<String, Arc<dyn ProviderBridgeAdapter>>>,
    capture_policies: RwLock<HashMap<ConversationId, InteractionCapturePolicy>>,
}

impl ProviderBridgeRegistry {
    pub fn new(
        broker: Arc<ConversationInteractionBroker>,
        session_resolver: Arc<dyn SessionLocatorResolver>,
    ) -> Self {
        let registry = Self {
            broker,
            session_resolver,
            adapters: RwLock::new(HashMap::new()),
            capture_policies: RwLock::new(HashMap::new()),
        };
        registry.register_adapter(Arc::new(SyntheticBridgeAdapter));
        registry
    }

    pub fn register_adapter(&self, adapter: Arc<dyn ProviderBridgeAdapter>) {
        if let Ok(mut lock) = self.adapters.write() {
            lock.insert(adapter.provider_name().to_string(), adapter);
        }
    }

    pub fn set_capture_policy(
        &self,
        conversation_id: ConversationId,
        policy: InteractionCapturePolicy,
    ) {
        if let Ok(mut lock) = self.capture_policies.write() {
            lock.insert(conversation_id, policy);
        }
    }

    pub fn get_capture_policy(&self, conversation_id: &ConversationId) -> InteractionCapturePolicy {
        self.capture_policies
            .read()
            .ok()
            .and_then(|lock| lock.get(conversation_id).cloned())
            .unwrap_or(InteractionCapturePolicy::None)
    }

    /// Process an incoming bridge envelope from a provider hook/extension.
    pub fn handle_envelope(
        &self,
        envelope: &ProviderBridgeEnvelope,
        wait_responder: Option<Arc<dyn Fn(BridgeResolutionDisposition) + Send + Sync + 'static>>,
    ) -> ProviderBridgeReply {
        if let Err(err) = envelope.validate() {
            return ProviderBridgeReply::Error { message: err };
        }

        let resolved_session = match self
            .session_resolver
            .resolve_locator(&envelope.provider, &envelope.session)
        {
            Some(res) => res,
            None => {
                // Unknown/untracked session -> fall back immediately to native provider UI
                return ProviderBridgeReply::NativeFallback;
            }
        };

        match envelope.mode {
            ProviderBridgeMode::Observe => {
                // Non-blocking observation event. Acknowledged immediately.
                ProviderBridgeReply::Ack
            }
            ProviderBridgeMode::Interaction => {
                // Check capture policy
                let policy = self.get_capture_policy(&resolved_session.conversation_id);
                match policy {
                    InteractionCapturePolicy::ChatActive(ref active_id)
                        if *active_id == resolved_session.conversation_id =>
                    {
                        // Eligible for capture
                        let adapters_lock = match self.adapters.read() {
                            Ok(lock) => lock,
                            Err(_) => {
                                return ProviderBridgeReply::NativeFallback;
                            }
                        };
                        let adapter = match adapters_lock.get(&envelope.provider) {
                            Some(a) => Arc::clone(a),
                            None => {
                                return ProviderBridgeReply::NativeFallback;
                            }
                        };

                        let parsed = match adapter.parse_interaction_request(envelope) {
                            Ok(p) => p,
                            Err(e) => {
                                return ProviderBridgeReply::Error {
                                    message: format!("failed to parse interaction request: {e}"),
                                };
                            }
                        };

                        let request = ProviderInteractionRequest {
                            conversation_id: resolved_session.conversation_id,
                            agent_ref: resolved_session.agent_ref,
                            provider: envelope.provider.clone(),
                            kind: parsed.kind,
                            prompt: parsed.prompt,
                            choices: parsed.choices,
                            multiple: parsed.multiple,
                            allow_custom_text: parsed.allow_custom_text,
                            anchor: parsed.anchor,
                            occupant_fingerprint: resolved_session.occupant_fingerprint,
                            bridge_request_id: BridgeRequestId::new(&envelope.request_id),
                            allowed_scopes: parsed.allowed_scopes,
                            expires_at_ms: parsed.expires_at_ms,
                        };

                        let interaction = self.broker.publish_request(request, wait_responder);
                        ProviderBridgeReply::CaptureAndWait {
                            interaction_id: interaction.id.to_string(),
                            revision: interaction.revision,
                        }
                    }
                    _ => {
                        // Terminal active or no capture interest -> fall through to native provider prompt
                        ProviderBridgeReply::NativeFallback
                    }
                }
            }
        }
    }

    /// Encode a committed resolution back into the provider's reply shape.
    pub fn encode_resolution(
        &self,
        envelope: &ProviderBridgeEnvelope,
        resolution: &InteractionResolution,
    ) -> Result<serde_json::Value, String> {
        let adapters_lock = self
            .adapters
            .read()
            .map_err(|e| format!("failed to read adapters: {e}"))?;
        let adapter = adapters_lock
            .get(&envelope.provider)
            .ok_or_else(|| format!("no adapter registered for provider {}", envelope.provider))?;
        adapter.encode_resolution_response(envelope, resolution)
    }

    pub fn broker(&self) -> &Arc<ConversationInteractionBroker> {
        &self.broker
    }
}
