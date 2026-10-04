# Shardlane MyGo 0.9.0 — Workspace Intelligence & Desktop Experience Plan

Status: **planned large post-0.8 release**
Target: `0.9.0`
Depends on: 0.8 History Management, Usage & Agent Inspector complete
Architecture authority: `docs/client-product-architecture.md`
Execution rules: `docs/mygo-native-execution-rules.md`
Reference audit: `docs/reference-magpie-herdr-gpui-shardlane-0.9-audit.md`

## 1. Version goal

0.9 is the first **large desktop-workflow release** of the MyGo rewrite.

Goal:

> Turn Shardlane from a client that can operate Agents into the daily development workspace around those Agents: find anything instantly, understand the active Project, inspect files/services/Git, open local previews, react through native desktop affordances, diagnose problems, and update the app without leaving the product.

This is a large version by **scope and user experience**, but it is intentionally still `0.9.0`, not `1.0`.

`1.0` remains the final cutover milestone where the Rust desktop client is no longer required for ordinary product use.

## 2. Product pillars

0.9 has eight product pillars:

```text
1. Command Center
2. Project Tooling / Right Panel
3. Services & Port Intelligence
4. Git Workspace Intelligence
5. Desktop Attention & Terminal Ergonomics
6. Diagnostics & Logs
7. Updates & What's New
8. Appearance & Task Presets
```

They share one principle:

> Every surface consumes the same existing Project/Agent/History/runtime identities. No feature creates a parallel workspace, process, Agent, or terminal authority.

## 3. User-visible target

A typical 0.9 workflow should look like:

```text
Cmd-Shift-P
→ type "portal"
→ jump to Project / Agent / Service / action

Project selected
→ Sidebar shows branch/ahead-behind summary
→ right-side tools offer Files / Services / Lazygit
→ :3000 service chip opens Preview

Agent needs input
→ system notification + Dock badge + menu-bar Status Center
→ click reaches exact Agent/Interaction

Something is broken
→ Diagnostics
→ filter logs / copy diagnostics / export sanitized bundle

Update available
→ release notes
→ install through official MyGo updater
→ relaunch
```

## 4. Scope classification

### 4.1 Release-blocking core

0.9 is not complete without:

- unified Command Center;
- Native project-scoped Right Panel framework;
- Files surface;
- Script/Services migration;
- listening-port intelligence;
- Lazygit surface;
- local Git status/ahead-behind/diff projection;
- local Preview workflow using a supported MyGo path;
- Dock badge/attention closure;
- local file drop to Terminal;
- Diagnostics/Logs surface;
- official MyGo Native updater integration;
- Sidebar density + high contrast;
- New Task Presets;
- package/real-app acceptance.

### 4.2 Capability-gated enhancement

These are planned and implemented only when their authority/API is verified:

- GitHub PR readiness through an optional authenticated adapter;
- semantic Terminal Find through Herdr `pane.copy_search` or equivalent;
- Herdr worktree actions beyond already verified APIs;
- embedded Right Panel WebView if MyGo ships official Native-UI embedding before implementation.

A missing optional capability must produce an explicit unavailable state, not block the release.

### 4.3 Deferred beyond 0.9

- mandatory model gateway;
- provider credential/model-profile mutation;
- WebDAV/S3 sync;
- general browser tabs;
- tabs in titlebar;
- remote file copy;
- SSH Teleport/device management expansion;
- custom audio stack;
- private launch-at-login integrations;
- full keyboard whole-scrollback Copy Mode.

## 5. Reference authorities

### Original Shardlane / Rust behavior

Use these as product-parity references:

```text
crates/herdr-gui/src/right_panel/*
crates/herdr-gui/src/scripts/*
crates/herdr-gui/src/git_status.rs
crates/herdr-gui/src/search_model.rs
crates/herdr-gui/src/search_view.rs
crates/herdr-gui/src/notifications.rs
crates/herdr-gui/src/update_check.rs
crates/herdr-gui/src/update_install.rs
crates/herdr-gui/src/browser_profile.rs
```

Port the behavior/domain contract, not GPUI mechanics.

### herdr-gpui reference

Use for proven UX patterns around:

- command palette;
- listening-port visibility;
- Git/PR readiness;
- desktop notifications/badges;
- file drops;
- logs/diagnostics;
- updater/release notes;
- high contrast/density;
- safe background work/stale target handling.

### Magpie reference

Use for:

- fast picker/profile ergonomics;
- task preset interaction inspiration;
- clean distinction between usage/configuration/health;
- menu-bar-first compact workflows;
- update/release-note polish.

Do not import Magpie's gateway ownership into Shardlane core.

## 6. Architecture

```text
                           MyGo Native UI
        ┌──────────────────────┼─────────────────────────┐
        ▼                      ▼                         ▼
 Command Center          Workspace Tools             Diagnostics
        │            ┌───────┼──────────┐                │
        │            ▼       ▼          ▼                │
        │          Files  Services   Lazygit              │
        │                    │                             │
        │                    ▼                             │
        │               PreviewService                    │
        │                    │                             │
        └────────────┬───────┴──────────────┬──────────────┘
                     ▼                      ▼
             Go application/domain       Desktop adapters
                     │                notification/dock/update
          ┌──────────┼───────────┐
          ▼          ▼           ▼
       Herdr      local Git    local filesystem
       runtime    read facts   read-only Files
```

Herdr remains authoritative for:

```text
Projects/runtime workspaces
Tabs
Panes
terminal sessions
Agent state
process lifecycle
Script execution panes/processes
worktree lifecycle when exposed by Herdr
```

Shardlane owns:

```text
Command catalog/presentation
Right Panel presentation state
read-only Files projection
Script semantic definitions while Herdr lacks Script resource
local Git fact cache
Task Presets
Diagnostics presentation/export
app update preferences
appearance preferences
```

## 7. Proposed Go package boundaries

Avoid one large `nativeui` file.

Suggested structure:

