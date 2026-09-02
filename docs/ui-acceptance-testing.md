# UI Acceptance Testing (Multiplexer / Hosted Surfaces)

Status: **Current** — engineering practice for driving the real Shardlane app
through scripted UI interactions with ground-truth assertions.

## 1. When to use this

The no-GUI capability inventory/unit tests and the mux contract kit
(`shardlane-host/src/mux/kit.rs`) prove the seams. Some acceptance questions
only the real app can answer: does the picker list a backend's instances, does
the hosted terminal render a bound instance, does a synthetic window resize
follow through to the runtime grid? This doc
explains the tooling and the rules for writing those cases quickly — without
re-deriving the same clicks, env hooks, and pitfalls every time.

## 1.1 Driver policy (Computer Use first)

There are two UI drivers and their risk boundaries are intentionally different:

| Driver | Use | Input scope | Required evidence |
| --- | --- | --- | --- |
| **Computer Use (default)** | ordinary UI navigation, AX inspection, menu/form actions | app-targeted `mcp__node_repl__js` + `@oai/sky`; no repository-generated global events | `driver=computer-use` in `evidence.jsonl` plus Herdr/tmux truth |
| **Native fallback** | real key-repeat/trackpad timing or renderer samples Sky cannot express | global CGEvent/System Events; can move the pointer and steal focus | explicit `SHARDLANE_UI_DRIVER=native SHARDLANE_ALLOW_GLOBAL_INPUT=1`; add capture authorization only for shared-display video/screenshots |

The public Sky API does not expose a queryable “virtual cursor/virtual display” flag.
Treat it as an app-scoped action channel, not as proof of headless display isolation.
If Sky cannot identify the target App or falls back to global input, stop the run and
use a backend assertion or request explicit native authorization.

The repository does not ship a second Computer Use server. Any Agent running in
Codex/ChatGPT should call the host-provided `mcp__node_repl__js` MCP tool and
import `@oai/sky`; an Agent without that connector must stop or stay on backend
assertions rather than inventing a shell substitute. The host-side Computer Use
capability must be enabled/available before UI actions; some Codex builds expose
it through the enabled `node_repl` service rather than a separately named MCP
entry, so the read-only probe below is the source of truth.

## 1.2 Agent connection contract and self-check

