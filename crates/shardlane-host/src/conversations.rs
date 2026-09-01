//! Host-owned Conversation identity and semantic projection helpers.
//!
//! [INPUT]: Herdr `AgentSessionInfo` and shardlane-history normalized transcript/live models.
//! [OUTPUT]: Opaque Live/History Conversation IDs, exact session-source resolution, and the
//! bounded provider-neutral `ConversationItem` projection consumed by every client.
//! [POS]: shardlane-host's Conversation domain seam; it reads no socket,
//! guesses no cwd/mtime, and owns no runtime. Remote and the native GUI must
//! consume the same semantic mapping from here.

use crate::dto::{
    AgentStatus, ConversationIdentity, ConversationItem, ConversationItemKind, ConversationSource,
    ConversationSummary, ConversationToolCall,
};
use crate::herdr::AgentSessionInfo;
use crate::ids::{AgentRef, ConversationId, ProjectId};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use shardlane_history::models::{SessionFileRef, SessionMeta};
use shardlane_history::{
    adapter_for, create_adapters, resolve_session_source_locator, AgentId, HistoryCatalog,
    LiveSnapshot, MessageKind, Role, SessionSourceLocator, TranscriptMessage,
};
use thiserror::Error;

const CONVERSATION_ID_PREFIX: &str = "conv_1_";
/// v2 live ids bind the exact typed session occupant (AC-03). v1 ids remain
/// readable for transition clients but cannot prove an occupant and are rejected for
/// mutations.
const CONVERSATION_ID_V2_PREFIX: &str = "conv_2_";
/// Domain-separation key for the opaque session fingerprint (AC-03/AC-16).
const LIVE_SESSION_FINGERPRINT_KEY: &[u8] = b"shardlane.live-session.v1";

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ConversationLocator {
    /// v1 pane-scoped live locator (legacy). Reads resolve; mutations reject.
    Live(AgentRef),
    /// v2 session-exact live locator: pane target + opaque typed-session
    /// fingerprint. A replacement occupant produces a different fingerprint,
    /// so stale ids fail closed instead of retargeting (AC-03).
    LiveSession {
        agent_ref: AgentRef,
        fingerprint: String,
    },
    History(String),
}

#[derive(Debug, Error, Eq, PartialEq)]
pub enum ConversationProjectionError {
    #[error("unknown Herdr agent provider: {0}")]
    UnknownProvider(String),
    #[error("invalid ConversationId version")]
    UnsupportedVersion,
    #[error("invalid ConversationId encoding")]
    InvalidEncoding,
    #[error("invalid ConversationId locator")]
    InvalidLocator,
    #[error("conversation identity is stale; the pane now hosts a different session")]
    StaleOccupant,
}

/// Keyed opaque fingerprint over the typed Herdr session locator. One-way and
/// domain-separated: it binds provider + locator kind + value without exposing
/// a provider path or native id.
pub fn session_fingerprint(session: &AgentSessionInfo) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(LIVE_SESSION_FINGERPRINT_KEY);
    hasher.update([0u8]);
    hasher.update(session.agent.as_bytes());
    hasher.update([0u8]);
    hasher.update(session.kind.as_bytes());
    hasher.update([0u8]);
    hasher.update(session.source.as_bytes());
    hasher.update([0u8]);
    hasher.update(session.value.as_bytes());
    let digest = hasher.finalize();
    URL_SAFE_NO_PAD.encode(&digest[..12])
}

/// Encode a live Agent target + its typed session into a session-exact
/// ConversationId (AC-03). Clients must not parse it.
pub fn conversation_id_for_live_session(
    agent_ref: &AgentRef,
    session: &AgentSessionInfo,
) -> ConversationId {
    let payload = URL_SAFE_NO_PAD.encode(format!(
        "live2:{}:{}",
        agent_ref.as_str(),
        session_fingerprint(session)
    ));
    ConversationId::new(format!("{CONVERSATION_ID_V2_PREFIX}{payload}"))
}

/// Legacy v1 pane-scoped live id. Retained for read compatibility and
/// session-less projections only; semantic mutations require a v2 id because
/// only it can prove the exact session occupant.
pub fn conversation_id_for_live_agent(agent_ref: &AgentRef) -> ConversationId {
    encode_locator("live", agent_ref.as_str())
}

