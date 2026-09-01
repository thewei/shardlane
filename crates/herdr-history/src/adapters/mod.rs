// SPDX-License-Identifier: MIT
// Portions Copyright (c) 2026 Corey Chiu; retained under the upstream MIT terms.
//! [INPUT]: External Agent session roots and provider data formats such as
//! JSONL/SQLite.
//! [OUTPUT]: The AgentHistoryAdapter contract, default/custom adapter
//!           construction, source-ownership routing, and generic transcript
//!           unit extraction.
//! [POS]: Provider read boundary of shardlane-history; reads external data
//!        only, owns neither catalog nor runtime.

pub mod antigravity;
pub mod claude;
pub mod codex;
pub mod command_code;
pub mod copilot;
pub mod cursor;
pub mod dsh;
pub mod gemini;
pub mod grok;
pub mod kimi;
pub mod kiro;
pub mod opencode;
pub mod pi;
pub mod qoder;

pub(crate) mod parse_utils;
mod sqlite_ro;

use crate::models::*;
use anyhow::Result;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Read-only boundary for one external Agent history format.
pub trait AgentHistoryAdapter: Send + Sync {
    fn agent(&self) -> AgentId;
    /// Whether any data root exists. The root list is the ownership truth.
    fn detect(&self) -> bool {
        self.data_roots().iter().any(|path| path.exists())
    }
    fn list_session_files(&self) -> Result<Vec<SessionFileRef>>;

    fn file_ref(&self, path: &Path) -> Option<SessionFileRef> {
        parse_utils::default_file_ref(self.agent(), path)
    }

    fn quick_meta(&self, _refs: &[SessionFileRef]) -> Option<HashMap<String, SessionMeta>> {
        None
    }

    fn merge_quick_meta(&self, mut parsed: SessionMeta, quick: &SessionMeta) -> SessionMeta {
        if parsed.source.is_none() {
            parsed.source = quick.source.clone();
        }
        if parsed.model.is_none() {
            parsed.model = quick.model.clone();
        }
        if parsed.tokens_used.is_none() {
            parsed.tokens_used = quick.tokens_used;
        }
        parsed
    }

    fn parse_session(&self, reference: &SessionFileRef) -> Result<ParsedSession>;
    fn parse_transcript(&self, reference: &SessionFileRef) -> Result<ParsedTranscript>;

    fn load_sidechain(
        &self,
        _reference: &SessionFileRef,
        _sidechain_id: &str,
    ) -> Result<Vec<TranscriptMessage>> {
        Ok(Vec::new())
    }

    /// By default directory roots are watched; SQLite roots are intentionally
    /// omitted because their changes are picked up by a scan.
    fn watch_paths(&self) -> Vec<PathBuf> {
        self.data_roots()
            .into_iter()
            .filter(|path| path.is_dir())
            .collect()
    }

    /// Construct another instance rooted at a user-selected source. The
    /// caller owns the resulting instance and keeps it in one roster.
    fn with_custom_root(&self, root: PathBuf) -> Box<dyn AgentHistoryAdapter>;

    /// Whether one of several roots can be disabled without dropping the
    /// remaining roots from this adapter instance.
    fn supports_individual_root_removal(&self) -> bool {
        false
    }

    /// Return an instance that excludes the selected roots, when supported.
    fn excluding_data_roots(&self, _roots: &[PathBuf]) -> Option<Box<dyn AgentHistoryAdapter>> {
        None
    }

    /// The real filesystem/virtual roots owned by this adapter instance.
    fn data_roots(&self) -> Vec<PathBuf>;
}

pub fn create_adapters() -> Vec<Box<dyn AgentHistoryAdapter>> {
    let all: Vec<Box<dyn AgentHistoryAdapter>> = vec![
        Box::new(claude::ClaudeAdapter::new()),
        Box::new(codex::CodexAdapter::new()),
        Box::new(copilot::CopilotAdapter::new()),
        Box::new(cursor::CursorAdapter::new()),
        Box::new(opencode::OpencodeAdapter::new()),
        Box::new(command_code::CommandCodeAdapter::new()),
        Box::new(kiro::KiroAdapter::new()),
        Box::new(gemini::GeminiAdapter::new()),
        Box::new(pi::PiAdapter::new()),
        Box::new(pi::PiAdapter::omp()),
        Box::new(grok::GrokAdapter::new()),
        Box::new(kimi::KimiAdapter::new()),
        Box::new(antigravity::AntigravityAdapter::new()),
        Box::new(dsh::DshAdapter::new()),
        Box::new(qoder::QoderAdapter::new()),
    ];
    all
}

