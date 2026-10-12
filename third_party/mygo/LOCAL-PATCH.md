# Local patch over mygo v0.4.5

## Delta 1: terminal `Options.LocalDragSelect`

`plugins/terminal`: adds `Options.LocalDragSelect` — with it set, the
primary-button drag and the right click stay local to the view (text
selection, context menu) even when the program takes the mouse; the wheel
keeps reporting to the program.

Reason: Shardlane F146/F147 — scrollback-backed panes (shells, agy) need
plain-drag selection and a right-click menu while the daemon's attach
renderer force-enables mouse tracking. Alt-screen agent TUIs (pi) keep the
option unset and report everything.

Sites: `plugins/terminal/terminal.go` (`Options` field) and a 4-line guard
at the top of `view.pointerEvent`.

## Delta 2: a field focuses its input (2026-10-07)

`ui.field` (the shared wrapper of `SearchField`, `Combobox`, `TokenField`
and the prompt input): the wrapper keeps its inner input in `focusTo`, and
`node.Focus` hands the focus to it. `AutoFocus` on a field wrapper now
reaches the editor, which alone takes text — before, `AutoFocus` focused
the wrapper row, where typing, the caret and IME did nothing (the runtime
delivers text only to the focused element's editor).

Reason: Shardlane command palette (WIX-021) — the palette's `SearchField`
must take the keyboard the frame the dialog opens, keyboard-first like
Spotlight/Raycast. Regression: `TestSearchField` now types without a click.

Sites: an unexported `node.focusTo` field in `ui/element.go`, a delegation
branch in `node.Focus` after its nil/invalid-state guard in `ui/input.go`, one line in `field`
(`ui/combobox.go`), and the `TestSearchField` regression in
`ui/combobox_test.go`.

## Delta 3: multi-display tray flyout anchoring on macOS (2026-10-12)

`internal/darwin`: in multi-monitor setups, macOS status items (`NSStatusItem`)
reside physically in the primary screen's window (`screens[0]`). When the user
clicks the tray icon while operating on a secondary display, the click event's
mouse position identifies the target display; `tray.recordClick` computes the
corresponding menu-bar anchor on that display, `tray.Bounds` projects that anchor,
and `w.popoverAnchor` binds an auxiliary transparent anchor window on that display
so that the native `NSPopover` (or `PlaceFlyout`) renders directly on the operated
screen without jumping to the primary display and stealing focus.

Reason: Shardlane multi-monitor workflow — clicking the status tray while working
in an editor on a secondary monitor must display the quick panel on that monitor
rather than dragging the window and cursor focus over to the main screen.

Sites: `clickScreen` and `clickAnchor` tracking + `recordClick` in
`internal/darwin/menu.go`; `trayAnchorWin` and secondary-display anchoring +
dismissal cleanup in `internal/darwin/popover.go`; `trayAnchorWin` field in
`internal/darwin/window.go`.

## Upgrade path

Rebased onto v0.4.5 on 2026-10-12 (terminal option and field-focus delegation
remain absent upstream; multi-monitor tray anchoring added 2026-10-12).
When an upstream mygo release ships these fixes, delete the
`replace github.com/egoist/mygo => ./third_party/mygo` directive in `go.mod`,
bump the pin, and delete this checkout. Shardlane code already uses only the
public option and needs no changes.

Delta vs v0.4.5: an `Options` field + a 4-line guard in `view.pointerEvent`;
an unexported `node.focusTo` field, a delegation branch after the nil-safe
`hasState` guard in `node.Focus`, and one line in `field`; multi-display tray
anchoring in `internal/darwin/{menu,popover,window}.go`. Preserve the upstream
`TestOptionalElementInput/nil` behavior when rebasing this patch.
