# Local patch over mygo v0.5.5

Four deltas, re-applied by hand after every pin bump. They live outside
Shardlane and stay as small as deletion allows.

## Delta 4: terminal OSC 7501 program status (2026-10-15)

`plugins/terminal`: adds `Options.OnProgramStatus func(report []byte)` —
the body bytes of one OSC 7501 report (Program Status Protocol, spec rev
0.4), framed by a small scanner (`osc7501.go`) that rides the same output
bytes libghostty-vt reads. The emulator core ignores the sequence as an
unknown OSC; the scanner knows only the framing (OSC `7501;` through ST,
4096-byte cap, chunk-safe). With the callback set the terminal answers the
protocol's feature-detection query `OSC 7501 ; ? ST` with the full fixed
state and kind lists; without it, support is never claimed. Pairs,
records, lifetime and merge policy live in the embedding app
(herdr-client `internal/agentstatus`), which owns the protocol semantics.

Reason: Shardlane agent panes — Claude Code 2.1.295+ emits native status
reports (working/blocked/done/idle/error) that must reach the pane header
without hooks; agents without the protocol keep the hook fallback.

Sites: `plugins/terminal/osc7501.go` (new scanner + tests), `Options`
field and the doc comment in `plugins/terminal/terminal.go`, the `events`
struct, the feed in `Terminal.write`, and the dispatch in `Terminal.fire`.

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

Rebased onto v0.5.5 on 2026-10-16 (the patch applied cleanly; the terminal
option, field-focus delegation, multi-display tray anchoring and OSC 7501 are
all still absent upstream — v0.5.x added `Options` of its own such as
`OnPasteStatus`, but none of these four). The four deltas keep their size:
terminal.go grows by the same 33 lines, view.go by 6, `ui/` by 12 in three
files, `internal/darwin/` by 84 in three files, and the OSC 7501 scanner
files copy over unchanged. When an upstream mygo release ships these fixes,
delete the `replace github.com/egoist/mygo => ./third_party/mygo` directive in
`go.mod`, bump the pin, and delete this checkout. Shardlane code already uses
only the public option and needs no changes.

Delta vs v0.5.5: an `Options` field + a 4-line guard in `view.pointerEvent`;
an unexported `node.focusTo` field, a delegation branch after the nil-safe
`hasState` guard in `node.Focus`, and one line in `field`; multi-display tray
anchoring in `internal/darwin/{menu,popover,window}.go`; the OSC 7501 scanner
file, an `Options` field, the `events` fields and the feed/fire hooks in
`plugins/terminal`. Preserve the upstream
`TestOptionalElementInput/nil` behavior when rebasing this patch.

## Linux

MyGo is cross-platform: `internal/linux/` is a first-class backend and
`go tool mygo build --platform linux/amd64` packages a binary, a `.deb`, a
`.tar.gz` and an `install.sh` without macOS. The two
`runtime.GOOS != "darwin"` branches in `cmd/mygo/bundle.go` only skip icon
compilation and code signing (warning, not failing). Shardlane compiles and
runs on Linux under this fork; `internal/nativeui/pty_other.go` supplies the
no-attach-pty fallback for the non-darwin build.