/// Construct the default + custom adapter set while preserving source order.
/// removed_defaults is keyed by AgentId for compatibility with callers that
/// disable an entire default adapter.
pub fn create_adapters_with(
    custom_roots: &[(AgentId, PathBuf)],
    removed_defaults: &[AgentId],
) -> Vec<Box<dyn AgentHistoryAdapter>> {
    let base = create_adapters();
    let customs = custom_roots
        .iter()
        .filter_map(|(agent, root)| {
            base.iter()
                .find(|adapter| adapter.agent() == *agent)
                .map(|adapter| {
                    adapter.with_custom_root(normalize_custom_root(*agent, root.clone()))
                })
        })
        .collect::<Vec<_>>();
    let mut active = base
        .into_iter()
        .filter(|adapter| !removed_defaults.contains(&adapter.agent()))
        .collect::<Vec<_>>();
    active.extend(customs);
    active
}

/// Provider-specific custom-root normalization. Most adapters accept either
/// their data root or the enclosing provider directory unchanged.
pub fn normalize_custom_root(agent: AgentId, root: PathBuf) -> PathBuf {
    match agent {
        AgentId::Codex => codex::normalize_custom_root(root),
        _ => root,
    }
}

/// Path ownership predicate shared by roster routing and catalog cleanup.
/// It respects separator boundaries and SQLite virtual db#id paths.
pub fn path_owns(root: &str, path: &str) -> bool {
    if root.is_empty() {
        return false;
    }
    if root.chars().last().is_some_and(is_path_separator) {
        return path.starts_with(root);
    }
    match path.strip_prefix(root) {
        Some("") => true,
        Some(rest) => rest.chars().next().is_some_and(is_path_separator) || rest.starts_with('#'),
        None => false,
    }
}

fn is_path_separator(character: char) -> bool {
    std::path::is_separator(character) || character == '\\'
}

/// Longest-root selection for same-provider custom locations.
pub fn adapter_ix_for(
    adapters: &[Box<dyn AgentHistoryAdapter>],
    agent: AgentId,
    file_path: &str,
) -> Option<usize> {
    let first = adapters
        .iter()
        .position(|adapter| adapter.agent() == agent)?;
    if !adapters[first + 1..]
        .iter()
        .any(|adapter| adapter.agent() == agent)
    {
        return Some(first);
    }
    let mut best: Option<(usize, usize)> = None;
    for (index, adapter) in adapters.iter().enumerate().skip(first) {
        if adapter.agent() != agent {
            continue;
        }
        for root in adapter.data_roots() {
            let root = root.to_string_lossy();
            if path_owns(&root, file_path) && best.is_none_or(|(length, _)| root.len() > length) {
                best = Some((root.len(), index));
            }
        }
    }
    Some(best.map_or(first, |(_, index)| index))
}

pub fn adapter_for<'a>(
    adapters: &'a [Box<dyn AgentHistoryAdapter>],
    agent: AgentId,
    file_path: &str,
) -> Option<&'a dyn AgentHistoryAdapter> {
    adapter_ix_for(adapters, agent, file_path).map(|index| adapters[index].as_ref())
}

#[cfg(test)]
mod tests;

pub(crate) fn units_from_messages(messages: &[TranscriptMessage]) -> Vec<IndexUnit> {
    messages
        .iter()
        .filter(|message| message.kind == MessageKind::Text)
        .filter_map(|message| {
            let mut parts = vec![message.text.clone()];
            for tool_call in &message.tool_calls {
                parts.push(format!("{} {}", tool_call.name, tool_call.input_preview));
            }
            let text = parse_utils::clip(&parts.join("\n"), MAX_MSG_TEXT).0;
            if text.trim().is_empty() {
                None
            } else {
                Some(IndexUnit {
                    seq: message.seq,
                    sidechain_id: None,
                    role: message.role,
                    timestamp: message.timestamp,
                    text,
                })
            }
        })
        .collect()
}