/// Encode an indexed History key into an opaque ConversationId. The provider key never
/// becomes a public path or client-side identity; only Host resolves it.
pub fn conversation_id_for_history_key(session_key: &str) -> ConversationId {
    encode_locator("history", session_key)
}

pub fn resolve_conversation_id(
    conversation_id: &ConversationId,
) -> Result<ConversationLocator, ConversationProjectionError> {
    if let Some(encoded) = conversation_id
        .as_str()
        .strip_prefix(CONVERSATION_ID_V2_PREFIX)
    {
        let payload = URL_SAFE_NO_PAD
            .decode(encoded)
            .map_err(|_| ConversationProjectionError::InvalidEncoding)?;
        let payload =
            String::from_utf8(payload).map_err(|_| ConversationProjectionError::InvalidEncoding)?;
        let body = payload
            .strip_prefix("live2:")
            .ok_or(ConversationProjectionError::InvalidLocator)?;
        // The fingerprint is the last colon-separated segment; pane ids may
        // themselves contain colons, so split from the right.
        let (agent_ref, fingerprint) = body
            .rsplit_once(':')
            .ok_or(ConversationProjectionError::InvalidLocator)?;
        if agent_ref.is_empty() || fingerprint.is_empty() {
            return Err(ConversationProjectionError::InvalidLocator);
        }
        return Ok(ConversationLocator::LiveSession {
            agent_ref: AgentRef::new(agent_ref.to_string()),
            fingerprint: fingerprint.to_string(),
        });
    }
    let encoded = conversation_id
        .as_str()
        .strip_prefix(CONVERSATION_ID_PREFIX)
        .ok_or(ConversationProjectionError::UnsupportedVersion)?;
    let payload = URL_SAFE_NO_PAD
        .decode(encoded)
        .map_err(|_| ConversationProjectionError::InvalidEncoding)?;
    let payload =
        String::from_utf8(payload).map_err(|_| ConversationProjectionError::InvalidEncoding)?;
    let (kind, value) = payload
        .split_once(':')
        .ok_or(ConversationProjectionError::InvalidLocator)?;
    if value.is_empty() {
        return Err(ConversationProjectionError::InvalidLocator);
    }
    match kind {
        "live" => Ok(ConversationLocator::Live(AgentRef::new(value))),
        "history" => Ok(ConversationLocator::History(value.to_string())),
        _ => Err(ConversationProjectionError::InvalidLocator),
    }
}

/// The live Agent target of a locator regardless of version (reads).
pub fn live_agent_ref(locator: &ConversationLocator) -> Option<&AgentRef> {
    match locator {
        ConversationLocator::Live(agent_ref) => Some(agent_ref),
        ConversationLocator::LiveSession { agent_ref, .. } => Some(agent_ref),
        ConversationLocator::History(_) => None,
    }
}

/// AC-03 mutation guard: prove the locator still points at the exact session
/// occupant. A v1 live id carries no fingerprint and fails closed; a v2 id
/// must match the current occupant's fingerprint.
pub fn validate_live_occupant(
    locator: &ConversationLocator,
    current: &AgentSessionInfo,
) -> Result<(), ConversationProjectionError> {
    match locator {
        ConversationLocator::LiveSession { fingerprint, .. } => {
            if session_fingerprint(current) == *fingerprint {
                Ok(())
            } else {
                Err(ConversationProjectionError::StaleOccupant)
            }
        }
        ConversationLocator::Live(_) => Err(ConversationProjectionError::StaleOccupant),
        ConversationLocator::History(_) => Err(ConversationProjectionError::InvalidLocator),
    }
}

fn encode_locator(kind: &str, value: &str) -> ConversationId {
    let payload = URL_SAFE_NO_PAD.encode(format!("{kind}:{value}"));
    ConversationId::new(format!("{CONVERSATION_ID_PREFIX}{payload}"))
}

/// Resolve the typed Herdr session identity. `kind` controls the locator semantics;
/// value shape is intentionally ignored, so a path/id can never be guessed from text.
pub fn resolve_agent_session_source(
    session: &AgentSessionInfo,
) -> Result<SessionSourceLocator, ConversationProjectionError> {
    let agent = agent_id_from_herdr_kind(&session.agent)
        .ok_or_else(|| ConversationProjectionError::UnknownProvider(session.agent.clone()))?;
    Ok(resolve_session_source_locator(
        agent,
        &session.kind,
        &session.source,
        &session.value,
    ))
}

