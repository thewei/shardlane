//! Menu action helper: removes the
//! `PopupMenuItem::new(…).on_click(move |_, window, app| { entity.update(app, |…| {…}) })`
//! boilerplate.
//!
//! [INPUT]: gpui Entity/Context/Window, gpui-component PopupMenuItem
//! [OUTPUT]: `menu_action` — a generic entity menu item constructor
//! [POS]: the ui module's menu helper layer; consumed by menu-dense areas like
//! sidebar/*, header_view, and history/page_view

use gpui::{Context, Entity, Window};
use gpui_component::menu::PopupMenuItem;

/// Builds a `PopupMenuItem` with `on_click` already wired, absorbing the
/// `entity.update(app, …)` boilerplate.
///
/// Covers the most common `(this, window, cx)` signature (~60% of uses).
pub(crate) fn menu_action<T: 'static>(
    label: impl Into<gpui::SharedString>,
    target: &Entity<T>,
    handler: impl Fn(&mut T, &mut Window, &mut Context<T>) + 'static,
) -> PopupMenuItem {
    let target = target.clone();
    PopupMenuItem::new(label).on_click(move |_, window, app| {
        target.update(app, |this, cx| handler(this, window, cx));
    })
}

/// Same as [`menu_action`], but the handler signature is `(this, cx)` — for
/// cases that do not need the window.
pub(crate) fn menu_action_cx<T: 'static>(
    label: impl Into<gpui::SharedString>,
    target: &Entity<T>,
    handler: impl Fn(&mut T, &mut Context<T>) + 'static,
) -> PopupMenuItem {
    let target = target.clone();
    PopupMenuItem::new(label).on_click(move |_, _window, app| {
        target.update(app, |this, cx| handler(this, cx));
    })
}