```text
next/internal/
├─ commandcenter/
│  ├─ model.go
│  ├─ catalog.go
│  ├─ ranking.go
│  └─ actions.go
├─ filesview/
│  ├─ tree.go
│  ├─ preview.go
│  └─ watcher.go
├─ scripts/
│  ├─ model.go
│  ├─ store.go
│  ├─ service.go
│  ├─ monitor.go
│  └─ ports.go
├─ gitintel/
│  ├─ status.go
│  ├─ service.go
│  └─ github.go
├─ diagnostics/
│  ├─ snapshot.go
│  ├─ logreader.go
│  └─ export.go
├─ presets/
│  ├─ model.go
│  └─ store.go
└─ desktop/
   ├─ attention.go
   ├─ filedrops.go
   └─ updates.go

next/internal/nativeui/
├─ command_center.go
├─ right_panel.go
├─ right_panel_files.go
├─ right_panel_services.go
├─ right_panel_lazygit.go
├─ diagnostics_page.go
├─ appearance_settings.go
└─ task_presets_ui.go
```

Names may be adjusted to existing package conventions, but ownership must remain similarly narrow.

## 8. Command Center domain

### 8.1 Distinguish from Search

Existing Search remains for content/history search.

Command Center answers:

```text
Where can I go?
What can I do?
```

Search answers:

```text
Where does this text/conversation occur?
```

Do not collapse both into one giant service.

### 8.2 Model

```go
type ItemKind string

const (
    KindProject ItemKind = "project"
    KindTab     ItemKind = "tab"
    KindPane    ItemKind = "pane"
    KindAgent   ItemKind = "agent"
    KindHistory ItemKind = "history"
    KindService ItemKind = "service"
    KindAction  ItemKind = "action"
    KindSetting ItemKind = "setting"
)

type Item struct {
    ID       string
    Kind     ItemKind
    Title    string
    Subtitle string
    Keywords []string
    Icon     string
    Action   Action
    RankHint int
}
```

No MyGo element types in the domain model.

### 8.3 Catalog sources

Catalog is assembled from already-derived snapshots:

```text
ProjectIndex
AgentDirectory
History recent metadata
Script/Service snapshot
Action registry
Settings section registry
```

No catalog source performs blocking work when the panel opens.

### 8.4 Ranking

Deterministic order:

```text
exact title/id
→ title word-prefix
→ keyword word-prefix
→ substring
→ fuzzy
→ RankHint
→ stable title/id fallback
```

Write pure tests with typo/Unicode/mixed-case fixtures.

### 8.5 Filters

```text
All
Navigation
Agents
Projects
Commands
History
```

Tab/Shift-Tab may cycle filters if Native input behavior remains predictable.

### 8.6 Navigation safety

Selecting an item carries a typed target.

Before executing stateful navigation:

```text
revalidate target identity
→ dismiss palette
→ route locally
```

Never find a target again by display title alone.

## 9. Command Center UX

Suggested shortcuts:

```text
Cmd/Ctrl + Shift + P → All
Cmd/Ctrl + P         → Navigation
```

Layout:

```text
┌───────────────────────────────────────────────┐
│ > portal                                  ⌘P │
├───────────────────────────────────────────────┤
│ Projects                                      │
│  ◫ portal             Project · main          │
│                                               │
│ Agents                                        │
│  ◉ Fix auth            Claude · Needs input   │
│                                               │
│ Commands                                      │
│  + New Task                                   │
│  ↻ Refresh                                    │
└───────────────────────────────────────────────┘
```

Requirements:

- keyboard-first;
- native focus/input/IME;
- virtualized/bounded result list;
- no input sent to Terminal while open;
- outside click/Escape dismisses;
- opening does not change current selection.

## 10. App Action Registry

One registry should feed:

```text
Command Center
native menu items
shortcut reference
possibly toolbar menus
```

Suggested action descriptor:

```go
type ActionDescriptor struct {
    ID          string
    Label       string
    Section     string
    Keywords    []string
    Shortcut    Shortcut
    Availability func(AppSnapshot) Availability
    Execute     func(ActionContext) error
}
```

Do not keep three separate command-name/shortcut catalogs.

## 11. Right Panel framework

The panel is Native UI and project-context aware.

Surfaces:

```text
Files
Services
Lazygit
```

Preview appears as a tool/action and may render in a separate MyGo WebView window until official embedded WebView exists.

### 11.1 Context

The active tool root follows the selected Tab's effective cwd.

Never assume a Project has one cwd.

### 11.2 Project-scoped presentation state

```go
type ProjectToolState struct {
    ActiveSurface string
    PanelWidth    float32
    Files         FilesPresentationState
    Services      ServicesPresentationState
}
```

Persist only lightweight presentation state.

Do not retain hidden Project tool processes.

### 11.3 Panel chrome

Reuse shared:

- IconButton;
- Toolbar;
- segmented/tabs primitive;
- EmptyState;
- InlineNotice;
- loading state;
- macOS/shadcn tokens.

No page-local duplicate controls.

## 12. Files service

### 12.1 Root

Root = selected Tab effective cwd.

If unavailable:

```text
No folder for this Tab
```

### 12.2 Lazy tree

Do not recursively walk the entire Project.

```text
open directory
→ list direct children
→ cache by path + modification identity
```

Sort:

```text
directories first
→ case-insensitive name
→ stable raw-name fallback
```

### 12.3 Symlinks

Show symlink files/directories as entries.
Do not recursively follow directory symlinks by default.

### 12.4 Preview

Initial supported preview:

- UTF-8-ish text;
- source code/plain text;
- bounded size.

Initial max preview target:

```text
1 MiB
```

Larger:

```text
File too large to preview
```

Binary:

```text
Binary file · size
```

No file editing in 0.9.

### 12.5 Refresh/watch

Manual refresh always exists.

Optional filesystem watch may invalidate expanded directories, but must be:

- rooted;
- bounded;
- cancellable on context switch;
- coalesced;
- never a recursive unbounded watcher fleet.

## 13. Scripts service

### 13.1 Domain

Port old Script definitions into Go.

At minimum:

```go
type Definition struct {
    ID        string
    Name      string
    ProjectKey string
    Command   string
    OneShot   bool
}
```

Preserve any additional old product fields only after direct parity audit.

### 13.2 Store

- versioned schema;
- atomic save;
- serialized mutation;
- corrupt-file recovery/error;
- no secrets in Script definition unless a later explicit secret model is added.

### 13.3 Execution

