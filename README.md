# Shardlane

Shardlane is a native macOS workspace for coding agents, built with Go and the MyGo Native UI framework (official MyGo Terminal/Ghostty surface). The previous Rust/GPUI implementation lives only on the `rewrite/mygo` and `main` branches.

Shardlane is the **client/product**. **Herdr remains the backend runtime** and owns workspaces, tabs, panes, terminal sessions, agents, persistence, layout state, and process lifecycle. Shardlane projects that runtime into a native macOS interface; it does not replace or duplicate Herdr.

## Requirements

- macOS 13+
- Go 1.27.1 — the module pins it; `GOTOOLCHAIN=auto` lets Go fetch that toolchain without changing your global install
- a MyGo framework checkout named `mygo` sibling to this repository: `go.mod` temporarily carries `replace github.com/egoist/mygo => ../mygo` for the approved F146/F147 `Options.LocalDragSelect` patch until upstream ships the option (see `../mygo/LOCAL-PATCH.md`)
- `herdr` — if it is missing, Shardlane offers `wax install herdr`

## Development

```sh
GOTOOLCHAIN=auto go tool mygo dev
```

## Checks

```sh
GOTOOLCHAIN=go1.27.1 go test ./...
GOTOOLCHAIN=go1.27.1 go tool mygo build
git diff --check
```

## Packaging

`go tool mygo build` produces the macOS app and DMG with no frontend build toolchain:

```text
build/darwin-arm64/Shardlane.app
build/darwin-arm64/Shardlane <version>.dmg
```

Packaging identity is configured once in `mygo.json` (app name, bundle identifier, icon, minimum macOS version).

## Native design system

Presentation is componentized rather than page-styled ad hoc: `internal/nativeui` provides `theme.go` (macOS/shadcn-like neutral tokens), `icons.go` (reusable outline SVG icon set), and `components.go` (iconButton/navButton/panelCard/treeRow) plus the titlebar/sidebar/project-tree/router/page files. On macOS the root uses MyGo vibrancy behind the sidebar/titlebar while content surfaces stay opaque for terminal and text clarity. Reuse these before adding one-off page styling.

## Diagnostics and bounded logs

Structured JSON logs are written to MyGo `PathLogs` (macOS: `~/Library/Logs/Shardlane/shardlane.log`). The active file is capped at 2 MiB with three backups. `SHARDLANE_LOG_LEVEL=debug` enables diagnostic detail. Terminal output, prompt/conversation bodies, credentials and provider secrets are intentionally not logged.

## Architecture

Ownership is intentionally narrow (full contract: [`docs/client-product-architecture.md`](docs/client-product-architecture.md)):

1. **Herdr** — runtime/backend authority; `session.snapshot.layouts` is the Pane topology authority.
2. **MyGo Native UI** — presentation framework and the official Terminal/Ghostty surface.
3. **Go application/domain packages** — Shardlane product semantics: navigation, projection, read-only history, Git workbench, settings, logging.

Terminal attachments are visibility-scoped Herdr `terminal attach <terminal_id>` clients mapped into same-window MyGo Terminal elements. Automatic attach never uses `--takeover`; hidden tabs keep no attachment fleet.

```text
main.go              — MyGo Native UI application/window entrypoint
internal/
├─ nativeui/         — MyGo-native desktop presentation (shell, router, pages, theme/icons/components)
├─ herdr/            — Herdr daemon protocol bindings: discovery, socket RPC, events, direct terminal attach
├─ agent/            — UI-independent Agent product semantics
├─ app/              — application service coordination
├─ settings/         — client preferences
├─ history/          — read-only coding-agent history (catalog, transcripts, live tailing)
├─ gitworkbench/     — local Git review model (status, diff review, commit)
├─ conversation/     — Conversation/Chat domain
├─ filesview/        — Right Panel Files tool
├─ preview/          — local web preview
├─ services/         — resident service monitoring
├─ commandcenter/    — fast command entry point
├─ scripts/          — user-defined project scripts
├─ codehl/           — syntax highlighting via Chroma lexers
├─ diagnostics/      — diagnostics & log export
├─ applog/           — bounded structured logging
└─ platform/         — platform integration
```

## Scope

- native macOS client (MyGo Native UI; WebView presentation is frozen until MyGo ships an officially embeddable WebView capability and a product decision unfreezes it)
- Herdr socket/runtime integration
- native Terminal rendering through MyGo's official Terminal/Ghostty
- workspace/tab/pane navigation, New Task / Search / History / Chat / Settings
- Git Workbench (Diff Review / Commit) scoped to the focused Tab cwd
- read-only coding-agent history browsing
- no plugin marketplace · no cloud account layer · no telemetry

## Contributing

Development setup, the verification gates every change must pass, and review expectations live in [CONTRIBUTING.md](CONTRIBUTING.md). Bug reports and feature requests use the issue templates.

## Security

Report vulnerabilities privately through GitHub's vulnerability reporting — see [SECURITY.md](SECURITY.md). Do not open public issues for anything exploitable.

## License

Shardlane is free software licensed under the [GNU General Public License v3.0](LICENSE): you may use, study, modify, and redistribute it; derivative works must likewise be licensed under GPLv3. Embedded third-party material keeps its own license — see [`THIRD_PARTY_NOTICES.md`](THIRD_PARTY_NOTICES.md).
