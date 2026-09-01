//! Help overlay: keyboard shortcuts rendered from the shortcut registry.
//!
//! [INPUT]: `shortcuts::REGISTRY` + the user's `ShortcutConfig` — the same sources the
//! Settings → Shortcuts page and the live keymap resolve bindings from
//! [OUTPUT]: `help_overlay(theme, shortcuts)` — the content-area help overlay
//! [POS]: Presentation only; binding semantics live in shortcuts.rs

use crate::shortcuts::{self, ShortcutConfig, REGISTRY};
use crate::theme::{FONT_DECORATIVE, FONT_LIST_TITLE, FONT_META};
use crepuscularity_gpui::prelude::*;
use crepuscularity_gpui::{div, px, IntoElement, Keystroke};
use gpui_component::{divider::Divider, kbd::Kbd, theme::ThemeColor};

/// Audit E04: the overlay is generated from `shortcuts::REGISTRY` (grouped by
/// `ShortcutCategory`, chords resolved through the user's overrides) so the help
/// overlay, Settings → Shortcuts, and the real bindings cannot drift apart.
pub fn help_overlay(theme: ThemeColor, shortcuts_config: &ShortcutConfig) -> impl IntoElement {
    let mut overlay = div()
        .absolute()
        .top(px(12.0))
        .right(px(12.0))
        .w(px(320.0))
        .rounded(px(10.0))
        .bg(theme.popover)
        .border_1()
        .border_color(theme.border)
        .p_4()
        .flex()
        .flex_col()
        .gap_2()
        .child(
            div()
                .text_size(FONT_LIST_TITLE)
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme.popover_foreground)
                .child("Keyboard Shortcuts"),
        )
        .child(Divider::horizontal());

    // REGISTRY is ordered by category; emit a section label whenever it changes
    // (hairlines between sections, none before the first).
    let mut current_category = None;
    for entry in REGISTRY {
        if current_category != Some(entry.category) {
            if current_category.is_some() {
                overlay = overlay.child(Divider::horizontal().my_1());
            }
            current_category = Some(entry.category);
            overlay = overlay.child(section_label(entry.category.label(), theme));
        }
        overlay = overlay.child(key_row(
            shortcuts::effective_chord(entry.id, shortcuts_config),
            entry.label,
            theme,
        ));
    }

    overlay
}

fn section_label(text: &str, theme: ThemeColor) -> impl IntoElement {
    div()
        .pt_1()
        .text_size(FONT_DECORATIVE)
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme.muted_foreground)
        .child(text.to_string())
}

/// One registry row: the resolved chord on the left (Kbd chip when the chord parses,
/// a fallback chip for unbound/disabled entries), the command label on the right.
fn key_row(chord: Option<&str>, action: &str, theme: ThemeColor) -> impl IntoElement {
    let key = match chord.and_then(|chord| Keystroke::parse(chord).ok()) {
        Some(stroke) => Kbd::new(stroke).into_any_element(),
        None => div()
            .rounded(px(4.0))
            .border_1()
            .border_color(theme.border)
            .px_1()
            .text_size(FONT_DECORATIVE)
            .text_color(theme.foreground)
            .child(chord.unwrap_or("—").to_string())
            .into_any_element(),
    };
    div()
        .h(px(25.0))
        .flex()
        .items_center()
        .justify_between()
        .gap_4()
        .child(key)
        .child(
            div()
                .flex_1()
                .text_right()
                .text_size(FONT_META)
                .text_color(theme.muted_foreground)
                .child(action.to_string()),
        )
}