Computer Use is a host capability, not a project-local daemon. In the Codex
desktop app, install/enable the **Computer Use** plugin from Plugins, turn on
the Computer Use server and skill toggles, and grant macOS Screen Recording and
Accessibility permissions when prompted. See the [official Computer Use setup
guide](https://learn.chatgpt.com/docs/computer-use). The repository itself needs
no `.mcp.json`, API key, socket, or `codex` executable. An Agent host declares
the following contract in its tool manifest/system prompt, then runs the
read-only probe:

```yaml
computer_use:
  transport: mcp__node_repl__js
  runtime: persistent node_repl
  package: "@oai/sky"
  target: mac
  probe: scripts/acceptance-capabilities.py mcp-snippet
```

The YAML is a portable declaration of the required host tool, not a universal
MCP server configuration. Codex's enabled Computer Use connector satisfies it;
an Agent on another host must provide an equivalent `mcp__node_repl__js` tool or
report the capability as unavailable. A shell process cannot grant that
connector or emulate its App-targeted boundary.

`codex mcp list` (or `/mcp` in the Codex TUI) is useful for diagnosing the host,
but its internal server names/statuses are implementation details. The
acceptance source of truth is the callable `mcp__node_repl__js` plus the
read-only `@oai/sky` probe below; if that probe already passes, do not edit
internal `node_repl`/Computer Use commands by hand.

If the host also needs a *custom* MCP server, configure it in the host's
`~/.codex/config.toml` (or a trusted project `.codex/config.toml`) under
`[mcp_servers.<name>]`, or use `codex mcp add/list`; the desktop app, Codex CLI,
and IDE extension share that host configuration. Do not invent a
`[mcp_servers.computer-use]` command for the bundled plugin or try to launch
`@oai/sky` as a standalone server. See the [official MCP configuration
guide](https://learn.chatgpt.com/docs/extend/mcp).

Print the exact probe from the repository:

```sh
scripts/acceptance-capabilities.py mcp-snippet
```

Execute the printed JavaScript through `mcp__node_repl__js` in the Agent host.
It imports `@oai/sky`, checks `target` and every required function, and writes
only a JSON result; it does not launch an app, move the pointer, type, click,
scroll, or read a screenshot. A healthy result has `target: "mac"` and an empty
`missing` array. If the tool is absent, the import fails, or an export is
missing, stop at backend assertions and record `computer-use:mcp` as
`unknown`/`unavailable`.

The local half of the inventory is independent and safe to run from CI or any
Agent:

```sh
scripts/acceptance-capabilities.py check --driver computer-use --json
```

Each item is `available`, `blocked`, `unknown`, or `unavailable`; `required: false`
marks a capability outside the selected driver, and `safe: false` marks an
explicitly enabled global-input/capture path. The report's `overall` value only
considers required capabilities. After the Agent has
collected the MCP export names, merge them into the report with repeated
`--mcp-export <name>` options. The default command exits non-zero only for an
unavailable local capability; `--strict` also treats unknown/blocked entries as
failure and is intended for a fully resolved host manifest.

## 1.3 Composable capability chain

Keep a scenario as a small chain whose layers can be reused independently:

| Layer | Public seam | Side effect |
| --- | --- | --- |
| Capability inventory | `scripts/acceptance-capabilities.py check` / `scripts.acceptance.capabilities` | none |
| Driver boundary | `scripts/ui-driver-guard.sh` | only native opt-in changes policy |
| Isolated runtime | `scripts/mux-acceptance.sh --prepare` | owns disposable Herdr/tmux/app PIDs |
| App action | host `mcp__node_repl__js` + `@oai/sky` | App-targeted UI action |
| Backend truth | `scripts.acceptance.assertions` | none; consumes captured output |
| Evidence | `scripts/acceptance-evidence.py` | append-only control-plane JSONL |

For a new scenario, select only the needed checks and assertions rather than
copying the full mux harness. The Python seam is intentionally dependency-free:

```python
from scripts.acceptance import (
    AssertionCheck,
    contains_marker,
    run_assertions,
    zoomed_flag_is,
)

suite = run_assertions((
    AssertionCheck("marker", lambda: contains_marker(capture, marker)),
    AssertionCheck("zoom", lambda: zoomed_flag_is(zoom_flag)),
))
if not suite.passed:
    raise SystemExit(suite.as_dict())
```

`capture`, `marker`, and `zoom_flag` come from an explicit Herdr/tmux query in
the caller; the helpers never invoke tmux or touch the UI. Add one evidence
event per passed behavior, then create the harness `PASS` marker only after the
suite and the required phase summary pass. This preserves the same setup →
act → backend assert → evidence order for picker, terminal, menu, or future
surfaces without coupling them to `mux-acceptance.sh` internals.

## 2. The three primitives

### 2.1 Automation seam: `SHARDLANE_BIND_INSTANCE`

Startup binds the first window to an explicit instance key with **zero UI
interaction**:

```sh
SHARDLANE_BIND_INSTANCE=tmux:default target/debug/shardlane   # tmux backend
SHARDLANE_BIND_INSTANCE=default target/debug/shardlane        # Herdr session
```

Keys follow the picker jump-key convention: a Herdr session name, or
`tmux:<instance>`. It takes precedence over the open-workspaces restore
snapshot (`main.rs`, startup restore block). This is the single biggest
time-saver: everything after bind is assertable without clicking.

### 2.2 Native event injection (explicit fallback): `scripts/keyrepeat-evpost.swift`

This tool posts global CGEvents and is never the default driver. The shell and
the compiled binary both require an explicit opt-in:

```sh
swiftc -O -o target/debug/evpost scripts/keyrepeat-evpost.swift
```

```sh
SHARDLANE_UI_DRIVER=native SHARDLANE_ALLOW_GLOBAL_INPUT=1 target/debug/evpost click  <x_pt> <y_pt>       # left click (move → down → up)
SHARDLANE_UI_DRIVER=native SHARDLANE_ALLOW_GLOBAL_INPUT=1 target/debug/evpost rclick <x_pt> <y_pt>       # right click (context menus)
SHARDLANE_UI_DRIVER=native SHARDLANE_ALLOW_GLOBAL_INPUT=1 target/debug/evpost key    <keycode> <count> <interval_us>
target/debug/evpost bounds <pid>               # "x y w h" of the app's main window
```

Rendered text is located with PID-scoped Vision OCR (`scripts/vision-ocr.swift`):

```sh
swiftc -O -o target/debug/vocr scripts/vision-ocr.swift
target/debug/vocr --pid <app_pid> <x_pt> <y_pt> <w_pt> <h_pt>
# output: text<TAB>x,y,w,h (global points)
```

Only the PID-scoped form is allowed by default. An unscoped `vocr` call reads
the shared display and requires `SHARDLANE_ALLOW_GLOBAL_CAPTURE=1`.

The returned bounding box makes menu interaction deterministic without a hardcoded
row pitch: right-click, OCR the target window, find the row whose text contains the
wanted item, then click the box center (see `menu_click` in
`scripts/mux-acceptance.sh`). OCR output is sorted top-to-bottom/left-to-right.

Native fallback text entry uses System Events keystrokes (`osascript -e 'tell application
"System Events" to keystroke "..."'`, Return is key code 36) after a click
has focused the target control — GPUI ignores unfocused synthetic keys.

### 2.3 Ground truth — never assert on pixels alone

Every UI action must be paired with a backend-side observable:

| UI action | Ground truth |
|---|---|
| hosted terminal renders + input works | `tmux capture-pane -t <sess>:<win> -p` contains the typed marker |
| window resize follows through | `tmux display-message -p -t <pane> '#{pane_width}'` changes |
| Split / Close | `tmux list-panes -t <win> \| wc -l` |
| Zoom | `#{window_zoomed_flag}` |
| Rename pane | `#{pane_title}` |
| Projects projected | lag log `bind.ok ws=N tabs=N panes=N` |
| picker aggregation | lag log `mux.instances total=…` (one line per refresh) |

The lag log (`SHARDLANE_LAG_LOG_PATH`) carries `bind.ok` / `bind.err` /
`bind.connected` / `mux.instances` diagnostics emitted by the bind flow —
read it before touching the UI at all; most bind failures are diagnosable
without a single click.

## 3. Isolation discipline (mandatory)

A UI smoke is only isolated when **socket routing and persisted state** are
both isolated — a unique `HERDR_SOCKET_PATH` **and** a temporary `HOME` for
the app, the Herdr server, and every CLI probe
(`scripts/terminal-native-smoke.sh` is the reference). The mux harness also
uses a dedicated `SHARDLANE_TMUX_SOCKET`; it never talks to the user's default
tmux server. Always tear down only the PIDs and paths recorded in its
`manifest.env` (`tmux kill-server`, kill the Herdr server, then remove the
runtime directory). Keep the runtime with `--keep` on failure.

Computer Use runs through an App-targeted MCP channel. The Agent must target
the `APP_PATH`/`APP_PID` from the manifest, re-read `sky.get_app_state` after
each action, and stop if the service reports an ambiguous target or a global
input fallback. This is an isolation boundary, not a claim that the public API
proves a virtual display or cursor. It keeps harness actions out of unrelated
apps and the user's pointer/keyboard path; it cannot stop the user from
manually changing the same target app, so do not touch that one test window.

Native fallback is different: CGEvent/System Events go to the frontmost app,
move the user's pointer, and can consume user keystrokes. It is allowed only
with `SHARDLANE_UI_DRIVER=native SHARDLANE_ALLOW_GLOBAL_INPUT=1`, in one
blocking process with no concurrent user interaction. Shared-display
screenshots/video additionally require `SHARDLANE_ALLOW_GLOBAL_CAPTURE=1`.
Re-assert frontmost and
wait for `evpost bounds <pid>` before every phase; OCR of an occluded region
reads whichever window covers it, which can masquerade as an app bug.

## 4. The harness: `scripts/mux-acceptance.sh`

One command runs the full tmux-instance acceptance against an isolated
runtime and prints PASS/FAIL per phase:

```sh
# No-GUI preflight (safe to run in CI or while user works).
scripts/verify.sh ui

# Default/safe path: prepare an isolated app for an Agent's Computer Use MCP.
scripts/mux-acceptance.sh --driver computer-use --prepare --keep

# Explicit real-device fallback: may move the user's pointer and steal focus.
SHARDLANE_UI_DRIVER=native SHARDLANE_ALLOW_GLOBAL_INPUT=1 \
  scripts/mux-acceptance.sh --driver native
```

Without `--prepare`, Computer Use mode exits before launching anything; this
prevents a shell-only Agent from silently falling back to global events. The
prepare mode runs isolated runtime → bind, then prints a manifest and waits
for the Agent to drive the app (bounded by `SHARDLANE_ACCEPTANCE_TIMEOUT_SEC`,
default 900 seconds). The Agent writes a `PASS`/`FAIL` marker when done; the
shell records cleanup and summarizes `evidence.jsonl`.

The summary requires a `pass` event for `runtime`, `bind`, `surface`, `input`,
`resize`, `menu`, and `switch`; a PASS marker without those events is a failed
acceptance.

Phases: isolated runtime → bind → hosted-terminal input echo (asserted via
`capture-pane`) → window resize (asserted via pane geometry) → right-click pane
operations Split / Zoom / Close (asserted via pane count / zoom flag) → switch
back to Herdr (asserted via TUI child + tmux state untouched).

Native context-menu items are located by PID-scoped OCR at run time
(`menu_click <label>` in the script). OCR returns each row's real bounding box,
so the click uses the box center and has no menu-top or row-pitch calibration
constants. Computer Use should prefer AX `element_index` and use OCR only as a
diagnostic fallback.

## 5. Pitfalls learned (do not re-learn them)

1. **Same bundle id, two processes** — the dev binary and any packaged copy
   both identify as `dev.shardlane.app`; the second process's window does not
   come on screen. Kill the old instance before launching.
2. **fmt drifts your edit anchors** — when patching with string replacement,
   run the replacement *after* `cargo fmt`, or match on short stable tokens;
   verify the edit landed (`grep`) before building.
3. **Jump keys must be unique per instance** — picker rows map 1:1 to
   `open_or_jump_project` keys; a Herdr session and a tmux instance that share
   a name silently route to Herdr (first match). That is why tmux rows use
   `tmux:<instance>` keys end to end (label, bound check, bind).
4. **Instance keys are not session names** — `tmux:default` is an instance;
   the adapter resolves the attach session (`open_shared_session(None, …)`).
   Passing the instance key as a tmux session name attaches to a session that
   does not exist.
5. **Capability-gated failures are terminal** — `events_push=false` backends
   must not enter retry loops (`subscribe_pane_events` returning
   `Unsupported("events_push")` is permanent; only transient errors schedule
   retries).
6. **Idle herdr CLI round trips in enumeration** — `list_instances` runs
   backend CLIs; keep it off the render path (the picker reads the cached
   `ShellSharedRuntime.instances`, refreshed explicitly).
7. **Global input is never implicit** — `evpost`, the pacing/scroll harnesses,
   and the native mux path fail closed unless both native-driver environment
   variables are present. Do not remove the guard; use Computer Use or a
   backend assertion first.

## 6. Writing a new acceptance case

1. Start from `scripts/mux-acceptance.sh`: copy the phase skeleton (setup →
   bind → act → assert → next).
2. Prefer a new `SHARDLANE_*` env seam over clicks for anything that is setup
   rather than the behavior under test (precedent: `SHARDLANE_LAG_LOG_PATH`,
   `SHARDLANE_BIND_INSTANCE`).
3. Assert on the backend-side observable (§2.3); use screenshots only to
   debug failures, never as the assertion.
4. Bound every wait with a poll loop and a hard iteration cap; leave the
   runtime dir behind on failure (`--keep` semantics) for post-mortem.
5. For Computer Use, publish the manifest, re-read AX state after every action,
   and write one backend assertion/evidence event per UI behavior before the
   `PASS` marker. Never copy terminal text or user input into the ledger.
6. Run the full gate suite afterwards (`AGENTS.md` §Checks) — UI harness
   changes still ship through the same gates.