/// Project the typed Herdr session identity into the public Conversation DTO.
///
/// Provider-native ids are safe to return because they are opaque provider
/// identities. A `kind="path"` locator remains Host-internal: its value is a
/// provider session file path and must never cross the Remote/native DTO seam.
/// Metadata-only identities also fail closed because they cannot support a
/// trustworthy semantic Conversation identity.
pub fn public_native_session_id(
    session: &AgentSessionInfo,
) -> Result<Option<String>, ConversationProjectionError> {
    match resolve_agent_session_source(session)? {
        SessionSourceLocator::NativeId { native_id, .. } if !native_id.is_empty() => {
            Ok(Some(native_id))
        }
        SessionSourceLocator::NativeId { .. } => Err(ConversationProjectionError::InvalidLocator),
        SessionSourceLocator::FilePath { .. } => Ok(None),
        SessionSourceLocator::MetadataOnly { .. } => {
            Err(ConversationProjectionError::InvalidLocator)
        }
    }
}

/// Herdr uses short integration ids (`claude`, `agy`) while shardlane-history
/// uses stable provider slugs. Keep this mapping in Host so Remote/native
/// callers never invent their own aliases. Only the non-slug Herdr aliases
/// need explicit arms (C21); every other integration id IS the history slug
/// and resolves through the canonical fallback.
pub fn agent_id_from_herdr_kind(value: &str) -> Option<AgentId> {
    match value {
        "claude" => Some(AgentId::ClaudeCode),
        "agy" => Some(AgentId::Antigravity),
        other => AgentId::from_slug(other),
    }
}

/// Provider label derivation for a live Agent (C14): the Herdr agent kind
/// mapped to the product provider name. `None` when Herdr's kind is not a
/// recognized provider — strict callers turn that into a typed error, lenient
/// callers fall back through [`agent_provider_label`].
pub fn agent_provider_kind(session: &AgentSessionInfo) -> Option<&'static str> {
    agent_id_from_herdr_kind(&session.agent).map(herdr_agent_kind)
}

/// Lenient provider label (C14): the recognized product provider name, else
/// the Agent's own kind string, else "unknown".
pub fn agent_provider_label(agent: &crate::herdr::Agent, session: &AgentSessionInfo) -> String {
    agent_provider_kind(session)
        .unwrap_or_else(|| agent.agent.as_deref().unwrap_or("unknown"))
        .to_string()
}

/// Shared lookup of a Herdr Agent by ref — pane id or terminal id (C15).
/// Callers adapt the `None`/error flavors to their own error types.
pub fn find_agent_by_ref(
    client: &crate::herdr::HerdrClient,
    agent_ref: &str,
) -> Result<Option<crate::herdr::Agent>, crate::herdr::HerdrError> {
    Ok(client.agents()?.into_iter().find(|agent| {
        agent.pane_id.as_deref() == Some(agent_ref) || agent.terminal_id == agent_ref
    }))
}

pub fn herdr_agent_kind(agent: AgentId) -> &'static str {
    match agent {
        AgentId::ClaudeCode => "claude",
        AgentId::Codex => "codex",
        AgentId::Copilot => "copilot",
        AgentId::Cursor => "cursor",
        AgentId::Opencode => "opencode",
        AgentId::CommandCode => "commandcode",
        AgentId::Kiro => "kiro",
        AgentId::Gemini => "gemini",
        AgentId::Pi => "pi",
        AgentId::Omp => "omp",
        AgentId::Grok => "grok",
        AgentId::Kimi => "kimi",
        AgentId::Antigravity => "agy",
        AgentId::Dsh => "dsh",
        AgentId::Qoder => "qoder",
    }
}

pub fn history_summary(meta: &SessionMeta, project_id: ProjectId) -> ConversationSummary {
    ConversationSummary {
        id: conversation_id_for_history_key(&meta.key),
        project_id,
        source: ConversationSource::History,
        agent_kind: meta.agent.as_str().to_string(),
        title: meta.title.clone(),
        status: None,
        sendable: false,
        live_agent_ref: None,
        updated_at_ms: u64::try_from(meta.updated_at.max(0)).ok(),
        revision: 0,
    }
}

