//! Tooltip closure factory: removes the
//! `.tooltip(|_, cx| cx.new(|_| Tooltip::new("…")).into())` boilerplate.
//!
//! [INPUT]: gpui_component Tooltip
//! [OUTPUT]: `tooltip_fn` — returns a closure usable directly as `.tooltip(…)`
//! [POS]: the ui module's tooltip helper layer; consumed by sidebar/shell and
//! right_panel

use gpui::{AnyView, App, AppContext as _, SharedString, Window};
use gpui_component::tooltip::Tooltip;

/// Returns a closure satisfying the `.tooltip(…)` signature.
pub(crate) fn tooltip_fn(
    text: impl Into<SharedString>,
) -> impl Fn(&mut Window, &mut App) -> AnyView + 'static {
    let text: SharedString = text.into();
    move |_, cx: &mut App| cx.new(|_| Tooltip::new(text.clone())).into()
}
