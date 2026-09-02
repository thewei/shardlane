# Multiplexer Backend API (mux)

Status: **Approved Next — Phases 1–5 landed (2026-09-02).** The seam, the Herdr
adapter, the contract kit, the GUI/Remote migration, and the tmux MVP adapter
(`mux/tmux.rs`: instance enumeration, CLI operation mapping, the `tmux attach`
child stream, `events_push=false` degradation) are implemented and green — cargo
gates, golden fixtures unchanged, the mux kit against a real throwaway tmux
server, and isolated-runtime app smoke (bind + reconnect) with no user-session
contact. The GUI instance picker aggregates the registry (Herdr sessions +
`tmux:<instance>` rows with backend-aware bind/reconnect/attach — the attach
path opens the `tmux attach` child through `open_shared_session`).

**Outstanding — user-executed in-app acceptance.** The picker/aggregation and
the Herdr bind path are click-verified in the running app; the final tmux
row → attach click-through could not be completed by desktop automation (the
unbundled/scratch app lacks a stable bundle identity and synthetic clicks are
unreliable next to the user's live Shardlane instance), so it is handed over
as the manual checklist:

1. launch the isolated acceptance runtime (`/tmp/shardlane-mux-accept.sh`,
   isolated HOME + dedicated Herdr socket + a throwaway `demo-proj` tmux
   server; Ctrl-C tears everything down) or against your real runtimes;
2. in the instance picker click the `tmux:default` row — the hosted terminal
   must render the tmux session through the `tmux attach` child;
3. verify input echo, window resize, and the right-click menu operations
   (Rename / Move / Swap / Split / Zoom / Process Info / Close);
4. switch back to a Herdr instance and confirm it is unaffected. Ownership truth
remains `client-product-architecture.md`; this document is the normative contract for
the backend-neutral runtime seam that the macOS shell, the Remote API, and mobile
clients will share once it lands.

**Computer-Use acceptance update (2026-09-02).** The isolated Computer-Use run
(`mux-acceptance.sh --prepare`) now packages the current dev binary into a
proper `.app` bundle and rejects stale same-bundle-id instances before launch;
the isolated bind, the hosted `tmux attach` surface, and the chrome projection
are click-verified against the throwaway `demo-proj` server. Two acceptance-
blocking defects were found and fixed in that run: the
`SHARDLANE_TMUX_SOCKET` isolated-testing seam was ignored by the Default bind
and instance probe (the acceptance app silently attached to the user's real
tmux server), and the focused-pane projection picked the first window's pane
(`pane_active` is window-scoped), whose never-resized geometry fed the GUI
chrome-compensation loop and grew the hosted grid without bound. Still
outstanding from the original checklist: real-keyboard input echo / right-click
pane operations through the attach surface (synthetic Computer-Use typing does
not reach the GPUI terminal surface; drive it with the native driver or by
hand), and switching back to a Herdr instance in the same window.

**Native-driver regression update (2026-09-02, afternoon).** With explicit
`native` authorization the full harness ran to completion and exposed three
more findings. Fixed: (a) the chrome-compensation anti-feedback guard needed a
precise stale signature (unchanged pane area across probes), not a two-edge
heuristic that could reject legitimate Remote resizes; (b) the startup landing
surface was the New Agent page for every backend, but a backend without agent
capability cannot act on it — the bind completion now lands directly on the
hosted work surface when `capabilities().agents` is false (the `Multiplexer
Connection` trait gained `capabilities()` for this). Still outstanding (P1):
keyboard input does not reach the `tmux attach` child PTY in the attached
work surface — System Events keystrokes, raw CGEvent keycodes, and paste all
fail to produce pane bytes while the Herdr TUI path is unaffected; the focus
seam between the terminal surface and the attach stream writer is the suspect.
Also outstanding (P2, intermittent): one observed silent app exit after
clicking the bound Project's sidebar row (no panic, no crash report; 1 in 3
reproductions).

