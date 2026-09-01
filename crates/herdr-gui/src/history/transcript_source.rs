//! [INPUT]: Existing imports and types from the crate root (via the history module root glob: `use super::*` chain).
//! [OUTPUT]: For the crate::history family: the transcript data source — db path / full loading / ConversationRef construction.
//! [POS]: Transcript-source responsibility slice of the herdr-gui History surface; mechanically split out of history.rs.
use super::*;

pub(crate) fn history_db_path() -> PathBuf {
    settings::app_data_dir().join("history.sqlite3")
}

pub(super) fn make_history_reference(session: &ConversationMeta) -> ConversationRef {
    ConversationRef {
        agent: session.agent,
        native_id: session.id.clone(),
        file_path: session.file_path.clone(),
        mtime_ms: session.updated_at,
        size: session.size_bytes,
    }
}

/// Read the full conversation transcript: prefer the page-addressable catalog
/// cache; on miss, fall back to on-the-spot adapter parsing and backfill the
/// cache.
pub(super) fn load_full_transcript(
    db_path: &std::path::Path,
    session: &ConversationMeta,
    roster: &HistoryAdapterRoster,
) -> Result<ParsedTranscript, String> {
    if let Ok(catalog) =
        HistoryCatalog::open_initialized(db_path).or_else(|_| HistoryCatalog::open(db_path))
    {
        let resolved = catalog
            .transcript_source(&session.key)
            .ok()
            .flatten()
            .unwrap_or_else(|| make_history_reference(session));
        if let Some(window) = catalog
            .cached_transcript_window(&session.key, &resolved, 0, usize::MAX)
            .map_err(|error| error.to_string())?
        {
            return Ok(ParsedTranscript {
                meta: window.meta,
                mainline: window.messages,
                sidechains: window.sidechains,
                unknown_line_count: window.unknown_line_count,
            });
        }
        let transcript = parse_history_transcript(&catalog, &session.key, &resolved, roster)?;
        return Ok(transcript);
    }
    let reference = make_history_reference(session);
    let adapter = roster
        .adapter_for_source(session.agent, &reference.file_path)
        .ok_or_else(|| {
            format!(
                "{} history source is unavailable",
                session.agent.display_name()
            )
        })?;
    adapter
        .parse_transcript(&reference)
        .map_err(|error| error.to_string())
}