Script start goes through the Herdr runtime path and creates/uses Herdr-owned Tabs/Panes/processes.

No ordinary Script should become a client-owned `os/exec` child.

### 13.4 Reconciliation

Runtime status derives from authoritative Pane/process facts.

```text
Starting
Running
Failed
Stopped
```

Do not classify from arbitrary terminal output.

### 13.5 Stop

Stop is shown only where Shardlane owns the Script lifecycle transaction and can identify its exact runtime process/Pane safely.

## 14. Services projection

Combine:

```text
resident Script services
+ observed listening processes
```

Observed processes remain read-only.

Group by Project where useful.

Row:

```text
● web                          Running
  pnpm dev · pid 12345
  :3000  :3001                   Open →
```

Primary actions:

```text
Script Running  → exact Terminal
Script Stopped  → Start
Observed        → exact owning Pane/Terminal when known
Port            → Preview
```

## 15. Port intelligence

Reuse old Shardlane's bounded ownership model rather than scanning every process globally without context.

Preferred inputs:

1. Herdr `pane.process_info` / Script runtime facts;
2. bounded local platform port observation only when needed to fill missing port facts.

Port identity:

```go
type ListeningPort struct {
    Port       int
    Host       string
    SchemeHint string
    PID        int
    PaneID     string
    Source     string
}
```

### Refresh policy

- no scan on every render;
- only active/visible Project context;
- default bounded cadence when Services/port chrome is enabled;
- stop polling when feature disabled/no relevant window;
- manual refresh available.

Initial target cadence if scan is required:

```text
5 seconds while visible/relevant
```

Prefer event/process facts when they remove the need for a scan.

## 16. Port presentation

Do not add a permanent bottom status bar.

Allowed locations:

- Services surface;
- compact Project/header secondary action;
- Command Center result/action;
- optional Sidebar secondary line in non-compact density.

Example:

```text
portal                         :3000 :5173
```

Click a local port → PreviewService.

## 17. Lazygit service

### 17.1 Resolver

Resolve Lazygit through:

- GUI process PATH;
- login shell fallback if existing environment service already supports it;
- standard executable paths.

Use one shared executable resolver utility, not page-local `exec.LookPath` copies.

### 17.2 Git root

```text
selected Tab cwd
→ git rev-parse --show-toplevel
```

Background, bounded timeout.

### 17.3 Lifecycle

One auxiliary tool terminal per window maximum.

Destroy on:

- Right Panel hide;
- leaving Lazygit surface;
- selected Project/Tab root change;
- window close.

### 17.4 Terminal

Use official MyGo Terminal plugin.
Do not write a second terminal renderer.

## 18. PreviewService

### 18.1 URL policy

Accept only validated Project-local targets in 0.9 core:

```text
localhost
127.0.0.1
[::1]
```

No search-engine fallback.

### 18.2 MyGo migration behavior

Because MyGo 0.2.7 lacks a Native-UI-embedded WebView element:

```text
Open Preview
→ create/reuse a separate project-scoped MyGo WebView Window
```

This is an explicit migration implementation, not a change into a general browser product.

### 18.3 Preview window

Suggested chrome:

```text
Back
Forward
Reload
address / local target
Open in Browser
```

External navigation:

```text
outside approved local Preview scope
→ system browser
```

No arbitrary web search.

### 18.4 Security

- do not expose privileged Go bindings to arbitrary preview content;
- external links hand off;
- no filesystem URL access unless specifically required and audited;
- no claim of cookie/profile isolation without official MyGo support;
- Preview closes/retargets cleanly when owning Project disappears.

## 19. GitStatusService

### 19.1 Snapshot

```go
type Snapshot struct {
    Root         string
    Branch       string
    FilesChanged int
    Additions    int
    Deletions    int
    Ahead        int
    Behind       int
    Dirty        bool
    FetchedAt    time.Time
    Error        string
}
```

### 19.2 Commands

Port measured old Rust behavior using bounded Git subprocesses in background.

Do not invoke Git in render.

### 19.3 Cache

Key by normalized Git root.

Use stale-while-refresh:

```text
cached result shown immediately
→ refresh behind when stale
```

Do not blank the UI during every refresh.

### 19.4 Refresh triggers

- selected Project/Tab changed;
- explicit refresh;
- relevant file watcher invalidation if already available;
- bounded stale timeout.

No sub-second Git polling.

## 20. Git presentation

### Sidebar

Use density-aware secondary facts:

```text
main ↑2 ↓1
+34 −8
```

Compact mode may show less.

### Header Git popover

Suggested:

```text
main
3 files changed · +34 −8
2 commits ahead · 1 behind

[Open Lazygit] [Refresh]
```

Optional PR section appears only when PR adapter is available.

## 21. Optional GitHub PR adapter

### 21.1 First implementation boundary

Read-only enrichment only.

Preferred authentication source:

```text
installed/authenticated official `gh` CLI
```

Do not store a token.

### 21.2 Snapshot

```go
type PullRequest struct {
    Number         int
    Title          string
    URL            string
    State          string
    ReviewDecision string
    Mergeable      string
    ChecksTotal    int
    ChecksFailed   int
    ChecksPending  int
}
```

### 21.3 UX

```text
PR #142 · Changes requested
2 checks failing
[Open on GitHub] [Refresh]
```

No merge/review/comment mutation in 0.9 core.

### 21.4 Failure

`gh` absent/not authenticated/API offline must not degrade local Git status.

Show concise unavailable state only when user opens the PR detail.

## 22. Worktree capability gate

The event projection already knows worktree events, but 0.9 must verify exact mutation API before exposing buttons.

Required spike:

```text
list available Herdr worktree methods
pin request/response fixtures
```

If verified, follow-up tasks may add create/open/remove.

If not:

```text
local Git facts + Lazygit remain complete
worktree mutation deferred
```

No direct `git worktree` mutation from Native UI without architecture approval.

## 23. Desktop Attention service

0.9 does not reclassify Agent states.
It consumes 0.5/0.7 semantic state.

### 23.1 Dock badge

```go
type AttentionSnapshot struct {
    NeedsAttention int
    ReviewPending  int
}
```

Badge count:

```text
NeedsAttention + ReviewPending
```

0 clears.

