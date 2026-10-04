# next/ — MyGo Native UI rewrite
> L2 | Parent: ../CLAUDE.md

Members:
- `main.go` — MyGo Native UI application/window entrypoint.
- `internal/nativeui/` — native presentation split by concern: shell/titlebar/sidebar/project tree, reusable theme/icons/components, router + per-page files, runtime/watcher/dialog adapters, and visibility-scoped Terminal layout.
- `internal/herdr/` — Go adapter for real Herdr session discovery/bootstrap/socket RPC/events/direct-terminal attach; no client-side runtime authority.
- `internal/history/` — read-only catalog, transcript parsing (Claude, Codex, Pi, Oh My Pi, Antigravity) and live tailing.
- `internal/gitworkbench/` — Git status, diff review, and commit models/services.
- `internal/applog/` — bounded structured logging (`shardlane.log`, 2 MiB active + 3 backups by default).
- `resources/` — MyGo packaging resources copied from Shardlane-owned assets.
- `mygo.json` — native MyGo packaging configuration.

Rules:
- Herdr remains authoritative for instances, Projects/runtime workspaces, Tabs, Panes, layouts, Agents, terminal sessions, persistence, scrollback and process lifecycle.
- `internal/nativeui` owns presentation state only. Page navigation uses a single long-lived MyGo `ui.Router`; runtime focus/navigation still goes through Herdr. `session.snapshot.layouts` is the Pane topology authority; native UI maps those rectangles into same-window MyGo Terminal elements and never invents its own Pane layout model.
- The normal Terminal is MyGo's official native Terminal/Ghostty path running Herdr `terminal attach <terminal_id>` for visible Herdr-owned Panes. Do not add a Web terminal or client-owned PTY/runtime.
- Never add `--takeover` to automatic direct attach. Hidden Tabs retain no terminal attachment fleet. While `/workspace` remains on the same selected Tab, 0.10 may retain that Tab's current terminal attachments while Diff/Commit is the visible primary surface; hidden terminal views receive no input and no additional attachment fleet is created.
- 0.10 removes Lazygit from the Native target. `/workspace` has exactly one local primary presentation owner for Terminal / Chat / Diff Review / Commit; the Right Panel is Changes / Files / Services and does not render a competing full diff/terminal surface. Git state is client-local repository context for the selected Tab cwd and never becomes Herdr runtime authority.
- `egoist/godiff` is a UI/behavior reference only under the 0.10 audit. Do not copy/import its `internal/*` implementation; use Shardlane-owned clean-room Git Workbench models/services and independently licensed dependencies.
- Native window contracts (2026-10-06 round five): ⌘W closes the focused Tab through the shared close-tab confirm (window_actions.go) — never the window; the native window title mirrors the current route via `Shell.syncWindowTitle` (Workspace stays the bare app name).
- Use MyGo native widgets before custom drawing. Reuse `theme.go`, `icons.go`, and `components.go` before adding one-off page styling. Keep framework/platform code out of `internal/herdr` and future ordinary Go domain packages.
- History UI and Chat UI are Native UI for the current migration. WebView presentation is frozen until MyGo ships an official supported Native-UI-embeddable WebView capability and a later explicit product decision unfreezes it. Do not create private AppKit/GTK/HWND embedding or a second desktop presentation backend.
- Current Rust Host/History/Remote remain migration references/compatibility seams until their domains are ported and parity is verified.
- Application logs are structured and size-bounded. Do not log terminal output, prompt/conversation bodies, credentials, environment dumps, or provider secrets. Log operation names, stable runtime IDs, errors, state counts, and timing needed for diagnosis.
- Keep MyGo pinned exactly until an explicit framework upgrade is verified. **Active exception (F146/F147, 2026-10-06): `go.mod` carries `replace github.com/egoist/mygo => ../../mygo` — a v0.2.15 checkout plus `Options.LocalDragSelect` (scrollback panes select text and get a right-click menu while the wheel keeps reporting; alt-screen agent TUIs keep full reporting). Shardlane code uses only the public option. When upstream ships the option, delete the replace, bump the pin, delete the `../../mygo` checkout — see `../../mygo/LOCAL-PATCH.md` and the L3 header of `internal/nativeui/attach_mouse.go`.

Checks:
```sh
GOTOOLCHAIN=go1.27.1 go test ./...
GOTOOLCHAIN=go1.27.1 go tool mygo build
```

[PROTOCOL]: Update this header on structural/ownership change, then check ../CLAUDE.md and ../docs/client-product-architecture.md.
