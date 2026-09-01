# AGENTS.md

## Start here

Before engineering Shardlane, read:

1. `docs/client-product-architecture.md` — canonical architecture source of truth.
2. `.agents/skills/herdr-client-development/SKILL.md` — Shardlane engineering workflow.

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

For native runtime/input/render changes, smoke the app and inspect `/tmp/shardlane-lag.log`.

## Scope and ownership

- Herdr socket/projection wrappers → `crates/herdr-gui/src/herdr.rs`.
- **Multi-instance model (2026-09-01, no-registry revision):** a workspace IS a Herdr instance (a named Herdr session); there is NO Shardlane-side workspace registry — instances are enumerated live from `herdr session list` (`shardlane_host::herdr::list_sessions`). One workspace is displayed per window; each window owns its client, runtime state, and TUI child for its bound instance (`bind_instance`/`open_or_jump_project` in `main.rs`); switching workspaces rebinds the window or jumps to the workspace's existing window. Layout persistence is Herdr's (`session.json` restores workspaces/tabs/per-Tab cwd on server restart); Shardlane persists only cosmetics + window bookkeeping: per-session display-name overrides (`settings.instance_display_names` — herdr has no session rename), the machine list (`settings.devices`, local seeded; SSH socket bridging lives in `ssh_bridge.rs`), and the open-window snapshot (`settings.open_workspaces` for launch restore). One TUI child per instance, process-wide (`TuiManagerRegistry`, shared by desktop windows and Remote/mobile viewers; unwatched children are reaped after their idle lease). Projects/workspaces inside an instance belong to Herdr: every Tab's cwd belongs to Herdr, and the right panel (Files/Lazygit) follows the focused Tab's cwd. Renaming a workspace writes only the display-name override — instances are Herdr-owned and never created/renamed/deleted behind the CLI. The legacy client-side "Workspace grouping" machinery (`workspace_management.rs` + `workspace_management/`, the `config.workspaces` registry, `project_path_overrides`/`project_path_last_seen_ms`/`workspace_dormant_project_paths`, the project auto-restore policy, the sidebar/status-bar workspace switchers, and the `workspace.N` shortcuts) was deleted on 2026-09-01; do not reintroduce it — project path resolution goes through `build_project_index` only. Still forbidden: a second Shardlane runtime implementation, the Embedded per-Pane path, a mode switch, Notes/Bookmarks/Annotation. Do not invent Herdr protocol methods; `HERDR_SESSION`/socket targeting (`bootstrap_for_session` / `connect_herdr_for` with `?instance=<session>`) is the only per-instance seam.
- `terminal_stream.rs` is the shared hosted-PTY transport for the primary Herdr TUI child and the bounded auxiliary tool child; the per-Pane controller branch remains deleted (2026-08-27 TUI-only convergence).
- libghostty terminal semantics required by the hosted TUI → `ghostty.rs`.
- Hosted terminal paint/input/IME/selection presentation currently reuses `terminal_view.rs` and related shell helpers; retain user-facing interaction quality while deleting per-Pane scrollbar/layout/scrollback ownership. Right click is owned by Shardlane's outer-native menu and is not sent to the hosted PTY; it must preserve Copy/Paste/Select All **and** the supported Herdr Pane operations (Rename, Move, Swap, Split, Zoom, Process Info, Close) through the existing Herdr API/action layer so hosting the TUI does not remove runtime capabilities.
- Native Shardlane shell/navigation → `main.rs`, `sidebar.rs`, `ui/`.
- Content-area native Tab strip (Terminal setting `terminal.tab_bar_placement`, 2026-09-01) → `shell_tabs.rs`. Presentation-only over Herdr's authoritative Tab order; it reuses the Sidebar Tab actions (Pin/Rename/Close/drag reorder) and the `FocusIntent` seam. In `native` mode the Sidebar keeps Projects without a per-Tab subtree and Project clicks focus the Project; `terminal_size`/`terminal_canvas_origin` must stay consistent with the strip's height.
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