**UX pass + harness correction (2026-09-02, evening).** The native harness's
input/resize/menu phases were re-audited and three harness defects corrected:
the capture/assertion target must be the attach client's CURRENT window (the
first window of a multi-window session is never resized by the client), the
front() activation needed retry-until-frontmost with a hard fail (activation
is silently ignored while the user works - input sent from a non-frontmost
app lands elsewhere), and the menu phase now drives the app's native menu
bar via AX presses (Split Right / Toggle Pane Zoom / Close Pane) instead of
synthetic right-clicks with OCR hitboxes - which doubles as the menu
accessibility regression. With those fixes, resize and split pass; the input
echo, zoom, and close remain flaky while the user's own foreground competes
for activation. Product UX fixes from the same pass: navigation now exits
the New Agent overlay (exit_blocked_secondary_surfaces previously only
cleared Settings/Help, so clicking a sidebar project while the New Agent
page was open looked like a dead button), and the workspace switcher panel
tags non-Herdr rows (default with a tmux suffix) so same-named instances on
different backends are distinguishable.

**App silent-exit update (F6 revised).** The "click-caused exit" attribution
was wrong: several of the silent exits were caused by the test harnesses'
own cleanup paths killing the app, and the rest reproduce without any click
at all (the app dies within its first minute, SIGKILL-class — the Herdr
server log shows no graceful client disconnect, no panic output, no crash
report). The death window is random (one instance survived 20 s under
monitoring while siblings died unprompted). Until this is root-caused, any
GUI regression on this machine must treat app-lifetime as untrusted: pin the
app pid, heartbeat it, and re-check liveness before asserting any negative
(input not received / surface not rendered) result.

## 1. Current facts and approved direction

**Current.** `crates/shardlane-host/src/herdr.rs` defines `HerdrClient`, the only
runtime-boundary implementation: a sync socket-RPC client (~60 public methods) plus
session/TUI/CLI helper functions. The macOS shell holds it concretely
(`ShardlaneApp.client: Option<HerdrClient>`, roughly 110 call sites across the shell
impl modules); `shardlane-remote` constructs it per request (`connect_herdr_for`) behind
the `?instance=<session>` seam. What GUI and Remote actually consume is narrower than
the RPC list: the `HerdrState`/navigation/tab-surface snapshots, the `HerdrEvent` stream
(global + pane-scoped), structural Workspace/Tab/Pane operations, one per-instance
hosted-TUI VT byte stream (`HerdrTuiSession`: subscribe / send_bytes / resize / stop),
and pane history reads.

**Approved Next.** Introduce a backend-neutral **Multiplexer API** in a new
`shardlane-host` `mux` module. Herdr is adapter #1, wrapping `HerdrClient` unchanged.
**tmux is adapter #2 and is in scope for this effort as a capability-degraded MVP**
(terminal multiplexing only; see §10, Phase 5). The macOS shell and every Remote route
migrate to consume the seam. The Herdr wire contract and the Remote v2 API shapes do
not change.

**Why a formal seam at all (M1 lesson).** The M1 convergence deliberately deleted
speculative service traits. This abstraction is justified only because a concrete
second backend is intended; therefore: every trait method must have at least one real
consumer on the day it lands, the first implementation is the existing concrete client
(no parallel production adapter), and the adapter contract kit (§9) pins the seam so a
future backend cannot silently drift it. Test-only scripted/recording adapters are
`#[cfg(test)]` infrastructure, not production implementations.

## 2. Goals and non-goals

Goals:

- one neutral API covering **every UI-visible runtime capability** (catalog, §6);
- Herdr adapter #1 with zero behavior change during the migration (P1–P4);
- tmux adapter #2 (MVP) proving the seam against the shared contract kit;
- macOS GUI and Remote/mobile clients consume the same API; Remote v2 wire shapes and
  golden fixtures unchanged;
- TDD verification: contract kit + characterization tests written before the code they
  pin, staying green throughout the refactor.

Non-goals:

- plugin systems, dynamic backend discovery, provider/marketplace surfaces;
- generalizing the Host Agent-semantic services (Conversation/Launch/Transfer) — they
  stay Herdr-typed and capability-gated at the entry points;
- any Herdr protocol change, invented protocol method, or wire DTO change;
- ACP (remains frozen per the canonical architecture document).

## 3. Naming rule

Backend integrations are **Backend adapters**, not "Providers": in this codebase
"Provider" already means a coding-agent vendor (`provider_bridges`, `ProviderBridgeAdapter`,
provider product capabilities). Herdr technical names stay in Herdr-named code
(`herdr.rs` is not renamed); the `mux` layer uses the neutral product-domain terms
Workspace / Tab / Pane, which map cleanly to both backends:

