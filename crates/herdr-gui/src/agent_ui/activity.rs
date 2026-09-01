//! Shared tool activity / work fold presentation primitives (shared by History
//! Detail and Live Chat).
//!
//! [INPUT]: depends on shardlane-history's TranscriptMessage/ToolCall,
//! gpui-component Icon/Button, and the crate root's ContentSurfaceTheme;
//! callbacks are injected by the caller.
//! [OUTPUT]: classify_tool/activity_icon (tool name → semantic class/icon),
//! turn_work_summary (Worked fold row label and duration), diff_lines_from_tool
//! (explicit old/new or unified diff text → colored lines),
//! render_activity_row / render_worked_fold_row / render_turn_footer.
//! [POS]: activity presentation primitives for herdr-gui `agent_ui`. Row
//! geometry/icons/status colors are the SSOT of the shared visual language:
//! History and Chat both render tool activity and Worked folds through here and
//! must not duplicate it. Owns no expansion state (caller provides expanded +
//! on_toggle).

use gpui::prelude::FluentBuilder as _;
use gpui::{
    div, px, AnyElement, FontWeight, Hsla, InteractiveElement as _, IntoElement,
    ParentElement as _, SharedString, StatefulInteractiveElement as _, Styled as _,
};
use gpui_component::{
    h_flex, v_flex, Icon, IconName as ComponentIconName, Sizable as _, WindowExt as _,
};
use shardlane_history::{Role, ToolCall, TranscriptMessage};
use std::time::Duration;

use crate::theme;
use crate::ui_metrics::SPACE_ICON;

/// Tool semantic class (the Phase-1 subset from plan §6.3; anything not safely
/// classifiable falls into Generic).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ActivityKind {
    Command,
    FileRead,
    FileSearch,
    FileChange,
    WebSearch,
    Plan,
    Generic,
}

/// Narrowed classifier (robust prefix/exact-name matching).
pub(crate) fn classify_tool(name: &str) -> ActivityKind {
    let lower = name.to_lowercase();
    match lower.as_str() {
        "bash" | "shell" | "local_shell" | "local_shell_call" | "terminal" | "exec" => {
            ActivityKind::Command
        }
        "read" | "view" | "cat" | "open" => ActivityKind::FileRead,
        "grep" | "glob" | "find" | "search" | "list" | "ls" => ActivityKind::FileSearch,
        "edit" | "write" | "multiedit" | "notebookedit" | "apply_patch" | "applypatch" => {
            ActivityKind::FileChange
        }
        "websearch" | "web_fetch" | "fetch" | "fetchpatch" => ActivityKind::WebSearch,
        "todowrite" | "todoread" | "update_plan" | "plan" => ActivityKind::Plan,
        _ => {
            if lower.contains("patch") || lower.contains("diff") {
                ActivityKind::FileChange
            } else if lower.contains("search") || lower.contains("find") {
                ActivityKind::FileSearch
            } else if lower.contains("read") {
                ActivityKind::FileRead
            } else {
                ActivityKind::Generic
            }
        }
    }
}

pub(crate) fn activity_icon(kind: ActivityKind) -> ComponentIconName {
    match kind {
        ActivityKind::Command => ComponentIconName::SquareTerminal,
        ActivityKind::FileRead => ComponentIconName::File,
        ActivityKind::FileSearch => ComponentIconName::Search,
        ActivityKind::FileChange => ComponentIconName::Replace,
        ActivityKind::WebSearch => ComponentIconName::Globe,
        ActivityKind::Plan => ComponentIconName::BookOpen,
        ActivityKind::Generic => ComponentIconName::Ellipsis,
    }
}

