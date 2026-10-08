# AGENTS.md

## Start here

Before engineering Shardlane, read:

1. `docs/client-product-architecture.md` — canonical architecture source of truth.
2. `docs/mygo-native-execution-rules.md` — active migration execution contract.
3. `docs/mygo-native-migration-roadmap.md` — active task roadmap and milestone order.
4. `.agents/skills/herdr-client-development/SKILL.md` — Shardlane engineering workflow.

Architecture ownership lives only in `docs/client-product-architecture.md`; ad-hoc audits/progress evidence that are not part of the active migration contract may remain outside the repository.

## Product/runtime boundary

- **Shardlane is the macOS client/product.**
- **Herdr is the backend/runtime authority** for workspaces, tabs, panes, layouts, terminal sessions, agents, scrollback, persistence, and process lifecycle.
- External Agent conversation history is a separate read-only catalog; it must not become a second runtime. Runtime continuation from history must go through Herdr.
- Use mature ownership before custom code: **Herdr API → MyGo native components/official plugins → proven compatible implementation → custom**.
- Do not invent Herdr protocol methods or socket shapes. Inspect the live/actual API.

## Build and checks

```sh
GOTOOLCHAIN=auto go tool mygo dev         # run the app
GOTOOLCHAIN=go1.27.1 go test ./...
GOTOOLCHAIN=go1.27.1 go tool mygo build   # builds build/darwin-arm64/Shardlane.app
git diff --check
git diff --cached --check
```

- `go.mod` pins Go 1.27.1 and MyGo v0.2.15; use `GOTOOLCHAIN=auto` so the repository pin does not require changing the user's global Go installation.
- `go.mod` carries a temporary, user-approved `replace github.com/egoist/mygo => ./third_party/mygo` (F146/F147 `Options.LocalDragSelect` patch vendored in-repo). Do not remove the replace or add a second framework source; bump the pin and delete `third_party/mygo` only once upstream ships the option.
- When packaging changes, also build and structurally verify `Shardlane.app`.

## Scope and ownership

- `internal/nativeui` owns presentation only: shell/titlebar/sidebar/project tree, theme/icons/components, one long-lived MyGo `ui.Router` (persistent Sidebar/Titlebar sit outside the router), per-page files, runtime/watcher/dialog adapters, and visibility-scoped Terminal layout.
- `internal/herdr` owns the Go Herdr CLI/socket boundary: session discovery/bootstrap, RPC, events, direct terminal attach. Keep framework/platform code out of it and out of ordinary domain packages.
- `internal/history` is the read-only history catalog/transcript/live-tailing domain; `internal/agent` owns Agent product semantics; `internal/gitworkbench` owns the local Git review model; `internal/applog` owns bounded structured logging.
- Herdr `session.snapshot.layouts` remains the Pane topology authority; native UI only maps visible Pane rectangles to official MyGo Terminal elements running supported `herdr terminal attach <terminal_id>`. No client-owned PTY/layout/runtime may appear.
- Automatic attach must never use `--takeover`; hidden Tabs keep no attachment fleet.
- History UI and Chat UI are Native UI. WebView presentation is frozen until MyGo ships an official Native-UI-embeddable WebView capability and a later explicit product decision unfreezes it — no private AppKit embedding, no second desktop presentation backend, no localhost HTTP backend for desktop presentation.
- 0.10 target: Lazygit is removed. `/workspace` has one local `WorkspacePrimarySurface` owner for Terminal / Diff Review / Commit; the Right Panel is Changes / Files / Services and must not render a competing full diff/terminal surface. Godiff is a UI/behavior reference only; do not copy/import its `internal/*` implementation.
- Use MyGo native widgets before custom drawing; reuse `theme.go`, `icons.go`, and `components.go` before adding one-off page styling.
- Native window contracts: ⌘W closes the focused Tab through the shared close-tab confirm — never the window; the native window title mirrors the current route via `Shell.syncWindowTitle` (Workspace stays the bare app name).
- Logs are structured and size-bounded under MyGo `PathLogs` (2 MiB active + 3 backups by default). Never log terminal bytes/output, prompt or conversation bodies, credentials, auth headers, provider secrets, or environment dumps; log operation names, stable runtime IDs, errors, state counts, and timing.
- Keep MyGo pinned exactly until an explicit framework upgrade is verified.
- Still forbidden: a second Shardlane runtime implementation, a mode switch, Notes/Bookmarks/Annotation, plugin UI/marketplace, cloud accounts, telemetry.

## Git safety

- Preserve staged/user changes.
- Do not `git add`, reset, commit, push, or otherwise rewrite user Git state unless explicitly asked.