### 23.2 Dock bounce

Optional setting, default conservative.

Only trigger on a **new** urgent transition while app inactive.

Throttle repeated bounce by Agent/status revision.

### 23.3 Notification integration

System notification click continues through the exact typed Agent destination from 0.5/0.7.

Do not make notification callbacks mutate Herdr focus globally.

## 24. Terminal file drop

### 24.1 Targeting

Native hit testing determines the Terminal Pane under the drop.

Do not route to whichever Pane happened to be selected before the drag.

### 24.2 Text

Local Unix first:

```text
paths
→ validate
→ POSIX shell quote each
→ join by spaces
→ terminal.Paste
```

No trailing newline/Enter.

### 24.3 Bounds

Initial:

```text
max paths: 256
max generated paste: 64 KiB
```

Reject:

- paths with control characters;
- unsupported shell/platform semantics;
- generated text above bound.

### 24.4 Remote

Remote file transfer/paste semantics are not part of 0.9 core.

## 25. Terminal links

Use MyGo terminal plugin's existing hyperlink detection/open path.

Tasks should verify:

- local packaged app opens HTTP/HTTPS correctly;
- no duplicate custom parser exists;
- opening a link does not send terminal input.

## 26. Terminal Find capability gate

### Required protocol audit

Before implementation:

- verify `pane.copy_search` or current equivalent on target Herdr protocol;
- pin exact response fields;
- pin unavailable behavior on older protocol.

### If supported

Native find bar:

```text
Find…                     3 of 17
[previous] [next] [close]
```

Query stays local to the find control.
Herdr performs semantic scrollback search.

### If unsupported

Do not implement VT-cell scraping/search shadow state.
Mark the task `DEFERRED_PROTOCOL_GAP`.

## 27. DiagnosticsSnapshot

```go
type Snapshot struct {
    AppVersion      string
    BuildID         string
    GoVersion       string
    MyGoVersion     string
    OS              string
    Arch            string
    HerdrVersion    string
    HerdrProtocol   int
    ActiveInstance  string
    Connection      string
    Integration     []IntegrationDiagnostic
    HistoryState    string
    UsageState      string
    LogPath         string
    DroppedLogs     uint64
}
```

Never gather this by running subprocesses synchronously during Native render.

Cache expensive facts.

## 28. Logs viewer

Read the existing bounded log files only when Diagnostics is open or explicitly exported.

Initial view cap:

```text
5,000 newest records
```

Filters:

```text
level
component
operation
text
```

Actions:

```text
Copy row
Copy visible
Reveal log file
Export diagnostics
```

No “upload logs” button in 0.9.

## 29. Diagnostic export

Create only on explicit user action.

Recommended `.zip`:

```text
diagnostics.json
logs.jsonl
README.txt
```

Redaction:

- replace home-prefix paths with `~` where possible;
- redact key/token-looking structured fields defensively;
- never include prompt/body/terminal/history content by design.

Export should have a hard maximum log payload.

## 30. MyGo updater

Use official:

```text
github.com/egoist/mygo/plugins/updater/native
```

rather than porting Rust platform swap code.

### 30.1 Release configuration

Enable signed MyGo updates in build configuration/release workflow.

The update private signing key is never stored in the repository.

### 30.2 Preferences

```text
Check automatically
Download automatically (default off unless product decides otherwise)
Last checked
Check now
```

### 30.3 User flow

```text
Check
→ Up to date
or
→ Update available + release notes
→ Install Update
→ Download progress
→ Install
→ Relaunch
```

Use plugin-provided error/unavailable states.

## 31. What's New

The updater release notes become the primary app-update change surface.

Do not build a second independent update-feed parser unless a product announcement requires a different source.

After an installed update, a lightweight “What's New” action may reopen the relevant release notes.

## 32. Sidebar density

One setting:

```go
type SidebarDensity string

const (
    DensityCompact     SidebarDensity = "compact"
    DensityDefault     SidebarDensity = "default"
    DensityComfortable SidebarDensity = "comfortable"
)
```

Changes only spacing/secondary metadata density.

It must not:

- change Project/Tab/Pane hierarchy;
- create alternate layout implementations;
- hide status semantics required for safety;
- become per-Project runtime state.

## 33. High contrast

High contrast raises the minimum contrast of Shardlane semantic UI colors:

- status glyphs;
- pills;
- selected rows;
- secondary text where needed.

Do not recolor Terminal output.

Implementation should remain token-based in Design System v2.

## 34. Task Presets

### 34.1 Model

```go
type TaskPreset struct {
    ID             string          `json:"id"`
    Name           string          `json:"name"`
    Provider       history.AgentID `json:"provider"`
    PromptTemplate string          `json:"prompt_template"`
}
```

### 34.2 Store

Shardlane-owned, versioned, atomic, small.

### 34.3 New Task UX

```text
Presets
[Review changes] [Fix tests] [Investigate]
```

Selecting fills Provider + Prompt.

No automatic launch.

### 34.4 Future fields

Model/effort may be added only after canonical Agent launch owns those values.

Do not mutate provider config files from a preset.

## 35. Settings additions

Suggested sections/fields:

### Appearance

```text
Sidebar density
High contrast
```

### Notifications

```text
Dock badge
Urgent Dock bounce
```

Reuse 0.5 notification settings rather than creating another notification preferences object.

### Updates

```text
Automatic checks
Automatic downloads
Last checked
Check now
```

### Developer / Diagnostics

```text
Open Diagnostics
Reveal logs
```

### Presets

Manage Task Presets from New Task first; dedicated Settings management is optional if needed.

## 36. Activity surface

Retain the product concept, but keep it semantic and bounded.

Activity may show recent client/session events such as:

```text
Agent needs attention
Agent ready for review
Script started/stopped/failed
Service port appeared/disappeared
Update installed
Integration changed
```

It must not become a duplicate debug log or raw Herdr event dump.

Initial policy:

- bounded recent list;
- process-memory/session-scoped unless a persistence need is proven;
- exact navigation target where applicable;
- no terminal-output events.

Activity is not a release blocker if the old product semantics cannot be pinned early; Services/Diagnostics already provide the critical workflow. If retained, finish it before package acceptance.

## 37. Cross-platform strategy

