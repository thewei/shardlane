# Shardlane Next (MyGo Native UI migration)

`next/` is the approved rewrite of the existing Shardlane client. It remains the same product and keeps Herdr as the sole runtime authority.

## Target shape

```text
MyGo Native UI
├─ native Header / Sidebar / Breadcrumbs / Toolbar
├─ native Terminal views for only the visible Herdr Panes
└─ optional future rich-content Web windows/surfaces for Chat/History
             │
             ▼
        Go application/domain services
             │
             ▼
           Herdr
```

The main shell is pure Go/MyGo Native UI: no React, Vite, Bun, DOM layout, or WebView is required to launch the desktop window. Herdr `session.snapshot.layouts` remains the Pane topology authority. Shardlane maps the focused Tab's authoritative Pane rectangles into same-window MyGo `terminal.View` elements, each attached through Herdr's supported `terminal attach <terminal_id>` client. Hidden Tabs keep no terminal attachment.

History View, History Detail and Chat View are Native UI for the current migration. Verified against MyGo 0.2.6 as well as the pinned 0.2.5: Native UI windows still have no official embedded WebView element. WebView presentation is therefore frozen rather than implemented as a separate window or private AppKit embedding. This can be reconsidered only after MyGo ships an official Native-UI-embeddable WebView capability and a later explicit product decision unfreezes it.

## Development

```sh
cd next
GOTOOLCHAIN=auto go tool mygo dev
```

The module pins Go 1.27.1 and MyGo (see `next/go.mod` for the authoritative pin, currently v0.2.15). `GOTOOLCHAIN=auto` can fetch the project toolchain without replacing the globally installed Go.

## Verification and packaging

```sh
cd next
GOTOOLCHAIN=go1.27.1 go test ./...
GOTOOLCHAIN=go1.27.1 go tool mygo build
```

The migration build deliberately opens the trial window centered on the primary display instead of restoring prior WebView-era window positions. State restoration will return after the Native UI shell stabilizes.

Current macOS outputs:

```text
build/darwin-arm64/Shardlane.app
build/darwin-arm64/Shardlane 0.2.0.dmg
```


## Native design system

The Native UI presentation is intentionally componentized rather than page-styled ad hoc:

```text
internal/nativeui/
├─ theme.go              macOS/shadcn-like neutral tokens
├─ icons.go              reusable outline SVG icon set
├─ components.go         iconButton/navButton/panelCard/treeRow
├─ titlebar.go
├─ sidebar.go
├─ project_tree.go       Project → Tab → Pane hierarchy + connector guides
├─ router.go
├─ page_*.go             one page/domain per file
├─ workspace.go
├─ terminal.go
├─ runtime.go
├─ watcher.go
└─ dialogs.go
```

On macOS the root uses MyGo vibrancy behind the sidebar/titlebar, while content surfaces remain opaque for terminal and text clarity. Project rows use native-painted hierarchy guides and folder/tab/terminal icons instead of margin-only indentation.

## Diagnostics and bounded logs

Structured JSON logs are written to MyGo `PathLogs` (macOS: `~/Library/Logs/Shardlane/shardlane.log`). The active file is capped at 2 MiB with three backups, so normal disk use is bounded to about 8 MiB. `SHARDLANE_LOG_LEVEL=debug` enables diagnostic detail; `info` is the default. Settings → Runtime shows the active path and can reveal it in Finder. Terminal output, prompt/conversation bodies, credentials and provider secrets are intentionally not logged.

## Native navigation

The main Native UI shell uses MyGo `ui.Router` rather than one monolithic page state. The persistent Sidebar/Titlebar sit outside the router; page state and keyboard focus are retained in router history.

```text
/workspace
/new-task
/search
/history
/history/{id}
/settings/{section...}
```

`/search` is already functional against the authoritative current Herdr projection (Projects, Tabs, Panes and live Agents). Selecting a result routes back to `/workspace` and focuses the corresponding Herdr target. `Cmd+K`, `Cmd+,` and `Cmd+Shift+N` match the current Shardlane defaults. Terminal attachments are visibility-scoped: leaving `/workspace` closes the MyGo presentation attachments, and returning recreates only the visible Herdr Pane attachments.

## Migration boundary

Established now:
- MyGo 0.2.15 native main window and hidden native title bar;
- native Sidebar matching the shipped shell hierarchy: New Task/Search/History actions, Agents, Projects → Tabs → Panes, plus footer Workspace switcher + Settings;
- native Workspace create/rename/delete dialogs and Project/Tab/Pane context menus;
- trial-window lifecycle hardened for Native UI: explicit native `OnShow` placement on the primary display instead of the old WebView `OnReadyToShow` path;
- native Agents section, Breadcrumbs, Toolbar and status/error presentation;
- Herdr instance/session discovery and protocol-22+ socket gate for the audited 0.9.3 baseline;
- Project/Tab/Pane/Agent projection from `session.snapshot`;
- Herdr event subscription with authoritative snapshot reconciliation;
- Project/Tab/Pane focus plus basic Tab/Pane mutations through verified Herdr RPCs;
- same-window MyGo native Terminal views attached to visible Herdr Pane terminal IDs;
- authoritative Herdr multi-Pane layout mapped into native element geometry;
- app/DMG packaging through MyGo with no frontend build toolchain.

Still intentionally on the migration path:
- full current Sidebar actions/polish, drag/reorder, menus and shortcut parity;
- 0.10 Git Workbench closure: one `/workspace` Terminal / Diff Review / Commit primary surface, plus Right Panel Changes / Files / Services; Lazygit is removed from the Native target;
- semantic Chat/Conversation services and final Chat presentation choice;
- read-only History catalog/search/cache and final History Detail presentation choice;
- Remote/Mobile Host API v2 and its golden contracts;
- Windows named-pipe Herdr transport and platform acceptance;
- final removal of the Rust GUI/Host/History compatibility implementation after parity.

### 0.10 workspace-surface target

The approved 0.10 Native target is documented in `../docs/mygo-native-0.10.0-git-workbench-primary-surface-plan.md`. Godiff is a UI/behavior reference only; Shardlane does not copy/import Godiff `internal/*` implementation. The global Router continues to own top-level pages, while `/workspace` has one local primary surface owner for Terminal, Diff Review, or Commit.

### Terminal ownership

MyGo owns terminal emulation/presentation through its official Terminal plugin and Ghostty library. Herdr owns PTYs, processes, Pane topology, layout, terminal IDs, persistence and runtime state. Shardlane owns only the visibility-scoped presentation attachments and product navigation. Automatic direct attach never uses `--takeover`.
