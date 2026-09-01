//! Shared keyboard/focus semantics for Shardlane-owned clickable surfaces.
//!
//! Two interaction primitives:
//! - `shardlane_interactive`: an ordinary clickable control that keeps a Tab stop
//!   (a few cases such as dialog buttons and tool rows).
//! - `shardlane_roving_row`: a list row. Rows themselves are not Tab stops (eliminating
//!   a dozens-of-presses Tab-trap); the section scroll container is the sole Tab stop.
//!   Once inside a list, ↑/↓ moves focus between rows, Enter/Space activates, and Esc
//!   returns focus to the container. Arrow navigation relies on GPUI 0.2.2's
//!   `FocusHandle::tab_stop(bool)` (a live flag; the Tab order skips handles set to false).
//!

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crepuscularity_gpui::prelude::*;
use crepuscularity_gpui::{App, Div, FocusHandle, Hsla, SharedString, Stateful, Window};

use crate::ui_metrics;

/// Roving focus registry for lists: rebuilds the visual order each frame starting from
/// `begin_frame`; FocusHandles are stable across frames (focus identity survives
/// re-renders), and rows that disappear are pruned on the next frame.
pub(crate) struct RovingList {
    order: RefCell<Vec<SharedString>>,
    handles: RefCell<HashMap<SharedString, FocusHandle>>,
}

/// Fixed key for the container handle in the handles table; not part of the row order.
const ROVING_CONTAINER_KEY: &str = "__container";

impl Default for RovingList {
    fn default() -> Self {
        Self {
            order: RefCell::new(Vec::new()),
            handles: RefCell::new(HashMap::new()),
        }
    }
}

impl RovingList {
    /// Called at the start of each frame's render: prunes the handle table against the
    /// previous frame's row set, then starts collecting this frame's order.
    pub(crate) fn begin_frame(&self) {
        let previous = std::mem::take(&mut *self.order.borrow_mut());
        self.handles
            .borrow_mut()
            .retain(|id, _| id == ROVING_CONTAINER_KEY || previous.iter().any(|live| live == id));
    }

    /// Get (or create) a row's focus handle and register it in render order. Rows are not Tab stops.
    pub(crate) fn row_handle(&self, id: &str, cx: &App) -> FocusHandle {
        let id = SharedString::from(id.to_string());
        {
            let mut order = self.order.borrow_mut();
            if order.last().is_none_or(|last| *last != id) {
                order.push(id.clone());
            }
        }
        self.handles
            .borrow_mut()
            .entry(id)
            .or_insert_with(|| cx.focus_handle().tab_stop(false))
            .clone()
    }

    /// Get (or create) the section container handle. The container is the list's only Tab stop.
    pub(crate) fn container_handle(&self, cx: &App) -> FocusHandle {
        self.handles
            .borrow_mut()
            .entry(SharedString::from(ROVING_CONTAINER_KEY))
            .or_insert_with(|| cx.focus_handle().tab_index(0).tab_stop(true))
            .clone()
    }

    fn indexed_handle(&self, index: Option<usize>) -> Option<FocusHandle> {
        let id = self.order.borrow().get(index?).cloned()?;
        self.handles.borrow().get(&id).cloned()
    }
    /// The neighboring row handle for the current row; returns None out of bounds (focus stays put, no wraparound).
    fn neighbor_handle(&self, id: &str, delta: isize) -> Option<FocusHandle> {
        let order = self.order.borrow();
        let index = order.iter().position(|row| row.as_ref() == id)?;
        let len = order.len();
        drop(order);
        let target = adjacent_index(len, index, delta)?;
        self.indexed_handle(Some(target))
    }

    pub(crate) fn first_row_handle(&self) -> Option<FocusHandle> {
        self.indexed_handle(Some(0))
    }

    pub(crate) fn last_row_handle(&self) -> Option<FocusHandle> {
        let last = self.order.borrow().len().checked_sub(1)?;
        self.indexed_handle(Some(last))
    }

    fn container_handle_unchecked(&self) -> Option<FocusHandle> {
        self.handles.borrow().get(ROVING_CONTAINER_KEY).cloned()
    }
}

/// Pure function for the adjacent index: out of bounds in either direction returns None.
fn adjacent_index(len: usize, index: usize, delta: isize) -> Option<usize> {
    let target = index.checked_add_signed(delta)?;
    (target < len).then_some(target)
}

