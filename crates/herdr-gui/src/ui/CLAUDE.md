# ui/
> L2 | Parent: ../CLAUDE.md

Home of Shardlane's shared UI elements (a GPUI rendition of the visual design
language). Division of labor with `ui_metrics.rs` (cross-view measurement
constants): composite elements live here, numeric contracts live there.

Members:
- `mod.rs`: module declarations and directory responsibility notes.
- `controls.rs`: form control family — `ControlSurface` (injected colors:
  foreground+background; callers derive border/accent/hairline), `Toggle`
  (36×20 monochrome capsule switch, 150ms thumb-slide animation),
  `Segmented<T>` (r7 bordered segmented selection), `ControlMenu<T>`
  (bordered trigger + downward popover attached via gpui-component's
  `DropdownMenu` trait, check on the selected item, popover always scrollable
  so long lists cannot overflow the window); dumb-element design — values are
  held by the caller, side effects flow back through on_change callbacks;
  `settings_view.rs` injects the terminal palette surface, `tasks.rs` injects
  the global theme.
- `menus.rs`: menu action helper — `menu_action` (generic entity menu item
  constructor, handler signature `(this, window, cx)`) and `menu_action_cx`
  (handler signature `(this, cx)`), removing the
  `PopupMenuItem::new(…).on_click(move |_, window, app| { entity.update(app, |…| {…}) })`
  boilerplate; consumed by sidebar/* and service_rows.
- `badge.rs`: compact pill badge primitives — `badge` (filled: bg+fg) and
  `outline_badge` (outlined: color+0.55 opacity border); consumed by
  history/formatting delegation, reusable later for sidebar chips.
- `drag.rs`: shared drag ghost row rendering — `DragGhostStyle`
  (parameterized padding/height/radius/font/gap) + `drag_ghost_row` (position
  offset + popover pill); consumed by sidebar/rows and
  workspace_management/dialogs.
- `tooltip.rs`: Tooltip closure factory — `tooltip_fn` (returns an
  `Fn(&mut Window, &mut App) -> AnyView` closure), removing the
  `.tooltip(|_, cx| cx.new(|_| Tooltip::new("…")).into())` boilerplate;
  consumed by sidebar/shell and right_panel.
- `empty_state.rs`: section empty-state placeholder —
  `empty_state(text, muted_color)` (p16 + FONT_BODY + the given muted color);
  consumed by right_panel.
