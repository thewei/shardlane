//! [INPUT]: Existing imports and types from the crate root (via the history module root glob: `use super::*` chain).
//! [OUTPUT]: For the crate::history family: plain-text display formatting — one-line summaries, list descriptions, badge chips, token counts, absolute-date and last-active short formats, message times and compact pill labels (preview truncation was removed along with the "answers are always Markdown" convergence).
//! [POS]: Formatting responsibility slice of the herdr-gui History surface; mechanically split out of history.rs.
use super::*;

/// Model badge color (Claude orange, works for the outline form in both modes).
pub(super) const HISTORY_MODEL_BADGE: u32 = 0xD97757;

/// One-line summary: truncate at the first non-empty line (shared by the
/// thinking collapsed line and the System pill).
pub(super) fn history_one_line(text: &str, max: usize) -> String {
    let line = text
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("")
        .trim();
    let mut out: String = line.chars().take(max).collect();
    if line.chars().count() > max {
        out.push('…');
    }
    out
}

pub(super) fn history_session_list_description(
    session: &ConversationMeta,
    descriptions: &HashMap<String, String>,
) -> String {
    descriptions
        .get(&session.key)
        .filter(|text| !text.is_empty())
        .cloned()
        .unwrap_or_else(|| history_one_line(&session.title, 120))
}

/// Solid small badge (header project name).
pub(super) fn history_badge(text: String, bg: gpui::Hsla, fg: gpui::Hsla) -> gpui::Div {
    crate::ui::badge::badge(text, bg, fg)
}

/// Outlined small badge (model / source).
pub(super) fn history_outline_badge(text: String, color: gpui::Hsla) -> gpui::Div {
    crate::ui::badge::outline_badge(text, color)
}

pub(super) fn history_fmt_tokens(tokens: i64) -> String {
    if tokens >= 1_000_000 {
        format!("{:.1}M", tokens as f64 / 1_000_000.0)
    } else if tokens >= 10_000 {
        format!("{:.0}k", tokens as f64 / 1_000.0)
    } else {
        tokens.to_string()
    }
}

/// Normalize epoch seconds/milliseconds to seconds (catalog timestamps were
/// historically sometimes milliseconds; same check as the chat-side
/// history_msg_time_label). 2026-08-29 walkthrough: the detail header once
/// rendered milliseconds as seconds, producing "Created 58628-…".
pub(super) fn history_epoch_secs(epoch_secs: i64) -> i64 {
    if epoch_secs > 1_000_000_000_000 {
        epoch_secs / 1000
    } else {
        epoch_secs
    }
}

/// Last-active short format (notate 2026-08-29): HH:MM today, `Aug 28` this
/// year, `YYYY-MM-DD` in earlier years. Consumed by the meta of the Sidebar
/// Agents history backfill rows.
pub(crate) fn history_last_active_short(epoch_secs: i64) -> String {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let secs = history_epoch_secs(epoch_secs);
    if secs <= 0 {
        return "—".to_string();
    }
    let (year, month, day, hour, minute) = history_civil_datetime(secs);
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let (now_year, now_month, now_day, _, _) = history_civil_datetime(now_secs);
    if (year, month, day) == (now_year, now_month, now_day) {
        format!("{hour:02}:{minute:02}")
    } else if year == now_year {
        format!("{} {day}", MONTHS[(month - 1) as usize])
    } else {
        format!("{year:04}-{month:02}-{day:02}")
    }
}

/// Epoch seconds → (year, month, day, hour, minute) (UTC; pure civil_from_days algorithm).
pub(super) fn history_civil_datetime(epoch_secs: i64) -> (i64, u32, u32, u32, u32) {
    let days = epoch_secs.div_euclid(86_400);
    let secs_of_day = epoch_secs.rem_euclid(86_400);
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = if m <= 2 { y + 1 } else { y };
    (
        year,
        m,
        d,
        (secs_of_day / 3600) as u32,
        ((secs_of_day % 3600) / 60) as u32,
    )
}

/// Epoch seconds/milliseconds → local date (Howard Hinnant civil_from_days;
/// a UTC date with no timezone offset — the session stats row only needs
/// day-level precision, not worth pulling in chrono for this).
pub(super) fn history_abs_date(epoch_secs: i64) -> String {
    let epoch_secs = history_epoch_secs(epoch_secs);
    let days = epoch_secs.div_euclid(86_400);
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = if m <= 2 { y + 1 } else { y };
    format!("{year:04}-{m:02}-{d:02}")
}

pub(super) fn history_fmt_msg_time(timestamp: i64) -> String {
    let secs = if timestamp > 1_000_000_000_000 {
        timestamp / 1000
    } else {
        timestamp
    };
    let secs_in_day = secs.rem_euclid(86_400);
    let hour = secs_in_day / 3600;
    let min = (secs_in_day % 3600) / 60;
    format!("{hour:02}:{min:02}")
}

pub(super) fn compact_history_label(text: &str, max_chars: usize) -> String {
    let mut chars = text.chars();
    let mut output = String::new();
    for _ in 0..max_chars {
        let Some(ch) = chars.next() else {
            return text.to_string();
        };
        output.push(ch);
    }
    if chars.next().is_some() {
        output.push('…');
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_filter_labels_are_unicode_safe_and_bounded() {
        assert_eq!(compact_history_label("short", 24), "short");
        assert_eq!(compact_history_label("你好世界", 2), "你好…");
        assert_eq!(compact_history_label("abcdefghijkl", 5), "abcde…");
    }
}
