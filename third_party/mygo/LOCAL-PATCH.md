# Local patch over mygo v0.2.15

## Delta 1: terminal `Options.LocalDragSelect`

`plugins/terminal`: adds `Options.LocalDragSelect` — with it set, the
primary-button drag and the right click stay local to the view (text
selection, context menu) even when the program takes the mouse; the wheel
keeps reporting to the program.

Reason: Shardlane F146/F147 — scrollback-backed panes (shells, agy) need
plain-drag selection and a right-click menu while the daemon's attach
renderer force-enables mouse tracking. Alt-screen agent TUIs (pi) keep the
option unset and report everything.

## Delta 2: a field focuses its input (2026-10-07)

`ui.field` (the shared wrapper of `SearchField`, `Combobox`, `TokenField`
and the prompt input): the wrapper keeps its inner input in `focusTo`, and
`Element.Focus` hands the focus to it. `AutoFocus` on a field wrapper now
reaches the editor, which alone takes text — before, `AutoFocus` focused
the wrapper row, where typing, the caret and IME did nothing (the runtime
delivers text only to the focused element's editor).

Reason: Shardlane command palette (WIX-021) — the palette's `SearchField`
must take the keyboard the frame the dialog opens, keyboard-first like
Spotlight/Raycast. Regression: `TestSearchField` now types without a click.

## Upgrade path

When an upstream mygo release ships both (a terminal option of the same
name/semantics and field-focus delegation), delete the
`replace github.com/egoist/mygo => ../mygo` directive in `next/go.mod`,
bump the pin, and delete this checkout. Shardlane code already uses only
the public option and needs no changes.

Delta vs v0.2.15: `Options` field + a 4-line guard in `view.pointerEvent`;
an unexported `Element.focusTo` field, a delegation branch at the top of
`Element.Focus`, and one line in `field`.
