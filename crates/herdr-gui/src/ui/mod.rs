//! Shardlane shared UI element module (a GPUI rendition of the visual design
//! language).
//!
//! [INPUT]: depends on interaction.rs's focus/keyboard semantics, theme.rs's
//! font-size tokens, and gpui-component's DropdownMenu/Selectable/Icon
//! [OUTPUT]: exposes the controls module (Toggle/Segmented/ControlMenu/ControlSurface)
//! [POS]: home of the presentation-layer shared elements declared in main.rs;
//! consumed by settings_view.rs and scripts.rs. Division of labor with
//! ui_metrics.rs (cross-view measurement contracts): composite elements live
//! here, numeric constants live there.

pub(crate) mod badge;
pub(crate) mod controls;
pub(crate) mod drag;
pub(crate) mod empty_state;
pub(crate) mod list_card;
pub(crate) mod menus;
pub(crate) mod syntax;
pub(crate) mod tooltip;