pub fn live_summary(
    agent_ref: AgentRef,
    session: Option<&AgentSessionInfo>,
    project_id: ProjectId,
    provider: impl Into<String>,
    title: impl Into<String>,
    status: AgentStatus,
    revision: u64,
) -> ConversationSummary {
    // AC-03: prefer the session-exact v2 id; the legacy v1 id is only produced
    // for session-less projections and is rejected by semantic mutations.
    let id = match session {
        Some(session) => conversation_id_for_live_session(&agent_ref, session),
        None => conversation_id_for_live_agent(&agent_ref),
    };
    ConversationSummary {
        id,
        project_id,
        source: ConversationSource::Live,
        agent_kind: provider.into(),
        title: title.into(),
        status: Some(status),
        sendable: true,
        live_agent_ref: Some(agent_ref),
        updated_at_ms: None,
        revision,
    }
}

/// Host-owned read projection for indexed History. Remote/native transports may
/// choose their own async/threading strategy, but provider lookup, window bounds,
/// and semantic normalization stay here so they cannot drift per client.
pub struct HistoryConversationService<'a> {
    catalog: &'a HistoryCatalog,
}

impl<'a> HistoryConversationService<'a> {
    pub fn new(catalog: &'a HistoryCatalog) -> Self {
        Self { catalog }
    }

    pub fn session(&self, key: &str) -> Result<Option<SessionMeta>, String> {
        self.catalog.session(key).map_err(|error| error.to_string())
    }

    pub fn source(&self, key: &str) -> Result<Option<SessionFileRef>, String> {
        self.catalog
            .transcript_source(key)
            .map_err(|error| error.to_string())
    }

    pub fn list_for_project(
        &self,
        project_path: &str,
        project_id: ProjectId,
        limit: usize,
    ) -> Result<Vec<ConversationSummary>, String> {
        let sessions = self
            .catalog
            .sessions_for_project(project_path, limit)
            .map_err(|error| error.to_string())?
            .1;
        Ok(sessions
            .iter()
            .map(|meta| history_summary(meta, project_id.clone()))
            .collect())
    }

    pub fn search(
        &self,
        query: &str,
        project_paths: &[String],
        project_id: Option<ProjectId>,
        limit: usize,
    ) -> Result<Vec<ConversationSummary>, String> {
        let sessions = if project_paths.is_empty() {
            self.catalog
                .search_session_metadata(query, limit)
                .map_err(|error| error.to_string())?
        } else {
            self.catalog
                .search_session_metadata_scoped(query, project_paths, &[], limit)
                .map_err(|error| error.to_string())?
        };
        Ok(sessions
            .iter()
            .map(|meta| {
                history_summary(
                    meta,
                    project_id
                        .clone()
                        .unwrap_or_else(|| crate::project_id_for_path(&meta.project_path)),
                )
            })
            .collect())
    }

    pub fn window(
        &self,
        key: &str,
        project_id: ProjectId,
        anchor_seq: Option<u64>,
        before: u32,
        after: u32,
        max_window: u32,
    ) -> Result<(ConversationSummary, crate::dto::ConversationWindow), String> {
        let meta = self
            .session(key)?
            .ok_or_else(|| "conversation not found".to_string())?;
        let summary = history_summary(&meta, project_id);
        let source = self
            .source(key)?
            .ok_or_else(|| "history source unavailable".to_string())?;
        let before = usize::try_from(before.clamp(1, max_window)).unwrap_or(80);
        let after = usize::try_from(after.clamp(1, max_window)).unwrap_or(80);
        let limit = before.saturating_add(after).max(1);
        let (messages, total, start) = match anchor_seq {
            Some(seq) => {
                let anchor_index = self.anchor_index(key, &source, seq)?;
                let start = anchor_index.saturating_sub(before);
                let (messages, total) = self.transcript_window(key, &source, start, limit)?;
                (messages, total, start)
            }
            None => self.transcript_tail_window(key, &source, limit)?,
        };
        let items = normalize_transcript(&messages);
        let window = crate::dto::ConversationWindow {
            conversation_id: conversation_id_for_history_key(key),
            revision: u64::try_from(meta.updated_at.max(0)).unwrap_or(0),
            first_seq: items.first().map(|item| item.seq),
            last_seq: items.last().map(|item| item.seq),
            has_older: start > 0,
            has_newer: start.saturating_add(messages.len()) < total,
            items,
        };
        Ok((summary, window))
    }