/// Worked fold row label: "Worked for Xs" when a duration can be derived,
/// otherwise "Worked"; the step count is always shown (number of hidden work
/// rows).
pub(crate) fn worked_summary_label(steps: usize, duration: Option<Duration>) -> String {
    let base = match duration {
        Some(duration) if duration.as_secs_f32() >= 1.0 => {
            format!("Worked for {:.0}s", duration.as_secs_f32())
        }
        Some(duration) if duration.as_secs_f32() > 0.0 => {
            format!("Worked for {:.1}s", duration.as_secs_f32())
        }
        _ => "Worked".to_string(),
    };
    format!("{base} · {steps} steps")
}

/// Derive work duration from turn message timestamps (last minus first; None
/// without valid timestamps).
pub(crate) fn turn_work_duration(
    messages: &[TranscriptMessage],
    range: std::ops::Range<usize>,
) -> Option<Duration> {
    let stamps: Vec<i64> = messages[range]
        .iter()
        .filter_map(|message| message.timestamp)
        .filter(|ts| *ts > 0)
        .collect();
    let first = *stamps.first()?;
    let last = *stamps.last()?;
    // Timestamps come in both seconds and milliseconds: normalize the pair by
    // magnitude before differencing.
    let (first, last) = normalize_stamp_pair(first, last);
    (last > first).then(|| Duration::from_millis((last - first) as u64))
}

fn normalize_stamp_pair(a: i64, b: i64) -> (i64, i64) {
    fn to_ms(ts: i64) -> i64 {
        if ts > 1_000_000_000_000 {
            ts
        } else {
            ts * 1000
        }
    }
    (to_ms(a), to_ms(b))
}

/// One line of diff inside expanded details.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DiffLineKind {
    Context,
    Added,
    Removed,
    Hunk,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DiffLine {
    pub(crate) kind: DiffLineKind,
    pub(crate) text: String,
}

/// Extract explicit diff lines from tool data. Only two sources are honored:
/// 1. unified diff text (`---/+++/@@/+/ -` prefixes), in the input
///    `patch`/`diff`/`content` fields or at the start of the output;
/// 2. an `old_string`/`new_string` field pair in Edit/Write-style input.
///
/// Returns None without explicit diff data — never synthesize a diff from
/// arbitrary terminal output (plan §12.8).
pub(crate) fn diff_lines_from_tool(tool: &ToolCall) -> Option<Vec<DiffLine>> {
    if let Some(input) = tool.input.as_deref() {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(input) {
            if let Some(lines) = diff_from_old_new(&value) {
                return Some(lines);
            }
            for key in ["patch", "diff", "content"] {
                if let Some(text) = value.get(key).and_then(|v| v.as_str()) {
                    let lines = parse_unified_diff(text);
                    if !lines.is_empty() {
                        return Some(lines);
                    }
                }
            }
        } else {
            let lines = parse_unified_diff(input);
            if !lines.is_empty() {
                return Some(lines);
            }
        }
    }
    if let Some(output) = tool.output.as_deref() {
        let lines = parse_unified_diff(output);
        if !lines.is_empty() {
            return Some(lines);
        }
    }
    None
}

fn diff_from_old_new(value: &serde_json::Value) -> Option<Vec<DiffLine>> {
    let old = value.get("old_string").and_then(|v| v.as_str())?;
    let new = value.get("new_string").and_then(|v| v.as_str())?;
    let mut lines: Vec<DiffLine> = Vec::new();
    if let Some(path) = value.get("file_path").and_then(|v| v.as_str()) {
        lines.push(DiffLine {
            kind: DiffLineKind::Hunk,
            text: path.to_string(),
        });
    }
    for line in old.lines() {
        lines.push(DiffLine {
            kind: DiffLineKind::Removed,
            text: format!("-{line}"),
        });
    }
    for line in new.lines() {
        lines.push(DiffLine {
            kind: DiffLineKind::Added,
            text: format!("+{line}"),
        });
    }
    (!lines.is_empty()).then_some(lines)
}