| mux concept | Herdr | tmux |
|---|---|---|
| Instance | Herdr session (`HERDR_SESSION`/socket) | tmux server socket |
| Workspace | runtime workspace | tmux session |
| Tab | tab | tmux window |
| Pane | pane | tmux pane |

## 4. Completeness principle

**Every object and operation visible in the UI must have a catalog entry (§6); every
client drives the UI only through the catalog.** Client-local presentation state
(domain 10) is explicitly excluded — that exclusion is a decision, not an omission.

Enforcement:

1. this catalog is normative: a UI capability without a catalog entry fails review;
2. code-level closure: after P2 (Remote) and P3 (GUI), concrete `HerdrClient` /
   `shardlane_host::herdr::*` references are allowed only in a reviewed whitelist
   (adapter file, Herdr-protocol escape hatches);
3. wire preservation: golden fixtures pass with zero updates through P2–P4;
   `UPDATE_FIXTURES=1` must never be needed for the migration.

## 5. Module layout (approved target)

```text
crates/shardlane-host/src/mux/
├── mod.rs        # neutral contract: traits, MuxCapabilities, InstanceRef, ID newtypes
├── registry.rs   # MuxRegistry: backend id → Arc<dyn Multiplexer>; sole assembly point
├── kit.rs        # adapter contract test suite (§9); every backend must pass it
├── herdr.rs      # Herdr adapter: trait impl blocks + thin mapping; the existing
│                 #   herdr.rs concrete client stays in place, unrenamed
└── tmux.rs       # (Phase 5) tmux adapter
```

Rules:

- adapters are static compile-time modules; adding a backend = one adapter file +
  one registry line; no trait-on-trait, no dynamic loading;
- backend-specific knowledge (socket shapes, CLI invocations, protocol version gates,
  env sanitizing, child-process spawn commands, wire/format parsing, ID mapping) lives
  only inside the adapter file;
- the registry is the only place that names backends for assembly; consumers hold
  `Arc<dyn MultiplexerConnection>` handles and capability data. Upper layers never
  branch on backend identity (`backend == herdr` style) — all capability differences
  flow through `MuxCapabilities` and `Option` accessors;
- escape hatch: `MultiplexerConnection::as_herdr() -> Option<&HerdrClient>` exists for
  Herdr-protocol-self concerns only (protocol version gate, CLI version display, TUI
  protocol gate). It is whitelist-reviewed.

## 6. API catalog

Classification: **A** = core, backend-neutral; **B** = capability-gated domain;
**C** = Host product service built on the mux (not part of the mux traits); **D** =
client-local presentation state (never in the API). "Herdr base" names the existing
call the Herdr adapter delegates to. The mux API names below are the approved contract
shape; exact Rust signatures are fixed in Phase 1.

### Domain 1 — Instances / Workspaces (A)

| mux API | Herdr base | Consumers |
|---|---|---|
| `list_instances()` (id, display name, status) | `list_sessions` + `read_session_display_name` | GUI instance picker / sidebar; mobile instances screen |
| `rename_instance(display_name)` | `write_session_display_name` (store stays Shardlane-owned) | GUI rename; `/api/v2/instances/{id}/rename` |
| `stop_instance()` / `delete_instance()` | `stop_session` / `delete_session` | GUI; tmux maps to kill-session / kill-server |
| `open_instance(InstanceRef) -> Arc<dyn MultiplexerConnection>` | `bootstrap_for_session` / `connect_to` (SSH bridge) / `connect` | window bind; `/api/v2/instances/{id}/bootstrap` |

### Domain 2 — Projection snapshots (A)

`navigation_state`, workspace/visible state, `host_bootstrap_state` (→ ProjectIndex),
`tab_surface_state`, `workspace_panes`, `pane_layout`, `agents()` (list + status for
Sidebar/Activity/mobile bootstrap). The Herdr adapter returns the existing projection
types; their naming is neutralized at the mux boundary without changing shape.

### Domain 3 — Events (A)

`subscribe_events()` (global) and `subscribe_pane_events()` (pane-scoped). The event
classification helpers (`refreshes_*`, `affected_workspace_id`, `updated_layout`, …)
move with the neutral event type into the mux layer. tmux MVP has no push stream:
`events_push=false` degrades consumers to bounded snapshot polling plus explicit
refresh (see §7).