0.9 core should keep platform adapters explicit.

### macOS

Primary acceptance platform.

Required:

- Native UI;
- Tray/Menu Bar from 0.7;
- Dock badge;
- Notification;
- file drop;
- updater;
- Preview Window;
- Lazygit terminal.

### Windows

Plan for:

- Native UI;
- notification-area tray where supported;
- app badge behavior through MyGo abstraction;
- file drop only when path quoting is safe;
- updater only when MyGo build/release configuration supports target;
- Preview Window.

Do not claim full parity until real native acceptance runs.

### Linux

Plan for:

- Native UI;
- tray where desktop backend supports it;
- notifications;
- file drop;
- updater only where packaging policy supports it;
- Lazygit/Preview.

Desktop-environment differences must degrade explicitly.

## 38. Concurrency / cancellation

Rules across all 0.9 services:

- no render-time blocking work;
- replaceable requests have cancellation/generation ownership;
- stale result cannot overwrite current target;
- Project/Tab switch cancels obsolete Files/Git/Services work;
- hidden heavy surfaces stop background activity when it no longer benefits the user;
- no unbounded goroutine per Project/Paned item;
- no full shell update when a local secondary snapshot is unchanged.

Use current MyGo-safe UI update patterns; service goroutines return immutable snapshots.

## 39. Performance budgets

Initial targets:

### Command Center

- warm open from current catalogs: `< 16 ms` UI-side;
- rank first 500 catalog items: target `< 5 ms`;
- no IO on open.

### Files

- expand a directory with 1,000 direct entries: target `< 50 ms` background + bounded UI projection;
- no recursive whole-tree scan;
- preview read capped at 1 MiB.

### Git

- cached render immediate;
- background refresh target `< 2 s` timeout budget per command group;
- no refresh more often than required by staleness/explicit trigger.

### Services

- no port scan more often than configured bounded cadence;
- stop scan when feature disabled/no relevant owner.

### Diagnostics

- only newest 5,000 log records materialized;
- export payload bounded;
- normal app idle behavior unchanged when Diagnostics closed.

## 40. Logging

Extend bounded `applog`; do not create independent tool logs.

Useful fields:

```text
component
operation
project_id/tab_id/pane_id when relevant
duration_ms
count/status
error class
```

Never log:

- terminal output;
- dropped file contents;
- prompt/conversation text;
- file preview content;
- provider credentials;
- GitHub tokens;
- browser page content.

## 41. Security / trust boundaries

### Files

Read-only; canonical root; no path traversal from UI-generated IDs.

### Preview

Local targets only; no privileged bindings; external navigation handed off.

### GitHub

No token ownership in 0.9; optional `gh` adapter.

### Updater

Signed official MyGo update path; private signing key outside repository.

### Diagnostics

Explicit export only; sanitized, bounded; no auto-upload.

### File Drop

Quote paths only; no Enter; no content read; unsafe platform semantics disabled.

## 42. Atomic implementation roadmap

Tasks should normally fit **30 minutes to 4 hours**.
Do not combine unrelated surfaces into one patch.

### C0 — preflight and authority pins

| ID | Task | Verify |
|---|---|---|
| WIX-001 | audit post-0.8 tree against this plan | status matrix document |
| WIX-002 | pin MyGo exact version + official APIs used by 0.9 | compile/API probe tests |
| WIX-003 | pin old Rust RightPanel behavior fixtures | focused parity tests/notes |
| WIX-004 | pin old Script/Service domain fields | fixture table |
| WIX-005 | audit target Herdr terminal-search/worktree capabilities | protocol fixture or GAP result |

### C1 — Command Center domain

| ID | Task | Verify |
|---|---|---|
| WIX-010 | commandcenter Item/Action models | pure model tests |
| WIX-011 | catalog source interface | fake source tests |
| WIX-012 | Project/Tab/Pane source | projection fixtures |
| WIX-013 | Agent source | AgentDirectory fixtures |
| WIX-014 | History recent source | metadata-only tests |
| WIX-015 | Services source | service snapshot tests |
| WIX-016 | App/Settings action source | registry tests |
| WIX-017 | deterministic ranker | exact/prefix/substring/fuzzy table |
| WIX-018 | filters | table tests |
| WIX-019 | stale typed target revalidation | vanished-target test |

### C2 — Command Center Native UI

| ID | Task | Verify |
|---|---|---|
| WIX-020 | Command Center modal/panel | headless open/close |
| WIX-021 | native search field + IME-safe focus | input test |
| WIX-022 | virtualized grouped results | large-list headless test |
| WIX-023 | keyboard navigation/activation | headless input test |
| WIX-024 | Cmd/Ctrl-Shift-P action | shortcut test |
| WIX-025 | Cmd/Ctrl-P Navigation filter | shortcut/filter test |
| WIX-026 | outside click/Escape dismiss | interaction test |
| WIX-027 | zero-IO open assertion | service/static audit |

### C3 — shared action registry

| ID | Task | Verify |
|---|---|---|
| WIX-030 | ActionDescriptor registry | duplicate-ID test |
| WIX-031 | menu/shortcut labels consume registry | catalog consistency test |
| WIX-032 | availability model | disconnected/target tests |
| WIX-033 | command execution uses typed targets | routing tests |

### R0 — Right Panel framework

| ID | Task | Verify |
|---|---|---|
| WIX-040 | ProjectToolState model | pure tests |
| WIX-041 | selected-Tab cwd resolver | multi-pane fixture |
| WIX-042 | Native panel open/resize/close | headless layout test |
| WIX-043 | Files/Services/Lazygit surface chooser | interaction test |
| WIX-044 | per-Project lightweight state swap | A/B Project test |
| WIX-045 | heavy resource teardown on context switch | lifecycle test |
| WIX-046 | width/state restoration bounded to window | placement test |

### F0 — Files domain

| ID | Task | Verify |
|---|---|---|
| WIX-050 | direct-directory listing model | temp-dir test |
| WIX-051 | sort dirs-first/stable Unicode | table test |
| WIX-052 | symlink no-recursive-follow rule | temp symlink test |
| WIX-053 | lazy expansion cache | repeated-list test |
| WIX-054 | text/binary classifier | fixtures |
| WIX-055 | 1 MiB preview bound | large-file test |
| WIX-056 | obsolete load cancellation/generation | race/stale test |
| WIX-057 | path root validation | traversal tests |