    fn anchor_index(&self, key: &str, source: &SessionFileRef, seq: u64) -> Result<usize, String> {
        if let Some(index) = self
            .catalog
            .cached_transcript_index_for_seq(key, source, i64::try_from(seq).unwrap_or(i64::MAX))
            .map_err(|error| error.to_string())?
        {
            return Ok(index);
        }
        // A cache miss still resolves the source sequence exactly; it must not
        // assume that a provider's seq is a zero-based array index.
        let messages = self.parse_transcript(source)?;
        Ok(messages
            .iter()
            .position(|message| u64::try_from(message.seq).ok() == Some(seq))
            .or_else(|| {
                messages.iter().position(|message| {
                    u64::try_from(message.seq)
                        .ok()
                        .is_some_and(|value| value >= seq)
                })
            })
            .unwrap_or(messages.len()))
    }

    fn transcript_tail_window(
        &self,
        key: &str,
        source: &SessionFileRef,
        limit: usize,
    ) -> Result<(Vec<TranscriptMessage>, usize, usize), String> {
        // The page-cache metadata exposes the total without loading a page. On
        // a cold cache, parse once and slice the tail from that same result.
        if let Some(cached) = self
            .catalog
            .cached_transcript_window(key, source, 0, 0)
            .map_err(|error| error.to_string())?
        {
            let start = cached.total_messages.saturating_sub(limit);
            let (messages, total) = self.transcript_window(key, source, start, limit)?;
            return Ok((messages, total, start));
        }
        let parsed = self.parse_transcript(source)?;
        let total = parsed.len();
        let start = total.saturating_sub(limit);
        let end = start.saturating_add(limit).min(total);
        Ok((parsed[start..end].to_vec(), total, start))
    }

    fn transcript_window(
        &self,
        key: &str,
        source: &SessionFileRef,
        start: usize,
        limit: usize,
    ) -> Result<(Vec<TranscriptMessage>, usize), String> {
        if let Some(window) = self
            .catalog
            .cached_transcript_window(key, source, start, limit)
            .map_err(|error| error.to_string())?
        {
            return Ok((window.messages, window.total_messages));
        }
        let parsed = self.parse_transcript(source)?;
        let end = start.saturating_add(limit).min(parsed.len());
        Ok((parsed[start.min(parsed.len())..end].to_vec(), parsed.len()))
    }

    fn parse_transcript(&self, source: &SessionFileRef) -> Result<Vec<TranscriptMessage>, String> {
        let adapters = create_adapters();
        let adapter = adapter_for(&adapters, source.agent, &source.file_path).ok_or_else(|| {
            format!(
                "{} history source is unavailable",
                source.agent.display_name()
            )
        })?;
        adapter
            .parse_transcript(source)
            .map(|parsed| parsed.mainline)
            .map_err(|error| error.to_string())
    }
}

/// Resolve a live Herdr session to its exact history source. The locator kind
/// decides whether `(provider,native_id)` or an explicitly reported path is
/// used; a metadata-only provider fails closed instead of guessing. A native-id
/// session whose transcript is not indexed yet is `Ok(None)`: brand-new live
/// sessions have no transcript on disk until the first prompt, and clients must
/// still receive an empty conversation they can send into.
pub fn live_session_source(
    catalog: &HistoryCatalog,
    session: &AgentSessionInfo,
) -> Result<Option<SessionFileRef>, String> {
    match resolve_agent_session_source(session).map_err(|error| error.to_string())? {
        SessionSourceLocator::NativeId { agent, native_id } => catalog
            .session_source_by_native(agent, &native_id)
            .map_err(|error| error.to_string()),
        SessionSourceLocator::FilePath { agent, path } => {
            let metadata = std::fs::metadata(&path)
                .map_err(|error| format!("history source unavailable: {error}"))?;
            let modified = metadata
                .modified()
                .ok()
                .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                .and_then(|duration| i64::try_from(duration.as_millis()).ok())
                .unwrap_or(0);
            Ok(Some(SessionFileRef {
                agent,
                native_id: session.value.clone(),
                file_path: path,
                mtime_ms: modified,
                size: i64::try_from(metadata.len()).unwrap_or(i64::MAX),
            }))
        }
        SessionSourceLocator::MetadataOnly { .. } => {
            Err("provider exposes metadata only; semantic live Chat is unavailable".to_string())
        }
    }
}

