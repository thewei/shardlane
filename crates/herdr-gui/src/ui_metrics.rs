//! Small shared presentation contracts for Shardlane-owned UI surfaces.
//!
//! Keep this module intentionally narrow: only metrics/state strengths that must stay
//! aligned across multiple views belong here. Optical offsets and component-specific
//! preview styling remain local to their view modules.
//!

use crepuscularity_gpui::{px, Pixels};

pub(crate) const SPACE_XS: Pixels = px(4.0);
pub(crate) const SPACE_SM: Pixels = px(8.0);
pub(crate) const SPACE_MD: Pixels = px(12.0);
pub(crate) const SPACE_ICON: Pixels = px(6.0);

pub(crate) const CONTENT_INSET: Pixels = px(8.0);
pub(crate) const SIDEBAR_EDGE_INSET: Pixels = px(10.0);
pub(crate) const ROW_HEIGHT_PRIMARY: Pixels = px(32.0);
pub(crate) const ROW_HEIGHT_SUB: Pixels = px(26.0);
pub(crate) const DIALOG_CONTENT_GAP: Pixels = px(12.0);

pub(crate) const INTERACTIVE_FOCUS_OPACITY: f32 = 0.72;
pub(crate) const INTERACTIVE_HOVER_OPACITY: f32 = 0.55;
pub(crate) const INTERACTIVE_PRESSED_OPACITY: f32 = 0.70;
/// Strength of the focus background wash. The focus style uses a background overlay
/// rather than a border to avoid 1px layout shifts at fixed row heights;
/// GPUI 0.2.2's BoxShadow has no inset, so an inner-shadow ring is not possible.
pub(crate) const INTERACTIVE_FOCUS_WASH_OPACITY: f32 = 0.22;

/// Collapses an arbitrary string into a form suitable for a single-line label:
/// compresses `\r`, `\n`, `\t`, and runs of whitespace into single ASCII spaces,
/// and trims both ends.
///
/// Used for: Services command, Script summaries, Agent title fallback, Search
/// subtitles — text that must display on one line but may contain newlines.
///
/// Do **not** use for Chat/Markdown bodies, Terminal output, or History message bodies.
pub(crate) fn single_line_label(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut last_was_space = false;
    for ch in text.chars() {
        if ch == '\r' || ch == '\n' || ch == '\t' || ch == ' ' {
            if !last_was_space && !out.is_empty() {
                out.push(' ');
                last_was_space = true;
            }
        } else {
            out.push(ch);
            last_was_space = false;
        }
    }
    // trailing space from above
    if out.ends_with(' ') {
        out.pop();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_line_collapses_whitespace() {
        assert_eq!(single_line_label("foo\nbar"), "foo bar");
        assert_eq!(single_line_label("foo\r\nbar"), "foo bar");
        assert_eq!(single_line_label("foo\t\tbar"), "foo bar");
        assert_eq!(single_line_label("  foo  bar  "), "foo bar");
        assert_eq!(single_line_label("\nhello\n"), "hello");
        assert_eq!(single_line_label(""), "");
        assert_eq!(single_line_label("no change"), "no change");
        assert_eq!(single_line_label("a\nb\tc"), "a b c");
    }
}
