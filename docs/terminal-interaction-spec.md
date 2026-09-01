# Terminal Interaction Specification

Status: canonical interaction-quality contract — **TUI-only current state**
Baseline: 2026-08-28 (rewritten after the TUI-only convergence; historical
per-Pane wording is not reproduced here)
Architecture ownership: `client-product-architecture.md` (§5)
Convergence record: the 2026-08-27 TUI-only cutover (see `client-product-architecture.md` §5)

This document defines **what terminal interaction must feel like** in the
current architecture: the normal work surface is the singleton hosted Herdr
TUI — one local `herdr` child, one PTY, one private Ghostty terminal model,
one GPUI presentation/input bridge. It does not redefine runtime ownership.

Deleted with the Embedded path and **not to be reintroduced**: per-Pane
controllers/takeover, native multi-pane terminal rendering, local Ghostty
viewport scrolling, a native terminal scrollbar, ⌘F local scrollback search,
Shift+PageUp/PageDown history paging, deep-history reseed, and the
Embedded/TUI mode switch.

## 1. Principles

1. Terminal semantics beat decorative gestures.
2. Herdr owns runtime/session/layout/process state and the running `herdr`
   client owns the whole terminal UI (its sidebar, tab bar, pane layout,
   history and search included). Shardlane never reimplements those.
3. libghostty-vt owns terminal-emulator semantics (VT state, key/mouse/paste/
   focus encoding, semantic selection) for the hosted model.
4. GPUI owns native input/render plumbing; gpui-component owns standard
   desktop controls.
5. Selection, scroll-gesture residual, IME composition, hover and menus are
   client-ephemeral presentation state.
6. There is exactly one keyboard priority chain: focused native control →
   resolved Shortcut Registry → Script hotkey → hosted TUI. No second
   hand-maintained swallow list may exist.
7. Every important pointer command also has a keyboard/menu path.

## 2. Current implementation summary

Implemented and covered by automated tests/code-level validation:

- singleton hosted child/PTY lifecycle with restart cooldown, token-scoped
  polling, synchronous-kill teardown (no orphans on quit/Crepus restart);
- TUI chrome projection: the hosted process runs on a compensated raw grid;
  Shardlane paints only Herdr's authoritative `pane.layout.area` rectangle,
  and the same projection translates pointer cells and selection coordinates;
- GPUI-font-metric-derived terminal geometry shared by grid sizing, paint,
  cursor, selection, mouse, links and IME placement;
- Ghostty-grid-anchored styled-run paint with wide-cell spacer boundaries;
- printable text via AppKit NSTextInputClient / GPUI InputHandler (CJK/IME
  composition happens before any PTY byte); named keys and modifier chords
  via the Ghostty key encoder straight to the singleton PTY;
- ordered input queue (text/keys/paste/focus are ordering boundaries);
  input-surface focus, dialogs and the resolved shortcut registry keep their
  keystrokes out of the PTY (`route_shell_keystroke` pins the decision);
- bracketed paste through Ghostty; terminal focus reporting (`?1004`) only on
  window-activation edges, never on in-app navigation;
- precision wheel: pixel residual accumulation → SGR wheel when the TUI
  reports mouse; alt-screen (less/vim) translation to ArrowUp/Down via the
  local key encoder (≤6 steps) otherwise; Shift escapes translation;
- per-target (singleton) text selection with Ghostty semantic word/line
  expansion; soft-wrap-aware copy formatter; copy-on-select preference;
