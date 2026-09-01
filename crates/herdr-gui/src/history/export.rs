//! [INPUT]: Existing imports and types from the crate root (via the history module root glob: `use super::*` chain).
//! [OUTPUT]: For the crate::history family: export — Markdown generation (full transcript / cached window) and writing to disk.
//! [POS]: Export responsibility slice of the herdr-gui History surface; mechanically split out of history.rs.
use super::*;

pub(crate) fn history_export_cached_window_markdown(window: &CachedTranscriptWindow) -> String {
    let meta = &window.meta;
    let mut out = String::new();
    out.push_str(&format!("# {}\n\n", meta.title));
    let branch = meta
        .git_branch
        .as_deref()
        .map(|branch| format!(" ({branch})"))
        .unwrap_or_default();
    let tokens = meta
        .tokens_used
        .map(history_fmt_tokens)
        .unwrap_or_else(|| "-".to_string());
    out.push_str(&format!(
        "> **Agent**: {} · **Project**: {}{} · **Created**: {} · **Updated**: {} · **Messages**: {} · **Tokens**: {}\n\n---\n\n",
        meta.agent.display_name(),
        if meta.project_path.is_empty() {
            "(unknown)"
        } else {
            &meta.project_path
        },
        branch,
        history_abs_date(meta.created_at),
        history_abs_date(meta.updated_at),
        window.total_messages,
        tokens,
    ));
    for message in &window.messages {
        let role_label = match message.role {
            shardlane_history::Role::User => "User",
            shardlane_history::Role::Assistant => "Assistant",
            shardlane_history::Role::System => "System",
        };
        out.push_str(&format!("### {role_label}\n\n"));
        if let Some(thinking) = &message.thinking {
            out.push_str("<details><summary>Thinking</summary>\n\n");
            out.push_str(thinking);
            out.push_str("\n\n</details>\n\n");
        }
        if !message.text.is_empty() {
            out.push_str(&message.text);
            out.push_str("\n\n");
        }
        for tool in &message.tool_calls {
            out.push_str(&format!(
                "<details><summary>Tool: {}</summary>\n\n```\n{}\n```\n\n</details>\n\n",
                tool.name, tool.input_preview
            ));
        }
    }
    out
}

/// On-the-spot catalog-path parsing plus cache backfill (same as the tail of
/// load_transcript_window).
pub(super) fn parse_history_transcript(
    catalog: &HistoryCatalog,
    key: &str,
    reference: &ConversationRef,
    roster: &HistoryAdapterRoster,
) -> Result<ParsedTranscript, String> {
    let adapter = roster
        .adapter_for_source(reference.agent, &reference.file_path)
        .ok_or_else(|| {
            format!(
                "{} history source is unavailable",
                reference.agent.display_name()
            )
        })?;
    let transcript = adapter
        .parse_transcript(reference)
        .map_err(|error| error.to_string())?;
    let _ = catalog.cache_transcript(key, reference, &transcript);
    let _ = catalog.prune_transcript_page_cache();
    Ok(transcript)
}