pub(crate) trait InteractiveSurfaceExt {
    fn shardlane_interactive<F>(self, focus_color: Hsla, on_activate: F) -> Self
    where
        F: Fn(&mut Window, &mut App) + 'static;

    /// List rows: TrackFocus explicit handle + ↑/↓ roving navigation + Enter/Space activation + Esc back to the container.
    fn shardlane_roving_row<F>(
        self,
        list: &Rc<RovingList>,
        focus_handle: FocusHandle,
        focus_key: SharedString,
        focus_color: Hsla,
        on_activate: F,
    ) -> Self
    where
        F: Fn(&mut Window, &mut App) + 'static;
}

impl InteractiveSurfaceExt for Stateful<Div> {
    fn shardlane_interactive<F>(self, focus_color: Hsla, on_activate: F) -> Self
    where
        F: Fn(&mut Window, &mut App) + 'static,
    {
        let on_activate = Rc::new(on_activate);
        let click_activate = on_activate.clone();
        self.tab_index(0)
            .focus(move |style| {
                style.bg(focus_color.opacity(ui_metrics::INTERACTIVE_FOCUS_WASH_OPACITY))
            })
            .on_click(move |_, window, app| click_activate(window, app))
            .on_key_down(move |event, window, app| {
                if !is_activation_key(
                    event.keystroke.key.as_str(),
                    event.keystroke.modifiers.modified(),
                ) {
                    return;
                }
                window.prevent_default();
                app.stop_propagation();
                on_activate(window, app);
            })
    }

    fn shardlane_roving_row<F>(
        self,
        list: &Rc<RovingList>,
        focus_handle: FocusHandle,
        focus_key: SharedString,
        focus_color: Hsla,
        on_activate: F,
    ) -> Self
    where
        F: Fn(&mut Window, &mut App) + 'static,
    {
        let down_list = list.clone();
        let up_list = list.clone();
        let escape_list = list.clone();
        let on_activate = Rc::new(on_activate);
        let click_activate = on_activate.clone();
        self.track_focus(&focus_handle)
            .focus(move |style| {
                style.bg(focus_color.opacity(ui_metrics::INTERACTIVE_FOCUS_WASH_OPACITY))
            })
            .on_click(move |_, window, app| click_activate(window, app))
            .on_key_down(move |event, window, app| {
                if event.keystroke.modifiers.modified() {
                    return;
                }
                match event.keystroke.key.as_str() {
                    "enter" | "space" => {
                        window.prevent_default();
                        app.stop_propagation();
                        on_activate(window, app);
                    }
                    "down" => {
                        app.stop_propagation();
                        if let Some(next) = down_list.neighbor_handle(&focus_key, 1) {
                            window.focus(&next);
                        }
                    }
                    "up" => {
                        app.stop_propagation();
                        if let Some(prev) = up_list.neighbor_handle(&focus_key, -1) {
                            window.focus(&prev);
                        }
                    }
                    "escape" => {
                        app.stop_propagation();
                        if let Some(container) = escape_list.container_handle_unchecked() {
                            window.focus(&container);
                        }
                    }
                    _ => {}
                }
            })
    }
}

fn is_activation_key(key: &str, modified: bool) -> bool {
    !modified && matches!(key, "enter" | "space")
}

#[cfg(test)]
mod tests {
    use super::{adjacent_index, is_activation_key};

    #[test]
    fn keyboard_activation_is_enter_or_space_without_modifiers() {
        assert!(is_activation_key("enter", false));
        assert!(is_activation_key("space", false));
        assert!(!is_activation_key("enter", true));
        assert!(!is_activation_key("right", false));
    }

    #[test]
    fn adjacent_index_stays_inside_the_list() {
        assert_eq!(adjacent_index(3, 0, 1), Some(1));
        assert_eq!(adjacent_index(3, 1, -1), Some(0));
        assert_eq!(adjacent_index(3, 0, -1), None);
        assert_eq!(adjacent_index(3, 2, 1), None);
        assert_eq!(adjacent_index(1, 0, 1), None);
        assert_eq!(adjacent_index(1, 0, -1), None);
        assert_eq!(adjacent_index(0, 0, 1), None);
    }
}