/// Project one normalized History/Live message into the single Conversation item shape.
/// The source sequence is retained, making append/replay reconciliation deterministic.
pub fn normalize_transcript_message(message: &TranscriptMessage) -> ConversationItem {
    let kind = match message.kind {
        MessageKind::Meta => ConversationItemKind::Meta,
        MessageKind::CompactSummary => ConversationItemKind::CompactSummary,
        MessageKind::Text if !message.tool_calls.is_empty() => ConversationItemKind::Tool,
        MessageKind::Text => match message.role {
            Role::User => ConversationItemKind::User,
            Role::Assistant => {
                if message.text.is_empty() && message.thinking.is_some() {
                    ConversationItemKind::Reasoning
                } else {
                    ConversationItemKind::Assistant
                }
            }
            Role::System => ConversationItemKind::Activity,
        },
    };
    ConversationItem {
        id: format!("item-{}", message.seq),
        seq: u64::try_from(message.seq.max(0)).unwrap_or(0),
        kind,
        role: Some(message.role.as_str().to_string()),
        text: message.text.clone(),
        thinking: message.thinking.clone(),
        tool_calls: message
            .tool_calls
            .iter()
            .map(|tool| ConversationToolCall {
                id: tool.id.clone(),
                name: tool.name.clone(),
                input_preview: tool.input_preview.clone(),
                input: tool.input.clone(),
                output: tool.output.clone(),
                is_error: tool.is_error,
            })
            .collect(),
        timestamp_ms: message
            .timestamp
            .and_then(|timestamp| u64::try_from(timestamp.max(0)).ok()),
        model: message.model.clone(),
        truncated: message.truncated,
    }
}

pub fn normalize_transcript(messages: &[TranscriptMessage]) -> Vec<ConversationItem> {
    messages.iter().map(normalize_transcript_message).collect()
}

pub fn normalize_live_snapshot(snapshot: &LiveSnapshot) -> Vec<ConversationItem> {
    normalize_transcript(&snapshot.messages)
}