/// Conversation → Markdown (header quote block + ### role lines +
/// thinking/tool `<details>` folds).
pub(super) fn history_export_markdown(transcript: &ParsedTranscript) -> String {
    let meta = &transcript.meta;
    let mut out = String::new();
    out.push_str(&format!("# {}\n\n", meta.title));
    let branch = meta
        .git_branch
        .as_deref()
        .map(|branch| format!(" ({branch})"))
        .unwrap_or_default();
    let tokens = meta
        .tokens_used
        .map(history_fmt_tokens)
        .unwrap_or_else(|| "-".to_string());
    out.push_str(&format!(
        "> **Agent**: {} · **Project**: {}{} · **Created**: {} · **Updated**: {} · **Messages**: {} · **Tokens**: {}\n\n---\n\n",
        meta.agent.display_name(),
        if meta.project_path.is_empty() {
            "(unknown)"
        } else {
            &meta.project_path
        },
        branch,
        history_abs_date(meta.created_at),
        history_abs_date(meta.updated_at),
        meta.message_count,
        tokens,
    ));
    for message in &transcript.mainline {
        if message.kind == shardlane_history::MessageKind::CompactSummary {
            out.push_str(&format!(
                "> **Context compacted**\n\n> {}\n\n",
                message.text
            ));
            continue;
        }
        let role = match message.role {
            shardlane_history::Role::User => "User",
            shardlane_history::Role::Assistant => "Assistant",
            shardlane_history::Role::System => "System",
        };
        let timestamp = message
            .timestamp
            .filter(|ts| *ts > 0)
            .map(|ts| format!(" · {}", history_abs_date(ts)))
            .unwrap_or_default();
        out.push_str(&format!("### {role}{timestamp}\n\n"));
        if let Some(thinking) = &message.thinking {
            out.push_str(&format!(
                "<details><summary>🧠 Thinking</summary>\n\n{thinking}\n\n</details>\n\n"
            ));
        }
        if !message.text.is_empty() {
            out.push_str(&message.text);
            out.push_str("\n\n");
        }
        for tool in &message.tool_calls {
            let failed = if tool.is_error { " (failed)" } else { "" };
            out.push_str(&format!(
                "<details><summary>🔧 {} — {}{}</summary>\n\n",
                tool.name,
                tool.input_preview.replace('<', "&lt;"),
                failed
            ));
            if let Some(input) = &tool.input {
                out.push_str(&format!("Input:\n\n```\n{input}\n```\n\n"));
            }
            if let Some(output) = &tool.output {
                out.push_str(&format!("Output:\n\n```\n{output}\n```\n\n"));
            }
            out.push_str("</details>\n\n");
        }
    }
    out
}

/// Writes ~/Downloads/Shardlane-<title>.md (falls back to ~ if the directory
/// is missing; the file name is sanitized).
pub(super) fn history_export_write(
    session: &ConversationMeta,
    markdown: &str,
) -> Result<std::path::PathBuf, String> {
    let home = std::env::var("HOME").map_err(|_| "HOME is unavailable".to_string())?;
    let home = std::path::Path::new(&home).to_path_buf();
    let dir = {
        let downloads = home.join("Downloads");
        if downloads.is_dir() {
            downloads
        } else {
            home
        }
    };
    let stem: String = session
        .title
        .chars()
        .map(|ch| {
            if ch.is_alphanumeric() || "-_.".contains(ch) {
                ch
            } else {
                '-'
            }
        })
        .take(60)
        .collect();
    let stem = stem.trim_matches('-').trim_end_matches(".md");
    let name = if stem.is_empty() {
        format!("Shardlane-{}.md", session.id)
    } else {
        format!("Shardlane-{stem}.md")
    };
    let path = dir.join(name);
    std::fs::write(&path, markdown).map_err(|error| error.to_string())?;
    Ok(path)
}

impl ShardlaneApp {
    /// Export an entire conversation as Markdown (full background read →
    /// ~/Downloads → system notification).
    pub(super) fn export_history_markdown(
        &mut self,
        session: ConversationMeta,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let db_path = history_db_path();
        let roster = self.history.roster.clone();
        let window_handle = window.window_handle();
        cx.spawn(async move |this, cx| {
            let export_session = session.clone();
            let result = cx
                .background_executor()
                .spawn(async move {
                    let transcript =
                        load_full_transcript(&db_path, &export_session, roster.as_ref())?;
                    let markdown = history_export_markdown(&transcript);
                    history_export_write(&export_session, &markdown)
                })
                .await;
            let _ = cx.update_window(window_handle, |_, window, cx| {
                let message = match result {
                    Ok(path) => format!("Exported to {}", path.display()),
                    Err(error) => format!("Export failed: {error}"),
                };
                window.push_notification(message, cx);
                let _ = this.update(cx, |_, cx| cx.notify());
            });
        })
        .detach();
    }
}
