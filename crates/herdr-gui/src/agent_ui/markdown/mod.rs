//! agent_ui/markdown — streaming Markdown engine (Live Conversation presentation backend).
//!
//! [INPUT]: depends on pulldown-cmark/regex and GPUI text primitives; no app state.
//! [OUTPUT]: parser (incremental GFM parsing), mend (streaming hanging-marker
//! repair), highlight (paint-only syntax highlighting), render (BlockTree → GPUI
//! elements, one block per shaped text), selection (cross-element selection),
//! veil (streaming reveal).
//! [POS]: Markdown primitives for herdr-gui `agent_ui`. Since 2026-08-28 the sole
//! Markdown backend for History Detail (mend=false static body) and Live Chat
//! (streaming tail mend=true) (audit CHAT-A05: shared rendering primitives rule out
//! TextView dual-backend visual drift).

pub mod cache;
pub mod highlight;
pub mod mend;
pub mod parser;
pub mod render;
pub mod selection;
pub mod veil;

use gpui::App;
use gpui_component::ActiveTheme as _;

use self::render::PaletteSource;

/// Derive the Markdown palette input from gpui-component's ActiveTheme.
///
/// This is a deliberately narrowed adapter surface: only the color surfaces the
/// Markdown renderer needs are mapped from Shardlane semantic tokens. Surfaces
/// without a dedicated Shardlane token (code/inset/overlay/selection) are
/// derived from foreground opacity, in the same language as the composer card
/// (foreground.opacity 0.035 base, accent focus border).
pub fn palette_source_from_active(cx: &App) -> PaletteSource {
    let theme = cx.theme();
    let dark = theme.mode.is_dark();
    let foreground = theme.foreground;
    PaletteSource {
        text: foreground,
        secondary: theme.muted_foreground,
        tertiary: theme.muted_foreground.opacity(0.8),
        ghost: theme.muted_foreground.opacity(0.62),
        border: theme.border,
        inset: foreground.opacity(if dark { 0.05 } else { 0.04 }),
        overlay: foreground.opacity(if dark { 0.09 } else { 0.07 }),
        code_text: foreground,
        code_wash: foreground.opacity(if dark { 0.045 } else { 0.035 }),
        selection: theme.accent.opacity(0.28),
        accent: theme.accent,
        success: theme.success,
        danger: theme.danger,
        is_dark: dark,
    }
}
