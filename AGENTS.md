# AGENTS.md

## Start here

Before engineering Shardlane, read:

1. `docs/client-product-architecture.md` — canonical architecture source of truth.
2. `.agents/skills/herdr-client-development/SKILL.md` — Shardlane engineering workflow.
3. For real-app UI acceptance, `.agents/skills/ui-acceptance-testing/SKILL.md` and
   `docs/ui-acceptance-testing.md` — Computer Use MCP, isolation, and evidence contract.

Internal iteration plans, audits, handoffs, and progress evidence are kept in a private engineering archive outside this repository; public documentation under `docs/` is self-contained.

## Product/runtime boundary

- **Shardlane is the macOS client/product.**
- **Herdr is the backend/runtime authority** for workspaces, tabs, panes, layouts, terminal sessions, agents, scrollback, persistence, and process lifecycle.
- External Agent conversation history is a separate read-only catalog; it must not become a second runtime.
- Runtime continuation from history must go through Herdr, not an external terminal launcher.
- Use mature ownership before custom code: **Herdr API → vendored libghostty-vt → gpui-component 0.5.1 → GPUI 0.2.2 → proven compatible implementation → custom**.
- Do not invent Herdr protocol methods or socket shapes. Inspect the live/actual API.
- The vendored Ghostty binary is the ABI authority; verify new FFI bindings against it.

## Build

- Use Cargo for Rust work.
- Use `crepus dev --bin shardlane` for local app smoke tests.
- Use `wax`, not `brew`.

## Checks

Run before calling a capability complete:

```sh
cargo fmt -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
cargo build --locked --workspace
git diff --check
git diff --cached --check
```

When packaging changes, also build and structurally verify `Shardlane.app`.

For real-app UI acceptance, read `docs/ui-acceptance-testing.md` and use
`.agents/skills/ui-acceptance-testing/SKILL.md`. Computer Use is the default
driver through the host Computer Use MCP (node_repl + Sky), with an isolated
manifest and Herdr/tmux ground-truth assertions. Any native CGEvent/System Events
fallback must set `SHARDLANE_UI_DRIVER=native SHARDLANE_ALLOW_GLOBAL_INPUT=1` explicitly;
it can move the user's pointer and steal focus. Native screenshots/video also
require `SHARDLANE_ALLOW_GLOBAL_CAPTURE=1`. Run `scripts/verify.sh ui` for the
no-GUI harness preflight before starting an acceptance session. Run
`scripts/acceptance-capabilities.py check --json` to inventory the local pieces;
run its `mcp-snippet` output through `mcp__node_repl__js` to resolve the host
Computer Use connector. Do not add a project-local fake MCP server.

For native runtime/input/render changes, smoke the app and inspect `/tmp/shardlane-lag.log`.

## Scope and ownership