### Domain 4 — Tabs (A)

`create_tab(CreateTab{cwd, index, focus})` (collapses the five current `create_tab*`
variants), `rename_tab`, `move_tab` (capability `cross_workspace_tab_move` — already a
Herdr protocol gap), `close_tab`, `tab_focus`.

### Domain 5 — Panes (A)

`split(direction)`, `close_pane`, `swap_pane`, `move_pane` (to Tab / new Tab),
`set_split_ratio`, `resize_pane` (pixels), `toggle_pane_zoom`, `pane_focus`,
`rename_pane`, `send_text`, `send_keys`, `pane_process_info`, `read_pane_history`
(Herdr base `read_pane_recent_ansi`; capability `pane_history_read`; tmux maps to
`capture-pane`). The hosted-TUI right-click menu actions (Rename, Move, Swap, Split,
Zoom, Process Info, Close) all resolve to this domain — hosting must not remove them.

### Domain 6 — Terminal byte stream (A + capability `shared_tui`)

`open_shared_session`, stream subscription (with startup replay), input bytes
(semantic-key→byte encoding stays client-side, `tui_input`), `resize`, `stop`/reap.
Herdr implementation = the existing hosted `herdr` TUI child
(`HerdrTuiSession`/`TuiManagerRegistry`). tmux MVP implementation = `tmux attach`
spawned on a PTY — the same hosted-TUI-child shape, so the GUI input path (Ghostty
encoders → PTY bytes) and the Remote TUI routes (`/api/v2/tui/*`) work unchanged.

### Domain 7 — Agents (B, capability `agents`)

List/status arrive via Domain 2 snapshots. Mutations reuse the existing `AgentRuntime`
trait (start/prompt/read/wait/send_keys), absorbing `report_pane_agent`,
`clear_pane_agent_authority`, and `agent_focus`, exposed through
`MultiplexerConnection::agent_runtime() -> Option<&dyn AgentRuntime>`. New Agent
launch transactions, Chat, follow-up queue, interactions, Context Transfer, and Live
Handoff are Domain 9 services and stay Herdr-typed.

### Domain 8 — Server administration (B, capability `server_admin`)

Protocol version report, CLI presence/version (upgrade banners), user config path,
`reload_config`, theme editing (Settings → Herdr config). Exposed via
`Multiplexer::server_admin() -> Option<&dyn MultiplexerServerAdmin>`; a tmux backend
returns `None` and the corresponding Settings cards degrade (hidden).

### Domain 9 — Host product services (C; not mux traits)

Scripts/Services (launch/observe/control/discovery over Domain 5 primitives),
Conversations (prompt/queue/continue/delegate), History catalog/search/paging
(backend-independent by construction — reads provider files, not the mux), ProjectIndex
bootstrap. These already live UI-independent in `shardlane-host` and are already
exposed by Remote; this effort does not generalize them. Entry points reject
non-Herdr backends via capability flags.

### Domain 10 — Client-local presentation state (D; never in the API)

Shardlane Workspace grouping/color/order and settings cosmetics, pinned sidebar tab
IDs, sidebar width/collapse, language/theme preferences, `FocusIntent` local
navigation, windows/shortcuts, right-click menu composition, Files panel local file
operations, the Lazygit auxiliary PTY, the web preview, and the device list / SSH
bridges (connection infrastructure that feeds `InstanceRef` targets; mobile clients
always connect through Host HTTP, never SSH directly).

## 7. Capability model

`MuxCapabilities` fields, each with a named degradation consumer — no field may exist
without one:

| Field | Herdr | tmux MVP | Degradation consumer |
|---|---|---|---|
| `agents` | true | false | Agent sections/New Agent/Chat entry points hidden; History (read-only) remains |
| `server_admin` | true | false | Herdr config/theme Settings cards hidden |
| `shared_tui` | true | true | `/api/v2/hello` `herdr_tui` flag and GUI TUI surface |
| `pane_history_read` | true | true | pane output routes / deep-history affordances |
| `cross_workspace_tab_move` | false (protocol gap) | false | drag-reorder restricted to within-Workspace |
| `events_push` | true | false | event-driven projection updates degrade to bounded polling + explicit refresh |

