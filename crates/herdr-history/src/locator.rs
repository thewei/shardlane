// SPDX-License-Identifier: MIT
// Portions Copyright (c) 2026 Corey Chiu; retained under the upstream MIT terms.
//! [INPUT]: Provider capability knowledge from models::AgentId and
//! live::LiveDecoder; zero I/O, zero GUI — pure functional resolution.
//! [OUTPUT]: SessionSourceLocator (NativeId / FilePath / MetadataOnly) and
//! resolve_session_source_locator.
//! [POS]: herdr-history's Herdr boundary contract layer (audit CHAT-A10):
//! resolves Herdr `AgentSessionInfo { agent, kind, source, value }` into a
//! typed semantic source locator so the GUI no longer guesses paths from the
//! value shape (contains `/`, whether it is a file). kind values come from the
//! measured live protocol: Pi/Omp use `kind="path"` (value = session file
//! path), Claude/Codex and others use `kind="id"` (value = provider-native
//! session id); `source` is the herdr self-identification tag
//! (`herdr:<provider>`). Phase-1 resolution only discriminates on kind; every
//! other kind maps to MetadataOnly (never guessed). New non-file locators
//! require explicit product approval; ACP is currently frozen and not an
//! extension direction.

use crate::live::registry::{LiveCapability, PROVIDERS};
use crate::models::AgentId;

/// Typed semantic source locator for one Agent session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SessionSourceLocator {
    /// `value` is a provider-native session id: consumers must exchange it for
    /// the session file via an exact `(agent, native_id)` HistoryCatalog lookup.
    NativeId { agent: AgentId, native_id: String },
    /// `value` is the session file path itself (kind="path"): use directly as
    /// the exact source.
    FilePath { agent: AgentId, path: String },
    /// Session identity exists but no semantic live source is currently
    /// available: a metadata-only provider with an encrypted body (e.g.
    /// Antigravity), or an unknown kind reported by Herdr. Consumers must
    /// explicitly show unavailability and must not guess.
    MetadataOnly { agent: AgentId, native_id: String },
}

impl SessionSourceLocator {
    /// Session identity in value form (carried by two of the locators).
    pub fn native_identity(&self) -> &str {
        match self {
            Self::NativeId { native_id, .. } => native_id,
            Self::FilePath { path, .. } => path,
            Self::MetadataOnly { native_id, .. } => native_id,
        }
    }
}

/// Resolve Herdr `AgentSessionInfo { agent, kind, source, value }` into a
/// typed locator. `source` (herdr self-tag `herdr:<provider>` / client-reported
/// `shardlane`) currently takes no part in discrimination and stays in the
/// signature to lock the contract boundary; if new non-file source locators
/// appear with explicit product approval in the future, they must extend this
/// typed API too — never return to guessing from the value shape. ACP is
/// currently frozen and is not that extension direction.
pub fn resolve_session_source_locator(
    agent: AgentId,
    kind: &str,
    source: &str,
    value: &str,
) -> SessionSourceLocator {
    let _ = source;
    // kind vocabulary measured from the Herdr live protocol ("id"/"path");
    // db-session/protocol etc. from plan §9 wait for real Herdr reports
    // before being added — no assumptions.
    let live_capable = PROVIDERS
        .iter()
        .any(|entry| entry.agent == agent && entry.live == LiveCapability::AppendLog);
    match kind {
        "path" => SessionSourceLocator::FilePath {
            agent,
            path: value.to_string(),
        },
        "id" if live_capable => SessionSourceLocator::NativeId {
            agent,
            native_id: value.to_string(),
        },
        _ => SessionSourceLocator::MetadataOnly {
            agent,
            native_id: value.to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Live-protocol measured shapes (2026-08-28, herdr agent list):
    /// Pi = kind "path" + file path; Claude/Codex = kind "id" + native id;
    /// Antigravity = kind "id" but no live semantic decoder.
    #[test]
    fn resolves_live_protocol_shapes() {
        // Pi: kind=path → FilePath, path kept verbatim.
        assert_eq!(
            resolve_session_source_locator(
                AgentId::Pi,
                "path",
                "herdr:pi",
                "/Users/x/.pi/sessions/2026-08-28T01-12-28.jsonl",
            ),
            SessionSourceLocator::FilePath {
                agent: AgentId::Pi,
                path: "/Users/x/.pi/sessions/2026-08-28T01-12-28.jsonl".into(),
            }
        );
        // Claude: kind=id + supported → NativeId.
        assert_eq!(
            resolve_session_source_locator(
                AgentId::ClaudeCode,
                "id",
                "herdr:claude",
                "2153fb6a-cb88-46cb-a8b6-1e1e509cf5fb",
            ),
            SessionSourceLocator::NativeId {
                agent: AgentId::ClaudeCode,
                native_id: "2153fb6a-cb88-46cb-a8b6-1e1e509cf5fb".into(),
            }
        );
        // Same for Codex.
        assert!(matches!(
            resolve_session_source_locator(AgentId::Codex, "id", "herdr:codex", "01a045da"),
            SessionSourceLocator::NativeId { .. }
        ));
        // Kimi (Wave 1): kind=id + AppendLog capability → NativeId.
        assert!(matches!(
            resolve_session_source_locator(AgentId::Kimi, "id", "herdr:kimi", "session_abc"),
            SessionSourceLocator::NativeId { .. }
        ));
        assert!(matches!(
            resolve_session_source_locator(AgentId::Cursor, "id", "herdr:cursor", "00d021ce"),
            SessionSourceLocator::NativeId { .. }
        ));
        // Antigravity: kind=id but encrypted body (no decoder) → MetadataOnly,
        // never faked.
        assert!(matches!(
            resolve_session_source_locator(
                AgentId::Antigravity,
                "id",
                "herdr:antigravity_cli",
                "b316307a",
            ),
            SessionSourceLocator::MetadataOnly { .. }
        ));
        // Unknown kind: never guessed even when the agent is supported.
        assert!(matches!(
            resolve_session_source_locator(AgentId::ClaudeCode, "sqlite", "herdr:claude", "x"),
            SessionSourceLocator::MetadataOnly { .. }
        ));
    }

    #[test]
    fn native_identity_covers_all_variants() {
        let locator =
            resolve_session_source_locator(AgentId::Omp, "path", "herdr:omp", "/a/b.jsonl");
        assert_eq!(locator.native_identity(), "/a/b.jsonl");
        let locator = resolve_session_source_locator(AgentId::Codex, "id", "herdr:codex", "c-1");
        assert_eq!(locator.native_identity(), "c-1");
    }
}