fn parse_unified_diff(text: &str) -> Vec<DiffLine> {
    let looks_like_diff = text
        .lines()
        .any(|line| line.starts_with("@@") || line.starts_with("diff --git"));
    if !looks_like_diff {
        return Vec::new();
    }
    text.lines()
        .filter_map(|line| {
            let kind = if line.starts_with("@@")
                || line.starts_with("diff --git")
                || line.starts_with("index ")
            {
                DiffLineKind::Hunk
            } else if line.starts_with('+') {
                DiffLineKind::Added
            } else if line.starts_with('-') {
                DiffLineKind::Removed
            } else if line.starts_with('\\') {
                return None;
            } else {
                DiffLineKind::Context
            };
            Some(DiffLine {
                kind,
                text: line.to_string(),
            })
        })
        .collect()
}

pub(crate) fn diff_line_color(kind: DiffLineKind, theme: &crate::ContentSurfaceTheme) -> Hsla {
    match kind {
        DiffLineKind::Context => theme.muted,
        DiffLineKind::Hunk => theme.primary,
        DiffLineKind::Added => theme.success,
        DiffLineKind::Removed => theme.danger,
    }
}

fn mono_meta(text: impl Into<SharedString>, color: Hsla) -> AnyElement {
    div()
        .w_full()
        .min_w_0()
        .font_family("monospace")
        .whitespace_normal()
        .text_size(theme::FONT_META)
        .text_color(color)
        .child(text.into())
        .into_any_element()
}

/// Expanded tool detail: diff (when explicit data exists) → output → input.
pub(crate) fn render_tool_detail(
    tool: &ToolCall,
    theme: &crate::ContentSurfaceTheme,
) -> AnyElement {
    let mut detail = v_flex().w_full().min_w_0().gap(px(4.0));
    if let Some(lines) = diff_lines_from_tool(tool) {
        let panel = v_flex()
            .w_full()
            .min_w_0()
            .p(px(8.0))
            .rounded(px(6.0))
            .bg(theme.background.opacity(0.65))
            .border_1()
            .border_color(theme.border.opacity(0.35))
            .children(
                lines
                    .into_iter()
                    .take(400)
                    .map(|line| mono_meta(line.text, diff_line_color(line.kind, theme))),
            );
        detail = detail.child(panel);
    }
    if let Some(output) = tool
        .output
        .as_deref()
        .filter(|text| !text.trim().is_empty())
    {
        detail = detail.child(mono_meta(output.to_string(), theme.muted));
    }
    if let Some(input) = tool.input.as_deref().filter(|text| !text.trim().is_empty()) {
        detail = detail.child(mono_meta(input.to_string(), theme.muted.opacity(0.8)));
    }
    if tool.output.is_none() && tool.input.is_none() {
        detail = detail.child(mono_meta("(no detail)", theme.muted.opacity(0.6)));
    }
    detail.into_any_element()
}

/// Compact tool activity row (shared visual language): chevron + class icon +
/// tool name + preview + status dot; click expands/collapses the detail.
pub(crate) fn render_activity_row(
    id: impl Into<gpui::ElementId>,
    tool: &ToolCall,
    expanded: bool,
    on_toggle: impl Fn(&mut gpui::Window, &mut gpui::App) + 'static,
    theme: &crate::ContentSurfaceTheme,
) -> AnyElement {
    let kind = classify_tool(&tool.name);
    let status_color = if tool.is_error {
        theme.danger
    } else if tool.output.is_some() {
        theme.success
    } else {
        theme.muted.opacity(0.5)
    };
    let preview = tool.input_preview.trim();
    h_flex()
        .id(id.into())
        .w_full()
        .min_w_0()
        .gap(SPACE_ICON)
        .py(px(3.0))
        .items_center()
        .cursor_pointer()
        .text_size(theme::FONT_META)
        .text_color(theme.muted)
        .hover(|style| style.text_color(theme.foreground))
        .on_click(move |_, window, app| on_toggle(window, app))
        .child(
            Icon::new(ComponentIconName::ChevronRight)
                .with_size(px(11.0))
                .flex_shrink_0()
                .when(expanded, |icon| {
                    icon.rotate(gpui::Radians(std::f32::consts::FRAC_PI_2))
                }),
        )
        .child(Icon::new(activity_icon(kind)).with_size(px(12.0)))
        .child(
            div()
                .flex_shrink_0()
                .font_family("monospace")
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme.foreground.opacity(0.9))
                .child(tool.name.clone()),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .when(preview.is_empty(), |el| el.child("—"))
                .when(!preview.is_empty(), |el| el.child(preview.to_string())),
        )
        .child(
            div()
                .size(px(6.0))
                .rounded_full()
                .flex_shrink_0()
                .bg(status_color),
        )
        .into_any_element()
}