## 8. Registry (assembly point)

`MuxRegistry` is built once at startup (from settings; Phase 1 registers only Herdr)
and held by `ShellSharedRuntime` (GUI) and `RemoteState` (Remote). `?instance=<session>`
semantics are unchanged; the registry resolves the backend dimension. Backends
advertise `MuxCapabilities` at registration; `/api/v2/hello` is driven from it.

## 9. Testing and verification strategy (TDD)

### 9.1 Principles

- **New contract code (Phase 1 kit, Phase 5 tmux): classic red→green.** The contract
  kit tests are written against the trait definitions first; adapter implementation
  turns them green.
- **Migration code (Phases 2–4): characterization-first.** Behavior-pinning tests are
  written/enabled *before* each change and must stay green throughout; the refactor
  proceeds only under green. Passing fixtures-with-zero-updates is the wire
  characterization.
- Test doubles (scripted/recording adapters) live in `#[cfg(test)]` only.

### 9.2 Test assets

1. **`mux/kit.rs` — adapter contract suite** (run per backend; the kit itself is
   validated in Phase 1 against the Herdr adapter over the existing in-crate
   test-socket harness, `HerdrClient::for_test_socket` + fake server in
   `herdr.rs` tests):
   - instance lifecycle: `list_instances` → `rename_instance` → list reflects the new
     display name; `open_instance` of an unknown ref is a typed error, not a panic;
   - snapshot invariants: every Tab references an existing Workspace, every Pane an
     existing Tab; snapshot rebuild after reconnect is referentially consistent;
   - structural round-trips: `create_tab` → `rename_tab` → `move_tab` → `close_tab`
     leaves the projection consistent using only authoritative return payloads;
   - pane round-trip: `split` → `send_text` → `close_pane`;
   - `read_pane_history` returns bounded output (≤ requested lines) when the
     capability is declared;
   - event contract: subscription returns a live receiver and classification helpers
     respond correctly to each synthetic event kind the backend emits;
   - capability coherence: invoking a gated domain without the capability returns a
     typed Unsupported error — never a panic, never a silent no-op;
   - stream contract: subscribe (with startup replay) → ordered bytes → `send_bytes`
     echo → `resize` → `stop`/reap; the receiver is bounded and preserves byte order;
   - concurrency: a connection handle is usable from two threads concurrently
     (Send + Sync);
   - registry contract: `open_instance` returns the backend the registry routed to.
2. **Golden fixtures** (`UPDATE_FIXTURES=1 cargo test -p shardlane-remote --test
   golden_fixtures` is the regeneration path — NOT to be run during P2–P4): the
   unchanged-fixture run is the wire-preservation proof for every Remote v2 route.
3. **Mobile Zod mirror** (`herdr-mobile/src/contracts/host.ts`): untouched; `pnpm
   test` is re-run in that repo only if fixtures ever change (they must not).
4. **GUI seam tests (P3)**: a `#[cfg(test)]` `RecordingConnection` records trait calls;
   unit tests pin FocusIntent → trait-call mapping, pane action runners (the current
   method-pointer helpers become closures over the trait), and the window bind path.
5. **tmux adapter tests (P5)**: a throwaway server on a dedicated socket
   (`tmux -S <tmp>/mux-kit.sock -f /dev/null new-session -d`) — never the user's
   default server, mirroring the Herdr smoke-isolation rule (dedicated socket +
   isolated state root). Kit runs against `TmuxBackend`; the stream contract runs
   against a `tmux attach` child on a `portable-pty` fake PTY (byte echo, resize via
   TIOCSWINSZ).

### 9.3 Behavior-preservation invariants ("current features unchanged" after P1–P4)