### F1 — Files Native UI

| ID | Task | Verify |
|---|---|---|
| WIX-060 | Files tree/list | headless tree test |
| WIX-061 | expand/collapse | interaction test |
| WIX-062 | file selection preview | headless fixture |
| WIX-063 | binary/too-large/error states | render smoke |
| WIX-064 | Copy Path | clipboard adapter test |
| WIX-065 | Reveal in Finder/Explorer where supported | platform seam test |
| WIX-066 | manual refresh | refresh test |

### S0 — Script definitions

| ID | Task | Verify |
|---|---|---|
| WIX-070 | Go Script model parity | Rust fixture parity |
| WIX-071 | versioned atomic Script store | temp roundtrip/corrupt test |
| WIX-072 | Project association | normalized project tests |
| WIX-073 | one-shot/resident classification | table tests |
| WIX-074 | Script CRUD app service | service tests |

### S1 — Script runtime transaction

| ID | Task | Verify |
|---|---|---|
| WIX-080 | Herdr-backed Script start | fake RPC transaction test |
| WIX-081 | exact created Tab/Pane identity | mutation result test |
| WIX-082 | runtime status reconciliation | projection test |
| WIX-083 | Script stop safety | owning-process fixture |
| WIX-084 | uncertain-delivery reconciliation | lost-response test |
| WIX-085 | no client-owned shadow process | source/static audit |

### S2 — Services & ports

| ID | Task | Verify |
|---|---|---|
| WIX-090 | ServiceItem model | table tests |
| WIX-091 | resident Scripts → Services | fixture test |
| WIX-092 | observed process projection | fake process-info test |
| WIX-093 | Script vs Observed ownership actions | matrix test |
| WIX-094 | ListeningPort model | parsing tests |
| WIX-095 | bounded local port observer if still required | command fixtures/timeouts |
| WIX-096 | stale result cancellation on Project switch | generation test |
| WIX-097 | no polling when Services disabled/unowned | lifecycle test |

### S3 — Services Native UI

| ID | Task | Verify |
|---|---|---|
| WIX-100 | Services list grouped by Project/context | headless fixture |
| WIX-101 | status/command/pid/port rows | render smoke |
| WIX-102 | running row → exact Terminal | routing test |
| WIX-103 | stopped Script → Start | service/UI test |
| WIX-104 | observed process stays read-only | negative UI test |
| WIX-105 | port → Preview intent | typed action test |

### L0 — Lazygit

| ID | Task | Verify |
|---|---|---|
| WIX-110 | shared executable resolver seam | PATH/login-shell fixtures |
| WIX-111 | Git-root resolver | temp repo tests |
| WIX-112 | Lazygit availability/version state | fake executable tests |
| WIX-113 | one auxiliary MyGo Terminal session | lifecycle test |
| WIX-114 | destroy on panel hide | lifecycle test |
| WIX-115 | destroy/rebind on Project/Tab root switch | A/B root test |
| WIX-116 | no impact on Herdr Pane terminals | identity regression |
| WIX-117 | Native missing/update-recommended states | render smoke |

### P0 — Preview

| ID | Task | Verify |
|---|---|---|
| WIX-120 | PreviewTarget validation | URL table/security tests |
| WIX-121 | PreviewService project/port identity | pure tests |
| WIX-122 | separate MyGo WebView Preview Window | window smoke test |
| WIX-123 | reuse/retarget same Project preview | lifecycle test |
| WIX-124 | Back/Forward/Reload/Open Browser chrome | interaction test |
| WIX-125 | external navigation handoff | URL policy test |
| WIX-126 | no privileged Go binding exposure | security/static audit |
| WIX-127 | Project disappearance cleanup | lifecycle test |

### G0 — local Git intelligence

| ID | Task | Verify |
|---|---|---|
| WIX-130 | Git Snapshot model | pure tests |
| WIX-131 | git root | temp repo test |
| WIX-132 | branch | temp repo test |
| WIX-133 | diff counts | fixture repo test |
| WIX-134 | ahead/behind | local remotes fixture |
| WIX-135 | bounded command runner/timeouts | timeout test |
| WIX-136 | root-keyed cache | two-repo test |
| WIX-137 | stale-while-refresh | deterministic clock test |
| WIX-138 | stale result rejected after context switch | generation test |
| WIX-139 | no Git command from render | static audit |

### G1 — Git presentation

| ID | Task | Verify |
|---|---|---|
| WIX-140 | density-aware Sidebar Git metadata | headless density tests |
| WIX-141 | Header Git popover | headless interaction |
| WIX-142 | Refresh | service/UI test |
| WIX-143 | Open Lazygit | typed routing test |

### G2 — optional GitHub PR

| ID | Task | Verify |
|---|---|---|
| WIX-150 | `gh` capability detector | fake PATH/auth status tests |
| WIX-151 | PR DTO/parser | JSON fixtures |
| WIX-152 | branch → PR lookup | fake gh tests |
| WIX-153 | stale-while-refresh cache | service test |
| WIX-154 | PR readiness presentation | render smoke |
| WIX-155 | Open on GitHub | URL action test |
| WIX-156 | unavailable auth never degrades Git | negative test |
| WIX-157 | prove Shardlane stores no gh token | source/security audit |

### G3 — optional worktree gate

| ID | Task | Verify |
|---|---|---|
| WIX-160 | pin Herdr worktree method availability | protocol fixture/GAP |
| WIX-161 | implement typed adapter only if supported | adapter tests |
| WIX-162 | stale clicked-worktree revalidation | mutation safety test |

### D0 — desktop attention

| ID | Task | Verify |
|---|---|---|
| WIX-170 | AttentionSnapshot from StatusCenterSnapshot | table tests |
| WIX-171 | Dock badge adapter | MyGo fake/platform test |
| WIX-172 | clear badge at zero | adapter test |
| WIX-173 | inactive urgent bounce policy | transition tests |
| WIX-174 | bounce dedupe/throttle | deterministic clock test |
| WIX-175 | notification click keeps exact local destination | routing regression |