/// "Worked …" fold row (anchored at the start of the work; when expanded the
/// caller renders the hidden rows below it).
pub(crate) fn render_worked_fold_row(
    id: impl Into<gpui::ElementId>,
    label: String,
    expanded: bool,
    on_toggle: impl Fn(&mut gpui::Window, &mut gpui::App) + 'static,
    theme: &crate::ContentSurfaceTheme,
) -> AnyElement {
    h_flex()
        .id(id.into())
        .w_full()
        .min_w_0()
        .gap(SPACE_ICON)
        .py(px(4.0))
        .px(px(8.0))
        .rounded(px(7.0))
        .bg(theme.hover.opacity(0.5))
        .cursor_pointer()
        .items_center()
        .text_size(theme::FONT_META)
        .text_color(theme.muted)
        .hover(|style| style.bg(theme.hover).text_color(theme.foreground))
        .on_click(move |_, window, app| on_toggle(window, app))
        .child(
            Icon::new(ComponentIconName::ChevronRight)
                .with_size(px(11.0))
                .flex_shrink_0()
                .when(expanded, |icon| {
                    icon.rotate(gpui::Radians(std::f32::consts::FRAC_PI_2))
                }),
        )
        .child(
            Icon::new(ComponentIconName::CircleCheck)
                .with_size(px(12.0))
                .text_color(theme.success),
        )
        .child(div().min_w_0().truncate().child(label))
        .into_any_element()
}

/// Turn footer: belongs to the turn as a whole (copy grabs the turn's full
/// answer text); the time is optional.
pub(crate) fn render_turn_footer(
    id: impl Into<gpui::ElementId>,
    time_label: Option<String>,
    copy_text: String,
    theme: &crate::ContentSurfaceTheme,
) -> AnyElement {
    let mut row = h_flex()
        .id(id.into())
        .w_full()
        .min_w_0()
        .gap(px(8.0))
        .pt(px(2.0))
        .pb(px(6.0))
        .items_center()
        .text_size(theme::FONT_META)
        .text_color(theme.muted.opacity(0.8))
        .child(
            Icon::new(ComponentIconName::CircleCheck)
                .with_size(px(11.0))
                .text_color(theme.success.opacity(0.8)),
        );
    if let Some(time_label) = time_label {
        row = row.child(div().child(time_label));
    }
    row = row.child(
        div()
            .id("turn-footer-copy")
            .cursor_pointer()
            .hover(|style| style.text_color(theme.foreground))
            .child("Copy answer")
            .on_click(move |_, window, app| {
                app.write_to_clipboard(crepuscularity_gpui::ClipboardItem::new_string(
                    copy_text.clone(),
                ));
                window.push_notification("Answer copied", app);
            }),
    );
    row.into_any_element()
}

/// Turn answer text (for the footer's copy): all non-empty assistant text in
/// the turn, concatenated in order.
pub(crate) fn turn_answer_text(
    messages: &[TranscriptMessage],
    range: std::ops::Range<usize>,
) -> String {
    messages[range]
        .iter()
        .filter(|message| message.role == Role::Assistant && !message.text.trim().is_empty())
        .map(|message| message.text.trim())
        .collect::<Vec<_>>()
        .join("\n\n")
}