/// Construct the identity returned after a Host-owned Continue/Fork transaction.
pub fn continued_identity(
    agent_ref: AgentRef,
    provider: impl Into<String>,
    session: &AgentSessionInfo,
    revision: u64,
) -> Result<ConversationIdentity, ConversationProjectionError> {
    let conversation_id = conversation_id_for_live_session(&agent_ref, session);
    Ok(ConversationIdentity {
        conversation_id,
        agent_ref,
        provider: provider.into(),
        native_session_id: public_native_session_id(session)?,
        revision,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn transcript(seq: i64, role: Role, kind: MessageKind, text: &str) -> TranscriptMessage {
        TranscriptMessage {
            seq,
            role,
            kind,
            text: text.into(),
            truncated: false,
            tool_calls: Vec::new(),
            thinking: None,
            timestamp: Some(42),
            model: Some("model".into()),
        }
    }

    #[test]
    fn conversation_ids_are_opaque_and_round_trip() {
        let live = conversation_id_for_live_agent(&AgentRef::new("pane-7"));
        assert!(!live.as_str().contains("pane-7"));
        assert_eq!(
            resolve_conversation_id(&live),
            Ok(ConversationLocator::Live(AgentRef::new("pane-7")))
        );
        let history = conversation_id_for_history_key("claude-code:session-1");
        assert_eq!(
            resolve_conversation_id(&history),
            Ok(ConversationLocator::History("claude-code:session-1".into()))
        );
    }

    fn claude_session(value: &str) -> AgentSessionInfo {
        AgentSessionInfo {
            agent: "claude".into(),
            kind: "id".into(),
            source: "herdr:claude".into(),
            value: value.into(),
        }
    }

    #[test]
    fn live_v2_ids_bind_the_exact_session_occupant() {
        // AC-03: same pane, different typed session → different opaque id.
        let agent_ref = AgentRef::new("w1:p2");
        let first = conversation_id_for_live_session(&agent_ref, &claude_session("native-1"));
        let second = conversation_id_for_live_session(&agent_ref, &claude_session("native-2"));
        assert_ne!(first, second);
        assert_ne!(first, conversation_id_for_live_agent(&agent_ref));
        assert!(first.as_str().starts_with("conv_2_"));
        assert!(
            !first.as_str().contains("native-1"),
            "opaque: no session value"
        );
        assert_eq!(
            resolve_conversation_id(&first),
            Ok(ConversationLocator::LiveSession {
                agent_ref: agent_ref.clone(),
                fingerprint: session_fingerprint(&claude_session("native-1")),
            })
        );
        // Pane ids may contain colons; the fingerprint splits from the right.
        let colon_pane = AgentRef::new("ws-1:tab:pane-9");
        let colon_id = conversation_id_for_live_session(&colon_pane, &claude_session("native-1"));
        assert_eq!(
            resolve_conversation_id(&colon_id),
            Ok(ConversationLocator::LiveSession {
                agent_ref: colon_pane,
                fingerprint: session_fingerprint(&claude_session("native-1")),
            })
        );
    }

    #[test]
    fn occupant_validation_rejects_stale_and_legacy_live_ids() {
        // AC-03: a replacement occupant must fail closed, never retarget.
        let id =
            conversation_id_for_live_session(&AgentRef::new("pane-1"), &claude_session("native-1"));
        let locator = resolve_conversation_id(&id).unwrap_or_else(|e| panic!("{e}"));
        assert!(validate_live_occupant(&locator, &claude_session("native-1")).is_ok());
        assert_eq!(
            validate_live_occupant(&locator, &claude_session("replaced")),
            Err(ConversationProjectionError::StaleOccupant)
        );
        // v1 live ids cannot prove an occupant: rejected for mutations.
        let legacy =
            resolve_conversation_id(&conversation_id_for_live_agent(&AgentRef::new("pane-1")))
                .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            validate_live_occupant(&legacy, &claude_session("native-1")),
            Err(ConversationProjectionError::StaleOccupant)
        );
        // Fingerprints never disclose the provider path/value.
        let path_session = AgentSessionInfo {
            agent: "pi".into(),
            kind: "path".into(),
            source: "herdr:pi".into(),
            value: "/Users/example/.pi/sessions/session.jsonl".into(),
        };
        let path_id = conversation_id_for_live_session(&AgentRef::new("pane-2"), &path_session);
        assert!(!path_id.as_str().contains("/Users/example"));
    }

    #[test]
    fn fingerprints_discriminate_locator_kinds_and_values() {
        let id_session = claude_session("shared-value");
        let path_session = AgentSessionInfo {
            agent: "claude".into(),
            kind: "path".into(),
            source: "herdr:claude".into(),
            value: "shared-value".into(),
        };
        assert_ne!(
            session_fingerprint(&id_session),
            session_fingerprint(&path_session)
        );
        assert_eq!(
            session_fingerprint(&id_session),
            session_fingerprint(&claude_session("shared-value"))
        );
    }

    #[test]
    fn locator_uses_declared_kind_not_value_shape() {
        let session = AgentSessionInfo {
            agent: "codex".into(),
            kind: "id".into(),
            source: "herdr:codex".into(),
            value: "/looks/like/a/path".into(),
        };
        assert_eq!(
            resolve_agent_session_source(&session),
            Ok(SessionSourceLocator::NativeId {
                agent: AgentId::Codex,
                native_id: "/looks/like/a/path".into(),
            })
        );
    }

    #[test]
    fn herdr_short_provider_ids_resolve_to_history_agents() {
        let session = AgentSessionInfo {
            agent: "claude".into(),
            kind: "id".into(),
            source: "herdr:claude".into(),
            value: "native-1".into(),
        };
        assert!(matches!(
            resolve_agent_session_source(&session),
            Ok(SessionSourceLocator::NativeId {
                agent: AgentId::ClaudeCode,
                ..
            })
        ));
        assert_eq!(herdr_agent_kind(AgentId::Antigravity), "agy");
    }

    #[test]
    fn herdr_kind_mapping_covers_every_alias_and_defers_to_the_slug_fallback() {
        // C21: the explicit arms are only the non-slug Herdr aliases; every
        // other integration id resolves identically through AgentId::from_slug.
        for kind in [
            "claude",
            "codex",
            "copilot",
            "cursor",
            "opencode",
            "commandcode",
            "command-code",
            "kiro",
            "gemini",
            "pi",
            "omp",
            "grok",
            "kimi",
            "agy",
            "antigravity",
            "dsh",
            "qoder",
        ] {
            assert!(
                agent_id_from_herdr_kind(kind).is_some(),
                "{kind} must resolve"
            );
        }
        assert_eq!(
            agent_id_from_herdr_kind("claude"),
            Some(AgentId::ClaudeCode)
        );
        assert_eq!(agent_id_from_herdr_kind("agy"), Some(AgentId::Antigravity));
        assert_eq!(
            agent_id_from_herdr_kind("antigravity"),
            Some(AgentId::Antigravity)
        );
        assert_eq!(agent_id_from_herdr_kind("not-a-provider"), None);
    }

    #[test]
    fn normalized_items_preserve_order_and_semantic_kind() {
        let messages = vec![
            transcript(1, Role::User, MessageKind::Text, "hello"),
            transcript(2, Role::Assistant, MessageKind::Text, "world"),
            transcript(3, Role::System, MessageKind::Meta, "status"),
        ];
        let items = normalize_transcript(&messages);
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].kind, ConversationItemKind::User);
        assert_eq!(items[1].kind, ConversationItemKind::Assistant);
        assert_eq!(items[2].kind, ConversationItemKind::Meta);
        assert_eq!(items[2].id, "item-3");
    }

    #[test]
    fn live_and_history_normalization_is_identical_for_tools_and_reasoning() {
        let messages = vec![
            transcript(1, Role::User, MessageKind::Text, "prompt"),
            TranscriptMessage {
                seq: 2,
                role: Role::Assistant,
                kind: MessageKind::Text,
                text: String::new(),
                truncated: false,
                tool_calls: Vec::new(),
                thinking: Some("inspect the repository".into()),
                timestamp: Some(43),
                model: Some("model".into()),
            },
            TranscriptMessage {
                seq: 3,
                role: Role::Assistant,
                kind: MessageKind::Text,
                text: "cargo test".into(),
                truncated: false,
                tool_calls: vec![shardlane_history::models::ToolCallView {
                    id: "tool-1".into(),
                    name: "shell".into(),
                    input_preview: "cargo test".into(),
                    input: None,
                    output: Some("ok".into()),
                    is_error: false,
                    sidechain_ref: None,
                }],
                thinking: None,
                timestamp: Some(44),
                model: Some("model".into()),
            },
        ];
        let history_items = normalize_transcript(&messages);
        let live_items = normalize_live_snapshot(&shardlane_history::LiveSnapshot {
            messages,
            facts: shardlane_history::LiveFacts::default(),
            generation: 1,
        });
        assert_eq!(live_items, history_items);
        assert_eq!(history_items[1].kind, ConversationItemKind::Reasoning);
        assert_eq!(history_items[2].kind, ConversationItemKind::Tool);
        assert_eq!(history_items[2].tool_calls[0].output.as_deref(), Some("ok"));
    }

    #[test]
    fn unknown_locator_kind_fails_closed_instead_of_guessing_path() {
        let session = AgentSessionInfo {
            agent: "claude".into(),
            kind: "unknown".into(),
            source: "herdr:claude".into(),
            value: "/tmp/provider-session.jsonl".into(),
        };
        assert!(matches!(
            resolve_agent_session_source(&session),
            Ok(SessionSourceLocator::MetadataOnly { .. })
        ));
    }

    #[test]
    fn public_identity_redacts_provider_file_paths() {
        let session = AgentSessionInfo {
            agent: "pi".into(),
            kind: "path".into(),
            source: "herdr:pi".into(),
            value: "/Users/example/.pi/sessions/session.jsonl".into(),
        };
        assert_eq!(public_native_session_id(&session), Ok(None));
        let identity = continued_identity(AgentRef::new("pane-1"), "pi", &session, 1)
            .unwrap_or_else(|error| panic!("path-backed identity failed: {error}"));
        assert_eq!(identity.native_session_id, None);
        let wire = serde_json::to_string(&identity)
            .unwrap_or_else(|error| panic!("identity serialization failed: {error}"));
        assert!(!wire.contains("/Users/example/.pi/sessions/session.jsonl"));
    }

    #[test]
    fn public_identity_rejects_metadata_only_sessions() {
        let session = AgentSessionInfo {
            agent: "agy".into(),
            kind: "id".into(),
            source: "herdr:agy".into(),
            value: "opaque-metadata".into(),
        };
        assert_eq!(
            public_native_session_id(&session),
            Err(ConversationProjectionError::InvalidLocator)
        );
    }
}