- ordinary right click owned by Shardlane's native menu: Copy / Paste /
  Select All only; Right press and Right drag/motion are never encoded into
  the PTY, so Herdr's own TUI menu cannot open underneath (`tui_native_
  context_menu_owns_button` regression pins this);
- navigation converges on `FocusIntent` (`shell_navigation.rs`):
  Project/Tab/Pane/Agent intents drive the protocol-20 chain
  `workspace.focus → tab.focus → pane.focus`, with `agent.focus(terminal_id)`
  direct-first and pane-chain fallback; the host process never respawns on
  navigation; sidebar highlight mirrors the hosted TUI's runtime focus;
- Herdr CLI/protocol gates: protocol < 20 renders an explicit
  "Herdr needs an update" state with Restart — never an Embedded fallback;
- OSC 8 hyperlink hit-testing and safe Cmd-click opening (allowed schemes:
  http, https, mailto, file); CJK/wide-cell preservation in frame, selection
  and link geometry; cursor style/blink rendering with user overrides.

Remaining protocol-level limitations (documented, not worked around):

1. the hosted TUI's screen buffer is Herdr's own UI surface — Shardlane does
   not search or scroll it as if it were scrollback; in-TUI history/search
   are used through the TUI itself;
2. `pane.read(source=recent)` is not exposed to the hosted path at all (it
   belonged to the deleted controller bootstrap);
3. there is no protocol method to clear Herdr-side retained scrollback.

Manual acceptance still open: real trackpad feel (slow/fast/momentum),
Chinese/Japanese IME candidates, right-click menu visuals, packaged-app run.

## 3. Focus

- clicking into the hosted surface focuses the root input bridge; PTY bytes
  flow to the hosted `herdr` client only;
- opening Settings / History / New Agent / Help / dialogs / Search transfers
  focus to the native surface and suppresses terminal keystrokes and
  FocusIn reporting; closing restores terminal focus exactly once;
- window deactivate/reactivate reports terminal FocusOut/FocusIn (once, on
  the activation edge).

## 4. Keyboard and IME

Application shortcuts (one resolved registry — `shortcuts.rs`):

- every configurable command id in the registry must resolve to a real action
  (`registry_shortcuts_all_resolve_to_runtime_actions` pins this);
- `⌘C` copy selection, `⌘V` paste, `⌘A` select all, `⌘=`/`⌘-`/`⌘0` terminal
  font size — bound in both the root and the hosted-surface (`HerdrTui`)
  contexts from the same resolved registry;
- `⌘K` search, `⌘N` new agent, `⌘R` reconnect, `⌘B` sidebar, `⌘⇧A` agents,
  pane/tab/project/workspace chords — all registry-owned;
- user overrides apply to both contexts; overridden-away or disabled chords
  fall through to the hosted TUI by design;
- Script custom hotkeys (e.g. `ctrl-alt-f8`) fire even while a composer is
  focused and never reach the PTY (`route_shell_keystroke` pins the route);
- `⌘F`/`⌘↑`/`⌘↓` are intentionally unbound by Shardlane since the cutover:
  the hosted TUI owns those semantics.

IME behavior: marked text stays local until committed; candidate positioning
follows the terminal caret cell geometry; composition is cleared when focus
moves to a modal/native surface; committed text goes to the hosted PTY as one
ordered insert; Enter/Tab/Backspace are never duplicated between the encoder
and the InputHandler (`text_is_terminal_control_payload` guards the handler).

## 5. Ordered input

Text, encoded keys, paste and focus reports share one ordered queue into the
singleton PTY. Adjacent compatible text/keys may coalesce; paste and focus
reports are ordering boundaries. Interactive input opens a ~16 ms presentation
window; passive output falls back to the bounded idle cadence. A poll loop
whose generation token went stale exits instead of waking periodically.

## 6. Paste

Paste is encoded against the hosted model's current Ghostty mode immediately
before dispatch: bracketed mode gets begin/end brackets, plain mode gets
standard newline behavior, unsafe controls are filtered per Ghostty paste
semantics, multiline content stays one transaction, and paste never races
earlier/later keys.

## 7. Selection

Selection is a single hosted-surface state (no per-target map). The pointer
surface owns the whole lifecycle; Shift is the local-escape modifier while a
TUI has mouse reporting on. Cell/word/line semantics, double-click-drag and
triple-click-drag behavior, and the 8 px content-inset contract (grid,
hit-testing, IME placement share one inset) are unchanged from the proven
implementation.

## 8. Copy semantics

Viewport selection → Ghostty grid refs → Ghostty selection → plain formatter
(unwrap soft-wrapped rows, trim padding, preserve hard breaks) → clipboard.
Reverse drags copy the same content; `⌘C` always works; copy-on-select remains
a preference.

## 9. Scrolling and wheel

- the hosted TUI owns its own scrolling; Shardlane keeps **no** local
  viewport, scrollbar, or history window;
- precision wheel/trackpad deltas accumulate into whole rows with a persistent
  pixel residual (no dead zone, no one-packet-per-event flood) and are encoded
  as SGR wheel presses when the application reports mouse. On macOS, GPUI's
  precise pixel delta receives the same 2× AppKit-side distance adjustment used
  by Ghostty before row accumulation; the residual survives successive wheel
  events and is not cleared merely because one event crossed a row;
- interaction-driven PTY output has no extra fixed 16ms presentation gate: the
  newest frame is projected immediately on wake and GPUI/display refresh owns
  paint coalescing. Non-interactive continuous output retains a 16ms budget and
  retries at the remaining deadline rather than sleeping another full tick;
- Ghostty RAW cell signatures are an exact row-change plan, not merely a whole-frame dirty bit:
  unchanged rows reuse the previous `TerminalLine`, and only changed rows perform the expensive
  text/color FFI extraction. GPUI presentation mirrors that boundary with cached `TerminalRowPane`
  entities, so ordinary repeat-key echo usually rebuilds one row rather than the full viewport;
- `surface_background` is sticky across low-confidence partial repaint frames. Once a Herdr
  background is confirmed at ≥80% visible-cell coverage, scroll/resize transition frames keep the
  previous confirmed color until another background reaches the threshold; they never encode
  "not enough evidence" as terminal-default black;
- when it does not and the model is in an alternate screen (less/vim), rows
  are translated to ArrowUp/Down through the local key encoder (≤6 steps);
- Shift is the local-intent escape and suppresses translation.

## 10. Terminal mouse reporting

Ghostty's live modes decide encoding — no per-app assumptions. Left/middle
press, drag motion, hover motion and wheel are encoded with the chrome-
compensated raw cell coordinates. Right press/drag are withheld (native menu
owns them). Hosted input is lossless: renderer lock contention waits rather
than dropping events.

## 11. OSC 8 hyperlinks

Only true OSC 8 metadata is clickable (no regex promotion). Span merging
includes wide-cell spacers; pointer hover indicates the range; Cmd-click opens
only http/https/mailto/file; normal click keeps terminal behavior. Per-cell
link lookups are skipped for models that never saw OSC 8.

## 12. Cursor

Rendering follows Ghostty state (block/bar/underline, hollow when unfocused,
blinking). Settings may force style (`Auto | Block | Bar | Underline`) and
blink (`Auto | Blink | Steady`); overrides are presentation-only and never
write VT sequences. Cursor geometry uses the shared measured cell size.

## 13. Context menus

The hosted terminal surface has exactly one right-click owner: Shardlane's
native menu. It keeps **Copy / Paste / Select All** and may mirror authoritative Herdr Pane
operations (**Rename, Move to New Tab / existing Tab, Swap directions, Split Right/Down, Toggle
Zoom, Process Info, Close**) through the same Herdr action layer used elsewhere. Right-button
traffic is never forwarded to the hosted PTY, so Herdr's own TUI menu cannot open underneath the
native menu. Tab/workspace context menus use the same action layer as keyboard/main-menu commands;
a menu item never implements a second behavior path.

## 14. Main macOS menu

Standard product actions sharing the registry-driven handlers (About,
Settings, Edit actions, terminal font actions, view/help actions).

## 15. Window and appearance

Native traffic lights; component TitleBar; semantic Shardlane chrome colors. Settings exposes
one unified theme choice, not independent App/Terminal pickers: the 17 `ThemePreset` entries each
pair a Shardlane App scheme with an exact Herdr 0.8.2 built-in theme (for example One Light →
`appearance=light`, App One family, Herdr `one-light`). Hosted Terminal colors are read from the
actual Herdr/Ghostty frame; Shardlane has no named Terminal palette and never calls a host-side
palette setter. A Shardlane-owned Herdr server is reloaded after the merged runtime config changes;
a pre-existing external/user server is never reloaded or mutated. Window opacity remains native;
Sidebar/right-panel and window resize drive one compensated PTY+model resize.

## 16. Manual acceptance scenarios

Shell/basic: plain ASCII/Unicode typing; Control chords; arrows/function keys;
Home/End/PageUp/PageDown/Delete in the hosted shell, `less`, Vim; multiline
paste; window resize during output.

IME: Chinese Pinyin; Japanese conversion; cancel/commit; open/close
Settings/Search/New Agent and return without losing or duplicating the first
keystroke.

Pointer: left-click focus; drag selection (forward/reverse, word/line,
double/triple-click-drag); right click opens exactly one native menu (clipboard + mirrored Herdr
Pane operations) and never Herdr's own TUI menu; mouse-aware TUI apps still receive
left/middle/hover/wheel; slow trackpad, fast flick, momentum — fine steps, no giant jumps and no
black/theme-background flashing during partial repaint.

Navigation: Project/Tab/Agent clicks, Global Search results, History
Continue / Focus Running Agent, New Agent success, Activity items,
notification clicks, status-bar jumps — all land in the same hosted surface
via one FocusIntent without respawning the host; host Restart recovers in
place; quit/relaunch leaves no orphan `herdr` child.

## 17. Definition of done

- automated regression suite passes;
- native runtime smoke: exactly one hosted child, no panic/projection storm,
  no orphan after quit;
- the manual scenarios above pass on macOS hardware;
- protocol limitations are documented here rather than hidden behind client
  workarounds.