- Herdr socket/projection wrappers → `crates/shardlane-host/src/herdr.rs` (`crates/herdr-gui/src/herdr.rs` is a re-export shim). Backend-neutral runtime seam → `crates/shardlane-host/src/mux/` (docs/multiplexer-api.md): the macOS shell and the Remote API consume instances only through `mux::MuxRegistry` + the `Multiplexer*` traits; `HerdrClient` concrete references above the adapter are restricted to the as_herdr() whitelist (Domain 7/9 services, protocol gate).
- **Integration acceptance order (2026-09-02):** Herdr-behavior parity first, then GUI completeness, then new-backend integration (tmux). An acceptance pass over a new mux adapter does not substitute for regression coverage of the Herdr path, and a new backend must never delay or mask a Herdr/GUI defect.
- **Multi-instance model (2026-09-01, no-registry revision):** a workspace IS a Herdr instance (a named Herdr session); there is NO Shardlane-side workspace registry — instances are enumerated live from `herdr session list` (`shardlane_host::herdr::list_sessions`). One workspace is displayed per window; each window owns its client, runtime state, and TUI child for its bound instance (`bind_instance`/`open_or_jump_project` in `main.rs`); switching workspaces rebinds the window or jumps to the workspace's existing window. Layout persistence is Herdr's (`session.json` restores workspaces/tabs/per-Tab cwd on server restart); Shardlane persists only cosmetics + window bookkeeping: per-session display-name overrides (`settings.instance_display_names` — herdr has no session rename), the machine list (`settings.devices`, local seeded; SSH socket bridging lives in `ssh_bridge.rs`), and the open-window snapshot (`settings.open_workspaces` for launch restore). One TUI child per instance, process-wide (`TuiManagerRegistry`, shared by desktop windows and Remote/mobile viewers; unwatched children are reaped after their idle lease). Projects/workspaces inside an instance belong to Herdr: every Tab's cwd belongs to Herdr, and the right panel (Files/Lazygit) follows the focused Tab's cwd. Renaming a workspace writes only the display-name override — instances are Herdr-owned and never created/renamed/deleted behind the CLI. The legacy client-side "Workspace grouping" machinery (`workspace_management.rs` + `workspace_management/`, the `config.workspaces` registry, `project_path_overrides`/`project_path_last_seen_ms`/`workspace_dormant_project_paths`, the project auto-restore policy, the sidebar/status-bar workspace switchers, and the `workspace.N` shortcuts) was deleted on 2026-09-01; do not reintroduce it — project path resolution goes through `build_project_index` only. Still forbidden: a second Shardlane runtime implementation, the Embedded per-Pane path, a mode switch, Notes/Bookmarks/Annotation. Do not invent Herdr protocol methods; `HERDR_SESSION`/socket targeting (`bootstrap_for_session` / `connect_herdr_for` with `?instance=<session>`) is the only per-instance seam.
- `terminal_stream.rs` is the shared hosted-PTY transport for the primary Herdr TUI child and the bounded auxiliary tool child; the per-Pane controller branch remains deleted (2026-08-27 TUI-only convergence).
- libghostty terminal semantics required by the hosted TUI → `ghostty.rs`.
- Hosted terminal paint/input/IME/selection presentation currently reuses `terminal_view.rs` and related shell helpers; retain user-facing interaction quality while deleting per-Pane scrollbar/layout/scrollback ownership. Right click is owned by the Herdr TUI itself (2026-09-18): every button, Right included, is encoded straight to the hosted PTY and the TUI's own context menu is the only right-click menu — do not register a native terminal menu on the hosted surface and do not withhold Right press/motion. Clipboard/Pane operations stay available through keyboard shortcuts and the Sidebar Pane rows via the existing Herdr API/action layer.
- Native Shardlane shell/navigation → `main.rs`, `sidebar.rs`, `ui/`.
- Tab presentation (revised 2026-09-19, second pass): the canonical Sidebar renders the per-Project Tab subtree plus an explicit "新增 Tab" row (creation through Herdr `tab.create` via `create_tab_in_workspace`), and the Header breadcrumb's Tab layer carries a hover-revealed "…" menu whose Rename/Close items call the same `open_tab_rename`/`close_tab_by_id` channels as the Sidebar rows — no second action implementation. The hosted Herdr TUI's own chrome — its Tab bar included, in single- AND multi-Tab windows — is never shown: Shardlane runs the client-side chrome-crop projection again (`herdr_tui::TuiChromeProjection` + `apply_tui_chrome_area`: `pane.layout.area` probes measure the Pane rectangle, the Ghostty model keeps the RAW TUI grid, and painted frames/mouse SGR coordinates/selections/semantic copy/PTY resize are all compensated through that projection; the 60ms probe after attach/focus is crop-only or a guarded re-impose, never a poll). Belt-and-suspenders: startup still normalizes Herdr `ui.hide_tab_bar_when_single_tab` to `true` (one-shot `migrations.tui_tab_bar_hidden`) so a single-Tab window needs no crop at all. Herdr 0.9.1 has no always-hide key — the crop is the sanctioned product answer, and the upstream `ui.hide_tab_bar` key remains the day it lands. The `terminal.tab_bar_placement` setting and the native content-area Tab strip (`shell_tabs.rs`) stay deleted; do not reintroduce a second Tab presentation owner.
- Settings persistence → `settings.rs`.
- Native-shell i18n (2026-09-01) → `i18n.rs` + `crates/herdr-gui/locales/` on the `rust-i18n` backend gpui-component already uses; the `i18n!` embedding stays at the main.rs crate root, one process-global locale is driven by `config.ui.language` (`settings::Language`, lenient deserialization) and switched from the Settings → Appearance Language card. Strings migrate to `i18n::t()` surface by surface (Appearance page first); new UI strings use `t()`. Do not add a second translation path, a runtime translation loader, or translate Herdr-owned runtime/TUI strings.
- App-shell theme/chrome tokens → `theme.rs`. Hosted Terminal colors have **one** authority: the official Herdr theme configured in the user's Herdr `config.toml` (`theme.*`, edited by Settings). As the hosted terminal emulator, Shardlane seeds and reports only colors derived from that same theme — OSC 10/11 dynamic defaults written into its Ghostty model, OSC 10/11 query answers sent to the hosted TUI child, and presentation `UiTheme.terminal` (2026-08-29: the Herdr TUI paints its surface from these answers; without them it falls back to a dark built-in palette regardless of the configured theme). Do not reintroduce a Shardlane-named Terminal palette, a `follow-app` Terminal theme, or a second palette selection path. Remaining presentation colors derive from the actual libghostty-vt `TerminalFrame` emitted by Herdr.
- Agent history formats/catalog/search semantics → the `shardlane-history` crate.
- Host Agent CLI discovery/validation → `agent_cli.rs`.
- Product Agent/Conversation semantics (M1–M9 convergence, 2026-08-30): Agent launch (`run_agent_launch`), Conversation query/prompt (`HostConversationService`), the subscribed live-session owner (`ConversationSessionManager`), safe Working follow-up queue (`ConversationFollowUpQueue`), lossless Context Transfer (`run_context_transfer` + `TransferArtifactStore`), History continuation planning (`plan_history_continuation`), Live Handoff (`run_live_handoff`), provider product capabilities, and session insight all live in `shardlane-host`. GUI/Remote consume them; normal semantic operations never type PTY text. Raw terminal input remains explicitly Terminal-only.
- Do not add a second Sidebar implementation, plugin UI, marketplace, cloud accounts, or telemetry. The right-panel local web preview (WKWebView, localhost/dev-server scope only) is the single sanctioned exception and must not grow into a general browser surface.

## Git safety

- Preserve staged/user changes.
- Do not `git add`, reset, commit, push, or otherwise rewrite user Git state unless explicitly asked.