### T0 — file drop / links

| ID | Task | Verify |
|---|---|---|
| WIX-180 | dropped-path validator | table tests |
| WIX-181 | POSIX shell quoting | adversarial path tests |
| WIX-182 | 256 path / 64 KiB bounds | boundary tests |
| WIX-183 | hit-test drop to exact Terminal | multi-pane headless test |
| WIX-184 | Paste only, never Enter | terminal fake test |
| WIX-185 | unsafe Windows shell state disables path drop | platform test |
| WIX-186 | packaged Terminal URL open behavior | focused plugin acceptance |

### T1 — Terminal Find capability

| ID | Task | Verify |
|---|---|---|
| WIX-190 | exact Herdr search contract pinned | protocol fixture/GAP |
| WIX-191 | typed search adapter if supported | fake RPC tests |
| WIX-192 | Native Find bar if supported | headless tests |
| WIX-193 | match navigation/3-of-17 model | fixture tests |
| WIX-194 | unsupported protocol has explicit state, no VT search | regression/static audit |

### X0 — Diagnostics domain

| ID | Task | Verify |
|---|---|---|
| WIX-200 | DiagnosticsSnapshot | pure tests |
| WIX-201 | cached app/framework/runtime version facts | service test |
| WIX-202 | Herdr diagnostics facts | fake adapter test |
| WIX-203 | integration/catalog health composition | fixture test |
| WIX-204 | home/path sanitization | table tests |
| WIX-205 | secret-field redaction | adversarial tests |

### X1 — Logs viewer

| ID | Task | Verify |
|---|---|---|
| WIX-210 | bounded log tail reader | temp rotated logs test |
| WIX-211 | newest 5,000 cap | large log test |
| WIX-212 | structured filter | table tests |
| WIX-213 | Native Diagnostics page | headless render |
| WIX-214 | live tail only while open | lifecycle test |
| WIX-215 | Copy row/visible | clipboard tests |
| WIX-216 | dropped-log indication | fake counter test |

### X2 — diagnostic export

| ID | Task | Verify |
|---|---|---|
| WIX-220 | bounded diagnostic bundle builder | temp archive test |
| WIX-221 | logs sanitized in export | secret fixture test |
| WIX-222 | no transcript/terminal/history body sources | static/source audit |
| WIX-223 | explicit Save dialog only | UI interaction test |

### U0 — MyGo updater

| ID | Task | Verify |
|---|---|---|
| WIX-230 | enable signed MyGo update config | build-config test |
| WIX-231 | use Native updater plugin | compile/plugin test |
| WIX-232 | Check for Updates action registry | action test |
| WIX-233 | automatic-check preference bridge | store/plugin test |
| WIX-234 | automatic-download preference bridge | store/plugin test |
| WIX-235 | last-check presentation | deterministic test |
| WIX-236 | update/release-notes UI smoke | plugin harness/manual fixture |
| WIX-237 | dev/unconfigured unavailable behavior | fake build test |
| WIX-238 | private signing key absent from repository | security audit |
| WIX-239 | packaged two-version install/relaunch acceptance | final release gate |

### A0 — appearance

| ID | Task | Verify |
|---|---|---|
| WIX-240 | SidebarDensity setting | settings tests |
| WIX-241 | density token mapping | pure tests |
| WIX-242 | Compact render | headless snapshot/layout test |
| WIX-243 | Default render | headless test |
| WIX-244 | Comfortable render | headless test |
| WIX-245 | hierarchy/status invariant across densities | regression test |
| WIX-246 | HighContrast setting | settings test |
| WIX-247 | semantic contrast token mapping | color math/table tests |
| WIX-248 | Terminal colors unchanged | terminal-theme regression |

### N0 — Task Presets

| ID | Task | Verify |
|---|---|---|
| WIX-250 | TaskPreset model/validation | pure tests |
| WIX-251 | versioned atomic preset store | temp roundtrip test |
| WIX-252 | preset chips/list in New Task | headless UI |
| WIX-253 | selecting preset fills provider/prompt only | interaction test |
| WIX-254 | save current provider/prompt as preset | service/UI test |
| WIX-255 | rename/delete preset | CRUD tests |
| WIX-256 | no model/key/provider-file mutation | static/security audit |

### Q0 — Activity retained surface

| ID | Task | Verify |
|---|---|---|
| WIX-260 | audit old Activity semantics | parity note/tests |
| WIX-261 | semantic ActivityItem model | pure tests |
| WIX-262 | bounded recent activity store | capacity tests |
| WIX-263 | Agent/Script/Service event adapters | fixture tests |
| WIX-264 | Native Activity surface if retained | headless tests |
| WIX-265 | exact navigation targets | stale-target tests |

### Z0 — cross-surface consistency

| ID | Task | Verify |
|---|---|---|
| WIX-270 | shared Project/Tab context drives Right Panel + Git + Services | A/B fixture |
| WIX-271 | Project switch cancels obsolete tool work | lifecycle/race test |
| WIX-272 | Command Center actions reuse Router/app services | routing audit |
| WIX-273 | Status Center/notifications/Dock share one attention model | projection test |
| WIX-274 | shared icon/component usage audit | source audit |
| WIX-275 | no render-time subprocess/filesystem/network | static review |
| WIX-276 | no new client runtime/process authority | architecture audit |

### Z1 — package acceptance only at the end

| ID | Task | Verify |
|---|---|---|
| WIX-280 | full Go test suite | `go test ./...` |
| WIX-281 | race-sensitive service suites | `go test -race ...` |
| WIX-282 | `git diff --check` | clean |
| WIX-283 | MyGo build | packaged app/build success |
| WIX-284 | isolated startup smoke | app launch + Herdr bootstrap |
| WIX-285 | real macOS workflow acceptance | focused checklist |
| WIX-286 | updater two-version acceptance | signed release fixture |
| WIX-287 | large Project performance pass | measured budgets |
| WIX-288 | security/privacy audit | checklist + tests |
| WIX-289 | 0.9 closure audit document | DONE/PARTIAL/DEFERRED matrix |

## 43. Development cadence

Do not run the entire product gate after every atomic task.

### Per-task

Run the narrowest package/test:

```sh
cd next
GOTOOLCHAIN=go1.27.1 go test ./internal/<package> -run <focused-test>
cd ..
git diff --check
```

### Per workstream

Run related packages only.

Examples:

```sh
cd next
GOTOOLCHAIN=go1.27.1 go test ./internal/commandcenter/... ./internal/nativeui/...
GOTOOLCHAIN=go1.27.1 go test ./internal/scripts/... ./internal/gitintel/...
```

### Final only

```sh
cd next
GOTOOLCHAIN=go1.27.1 go test ./...
GOTOOLCHAIN=go1.27.1 go test -race \
  ./internal/agent/... \
  ./internal/app/... \
  ./internal/commandcenter/... \
  ./internal/filesview/... \
  ./internal/scripts/... \
  ./internal/gitintel/... \
  ./internal/diagnostics/... \
  ./internal/nativeui/...
GOTOOLCHAIN=go1.27.1 go tool mygo build
cd ..
git diff --check
```

Do full visual/real-app verification only after feature workstreams are substantially closed.

## 44. Focused acceptance scenarios

### Command Center

1. Cmd-Shift-P opens instantly without IO.
2. `portal` returns Project, relevant Agent and commands in deterministic order.
3. stale Agent disappears between open/click → no wrong navigation.
4. Cmd-P starts in Navigation filter.
5. IME text stays in the search field, never Terminal.

### Files / tools

6. Files follows selected Tab cwd, not a stale Project cwd.
7. expanding one folder does not recursively scan the whole repository.
8. 10 MiB file shows too-large state without loading it all into UI.
9. binary file does not render garbage.
10. Project switch restores that Project's lightweight panel state but not hidden Lazygit processes.

### Scripts / Services

11. resident Script appears in Services.
12. one-shot command does not masquerade as a service.
13. Running service click reaches exact Terminal.
14. observed process has no unsafe Stop action.
15. stopped Script Start creates Herdr-owned runtime objects exactly once.
16. `:3000` port opens Preview intent.

### Lazygit

17. one Lazygit auxiliary process maximum.
18. hiding panel stops Lazygit.
19. switching Project root stops/rebinds it.
20. missing lazygit is a clear Native state.

### Preview

21. localhost target opens project-scoped Preview Window.
22. external link leaves to system browser.
23. Preview cannot call privileged Shardlane bindings.
24. Preview disappears/retargets when Project no longer exists.

### Git / PR

25. branch/ahead/behind/diff state renders from cache while refresh runs.
26. Git failure never blocks Terminal/Agent operation.
27. no Git command occurs from render.
28. authenticated `gh` shows PR readiness when adapter is available.
29. missing/unauthenticated `gh` simply hides/degrades PR enrichment.
30. Shardlane stores no GitHub token.

### Desktop attention

31. new NeedsAttention while app inactive updates notification/Dock badge.
32. Working alone does not increase Dock badge.
33. Mark reviewed reduces badge consistently.
34. repeated same status does not bounce repeatedly.
35. notification click reaches exact Agent destination.

### Terminal ergonomics

36. dropping `a b.txt` pastes a correctly quoted path, not a command execution.
37. drop never presses Enter.
38. drop targets Pane under pointer.
39. >256 paths or >64 KiB is refused.
40. terminal HTTP link opens through official plugin behavior.
41. Find is available only when semantic Herdr search capability is pinned.

### Diagnostics

42. log viewer shows newest bounded rows, including prior run logs.
43. filtering does not reread unbounded file content.
44. diagnostic export contains no prompt/terminal/history text.
45. secrets are redacted.
46. nothing uploads automatically.

### Updates

47. packaged release can check for update.
48. release notes render through official MyGo updater.
49. development/unconfigured build reports unavailable cleanly.
50. signed two-version test installs and relaunches successfully before release.

### Appearance / Presets

51. density changes spacing only; hierarchy remains Project → Tab → Pane.
52. high contrast improves app semantic colors without changing Terminal colors.
53. Task Preset fills Provider + Prompt but does not launch.
54. preset cannot modify provider model/key configuration.

## 45. Definition of Done

0.9 is complete when:

- [ ] Command Center is the unified fast navigation/action entry point;
- [ ] Command Center uses existing snapshots and performs no open-time IO;
- [ ] one shared action registry feeds palette/shortcuts/menu metadata where practical;
- [ ] Native Right Panel framework is project-context aware;
- [ ] Files is fast, lazy, read-only and bounded;
- [ ] Scripts/Services are ported without creating a client process runtime;
- [ ] listening ports are visible/actionable with bounded observation;
- [ ] Lazygit uses one bounded official MyGo Terminal auxiliary session;
- [ ] Local Preview works through a supported MyGo WebView Window path while embedded WebView remains unavailable;
- [ ] local Git branch/diff/ahead-behind intelligence is available;
- [ ] optional PR enrichment degrades safely and stores no GitHub token;
- [ ] Dock badge is driven by existing Agent attention/review semantics;
- [ ] local file drops paste safe paths into the exact Terminal without Enter;
- [ ] Terminal link behavior uses the official plugin;
- [ ] Terminal Find either uses a pinned semantic Herdr API or is explicitly deferred;
- [ ] Diagnostics/Logs Center gives users actionable bounded diagnostics;
- [ ] diagnostic export is explicit, sanitized and privacy-safe;
- [ ] official MyGo updater replaces the need to port the Rust updater mechanics;
- [ ] update/release notes flow is packaged and verified;
- [ ] Sidebar density and high contrast reuse Design System tokens;
- [ ] Task Presets improve New Task without mutating provider config;
- [ ] no general browser, titlebar Tabs, mandatory gateway or second runtime was introduced;
- [ ] atomic/package tests pass;
- [ ] final race/build/real-app/security/performance gates pass;
- [ ] `docs/mygo-native-0.9.0-closure-audit.md` records evidence and remaining protocol gaps.

## 46. Post-0.9 boundary

After 0.9, the product should have the complete local desktop workflow.

The remaining major program becomes:

```text
Remote/Mobile API v2 convergence
→ remote/device capability parity
→ Linux/Windows native acceptance and packaging
→ Rust dependency removal
→ 1.0 final cutover
```

Do not silently expand 0.9 into the Remote/Teleport program.