| User-visible behavior | Verified by |
|---|---|
| Sidebar Project/Tab/Pane projection and navigation | existing GUI tests + kit snapshots + manual smoke |
| Tab strip ops (create/rename/move/close/drag) | GUI seam unit tests + manual |
| Pane ops incl. right-click menu (Rename/Move/Swap/Split/Zoom/Process Info/Close) | GUI seam unit tests + manual checklist |
| Terminal input/IME/paste/mouse/focus quality | `crepus dev` smoke + `/tmp/shardlane-lag.log` inspection; keyrepeat/scroll A/B scripts not required (byte path untouched) unless smoke shows an anomaly |
| Reconnect (kill server → overlay → restart → recovery) | existing tests + manual |
| Instance list / rename / open / SSH-bridge probe | existing tests + manual |
| Scripts / Services lifecycle | existing tests + one manual script run |
| New Agent / Chat / History flows | existing workspace tests + manual |
| Remote/mobile: bootstrap, conversations, TUI session open/input/resize, events WS, instances | golden fixtures (zero updates) + `tui_tests` + manual web regression |
| Settings: Herdr theme edit + `reload_config` | manual |

### 9.4 Gates per phase

Standard gate every phase:

```sh
cargo fmt -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
cargo build --locked --workspace
git diff --check
git diff --cached --check
```

Phase 2 adds the unchanged-fixture run; Phases 3–4 add `crepus dev --bin shardlane`
smoke + lag-log inspection; Phase 5 adds the tmux kit run and a manual tmux-instance
smoke. Work on uncommitted trees is forbidden: the currently open shell changes are
committed or stashed before Phase 1 starts.

## 10. Implementation sequencing (Approved Next; each phase independently mergeable)

| Phase | Content | Merge criteria | Effort |
|---|---|---|---|
| P1 | `mux/{mod,registry,kit,herdr}.rs`: traits + full catalog + `HerdrClient`/`HerdrBackend`/`HerdrTuiSession` impls; kit red→green; zero consumer migration | gates green; no GUI/Remote behavior change | 2–3 d |
| P2 | `shardlane-remote` onto the seam (`connect_herdr_for` → registry, handlers over `&dyn MultiplexerConnection`, protocol gate via escape hatch; `hello` capabilities from registry) | gates + fixtures unchanged | 1–2 d |
| P3a | GUI navigation/render: `ShardlaneApp.client` → `Arc<dyn MultiplexerConnection>`, bind sites via registry, `shell_navigation`/`shell_render`/`sidebar` | gates + smoke + lag log | 1–2 d |
| P3b | GUI structural ops: `shell_panes`/`rename`/`shell_tabs`/`shell_input` | gates + smoke | 1–2 d |
| P3c | GUI long tail: `scripts/*`, `new_agent/*`, `shell_projects`, `ssh_bridge`, `herdr_tui`, `chat/*`, `history/resume`, `settings_view`; closure grep (whitelist only) | gates + smoke + right-click menu checklist | 1–2 d |
| P4 | Stream seam: `TuiManagerRegistry` behind `MultiplexerStream`; `herdr_tui` broadcast driven by capabilities | gates + terminal smoke | 1–2 d |
| P5 | **tmux MVP adapter** (`mux/tmux.rs`): Domains 1–6 minus push events (`events_push=false`); `tmux attach`-child stream; `events` degradation path; registry + settings surfacing for tmux instances; **out of MVP**: agents, server_admin, Scripts/Services (capability-degraded), control-mode event stream (future) | tmux kit green; manual smoke: open a tmux instance in the app (Projects from sessions, Tabs from windows, attach/input/resize, split/close/rename/zoom, right-click menu, Files panel follows cwd) with Herdr instances unaffected | 3–5 d |

Total: roughly 2.5–3 weeks single-engineer, ~25–30 files touched plus the new mux
module. Preconditions: current uncommitted shell work lands first; tmux 3.6a is
present on the dev machine (other machines need tmux installed).

## 11. Risks and the fragile assumption

- **Fragile assumption**: tmux's value here is terminal multiplexing itself (Domains
  1–6); Agent semantics (Domains 7/9) remain Herdr-bound. If the intent were full
  Agent/Chat parity on tmux, the capability-gated model degrades half the product and
  the seam must be re-levelled before P3. P1/P2 are unaffected.
- The largest risk is mechanical-churn regression across ~110 GUI call sites;
  mitigated by the P3 sub-phase split, characterization tests, and per-phase smoke.
- Documentation truth: when P5 lands a second runtime, the canonical "Herdr is the
  sole runtime authority" wording is amended then — not by this approval. P1 also
  updates the ownership-router lines in `AGENTS.md` and the development skill
  (Herdr wrappers now live in `shardlane-host`; `mux` owns the neutral seam).
