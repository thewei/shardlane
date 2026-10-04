# Shardlane MyGo 0.9.0 Workspace Intelligence & Desktop Experience — Implementation Prompt

Continue the Shardlane MyGo migration after **0.8 History Management, Usage & Agent Inspector** is complete.

Repository:

`/Users/wilson/Workspaces/wh-studio/herdr-client`

Use the existing Devspace workspace and branch `rewrite/mygo`.
Preserve all user/other-agent changes. Do not reset, stage, commit or push unless explicitly requested.

## Read first

1. `AGENTS.md`
2. `CLAUDE.md`
3. `.agents/skills/herdr-client-development/SKILL.md`
4. `docs/client-product-architecture.md`
5. `docs/performance-engineering.md`
6. `docs/terminal-interaction-spec.md`
7. `docs/mygo-native-execution-rules.md`
8. `docs/mygo-native-migration-roadmap.md`
9. `docs/reference-magpie-herdr-gpui-shardlane-0.9-audit.md`
10. `docs/mygo-native-0.9.0-workspace-intelligence-desktop-experience-plan.md`
11. `next/CLAUDE.md`

Then inspect current implementations before changing them:

```text
next/internal/nativeui
next/internal/herdr
next/internal/agent
next/internal/history
next/internal/usage
next/internal/applog
next/internal/settings
```

Use the old Rust surfaces as product-parity references only:

```text
crates/herdr-gui/src/right_panel/*
crates/herdr-gui/src/scripts/*
crates/herdr-gui/src/git_status.rs
crates/herdr-gui/src/search_model.rs
crates/herdr-gui/src/search_view.rs
crates/herdr-gui/src/notifications.rs
crates/herdr-gui/src/update_check.rs
crates/herdr-gui/src/update_install.rs
```

Do not port GPUI mechanics line by line.

## Mission

Implement **0.9 Workspace Intelligence & Desktop Experience** as one large but decomposed release.

The user outcome is:

```text
one fast command entry point
+ Project-native Files / Services / Lazygit
+ port → local Preview
+ Git/optional PR intelligence
+ native desktop attention
+ terminal file-drop polish
+ self-service diagnostics
+ official updates
+ appearance density/high contrast
+ safe New Task presets
```

Herdr remains the runtime authority.

## Hard architecture invariants

Do not violate these even for parity:

- no second Workspace/Project registry;
- no client-owned Pane/PTY/runtime authority;
- no general browser product;
- no titlebar Tabs;
- no mandatory model gateway;
- no provider credential ownership for Agent runtime;
- no render-time Herdr RPC/filesystem/Git/network subprocess;
- no unbounded polling/goroutine fleets;
- no private Native-UI WebView embedding;
- no direct `git worktree` mutation unless a verified Herdr contract owns it;
- no terminal-output parsing to infer Agent or Service semantics;
- no prompt/terminal/history content in logs/diagnostic exports.

## P0 — preflight before feature implementation

Audit the actual post-0.8 tree against the 0.9 plan.

Verify:

- exact MyGo version;
- official File Drop / Notification / Dock / Tray / Updater APIs;
- exact current Herdr protocol;
- whether semantic Terminal Find exists;
- whether worktree mutation APIs exist;
- which old Rust Script fields still matter;
- whether any 0.9 item was already implemented by earlier work.

Update the task ledger before writing duplicates.

Never assume a reference project's API exists in Shardlane/Herdr.

## P1 — Command Center

Create `internal/commandcenter` independent of MyGo.

Sources must be snapshot-based:

```text
Projects/Tabs/Panes
Agents
recent History
Services/Scripts
App actions
Settings destinations
```

Ranking:

```text
exact
prefix
substring
fuzzy
```

Required shortcuts:

```text
Cmd/Ctrl + Shift + P → All
Cmd/Ctrl + P         → Navigation
```

The full Search page remains content/history search.
Command Center is navigation/actions.

Opening the palette performs zero filesystem/network/RPC work.

Every result carries a typed stable target; revalidate it before executing.

## P2 — shared Action Registry

Unify action metadata used by:

```text
Command Center
shortcut reference
native menus
selected toolbar/menu surfaces
```

Do not duplicate labels/shortcuts/availability rules across pages.

Availability is derived from application snapshot/capabilities, not from render-time IO.

## P3 — Native Right Panel framework

Port the original Project tool concept with surfaces:

```text
Files
Services
Lazygit
```

The active tool root follows the selected **Tab cwd**.
A Project does not have one universal cwd.

Per-Project presentation state may persist, but heavy resources do not.

Project/Tab switch must cancel obsolete tool work and stop auxiliary Lazygit.

## P4 — Files

Initial Files is read-only.

Implement:

- lazy direct-directory listing;
- directories-first stable sort;
- expand/collapse;
- selected file preview;
- binary state;
- 1 MiB preview cap;
- no recursive directory-symlink following;
- Copy Path;
- Reveal in Finder/Explorer where supported;
- explicit Refresh;
- cancellation/generation guard.

Do not recursively scan the whole repository on page open.
Do not add file editing/saving in 0.9.

## P5 — Scripts

Port Script definitions/application service into Go.

Shardlane owns Script definitions while Herdr has no native Script resource.
Herdr owns execution:

```text
Tab
Pane
PTY
process
```

Implement:

- versioned atomic store;
- CRUD;
- one-shot/resident classification;
- Herdr-backed Start transaction;
- safe Stop only for exact Shardlane-owned Script lifecycle;
- uncertain-delivery reconciliation;
- runtime status projection.

Do not turn Scripts into `os/exec` children managed by Shardlane.

## P6 — Services and port intelligence

Combine:

```text
resident Script services
+ observed listening processes
```

Observed processes are read-only.

Show:

```text
status
command
pid
ports
owner Project/Pane
```

Actions:

```text
running service → exact Terminal
stopped Script  → Start
port            → Preview intent
```

Use Herdr/process facts first. Only use a bounded local port observer when needed.

No scan from render.
No scan when the feature has no relevant owner.

## P7 — Lazygit

Use the official MyGo Terminal plugin.

Lifecycle:

```text
selected Tab cwd
→ resolve Git root
→ resolve lazygit
→ create exactly one auxiliary tool terminal
```

Destroy it on:

- panel hide;
- surface switch;
- Project/Tab root switch;
- window close.

Never replace or reuse a Herdr Pane Terminal as the Lazygit runtime.

## P8 — Local Preview

MyGo 0.2.7 has no official WebView element embedded in a Native UI tree.
Do not build private embedding.

Until that changes:

```text
port / Open Preview
→ separate project-scoped MyGo WebView Preview Window
```

Accept only approved local targets:

```text
localhost
127.0.0.1
[::1]
```

Preview is not a browser product.

External navigation → system browser.
No arbitrary search/bookmarks/general tabs.
Do not expose privileged Go bindings to preview pages.

If MyGo gains an official embedded WebView before implementation, document and audit it first; only then may the same PreviewService move into the Right Panel.

## P9 — local Git intelligence

Create UI-independent `gitintel` service.

Snapshot:

```text
root
branch
changed files
additions/deletions
ahead/behind
dirty
fetched-at/error
```

Use bounded background Git commands and stale-while-refresh cache.

No Git command from render/menu-open.

Presentation:

```text
Sidebar secondary metadata
Header Git popover
Command Center actions
```

Keep normal rows compact.

## P10 — optional GitHub PR readiness

First implementation is **read-only enrichment**.

Preferred auth seam:

```text
installed + authenticated official `gh` CLI
```

Do not build/store a GitHub token vault in 0.9.

Show, when available:

```text
PR number/title/state
review decision
checks
mergeability/conflict
Open on GitHub
```

Do not merge/approve/comment in 0.9 core.

Missing or unauthenticated `gh` must never degrade local Git intelligence.

## P11 — worktree capability gate

Before exposing worktree mutation:

- inspect target Herdr protocol;
- pin method names/request-response fixtures;
- add typed adapter tests.

If no stable Herdr API exists, defer.

Do not call `git worktree` directly from Native UI as a workaround.

## P12 — desktop attention closure

Reuse 0.5 notifications and 0.7 StatusCenterSnapshot.

Add official MyGo desktop affordances:

```text
Dock badge = NeedsAttention + ReviewPending
optional urgent Dock bounce while inactive
```

Working does not increase badge.

Bounce only on a new urgent transition and throttle/dedupe it.

Do not create another Agent status classifier.

Custom notification sound is not required unless a separately approved supported audio path exists.

## P13 — local File Drop → Terminal

Use official MyGo file-drop + terminal Paste APIs.

Rules:

- target Terminal under pointer;
- paths only, not file contents;
- safe shell quoting;
- no Enter/newline execution;
- max 256 paths;
- max 64 KiB generated text;
- reject control characters;
- unsafe/unknown Windows shell semantics disable the action rather than guessing;
- no remote upload semantics.

## P14 — Terminal links

Use the official MyGo Terminal behavior.

Verify packaged HTTP/HTTPS opening.
Do not add another terminal hyperlink parser.

## P15 — Terminal Find capability gate

First audit exact Herdr semantic API (`pane.copy_search` or current equivalent).

If supported:

```text
Native find field
match count
next/previous
Herdr semantic scrollback search
```

If unsupported:

```text
DEFERRED_PROTOCOL_GAP
```

Never implement a hidden client-side VT/terminal-output search index.

## P16 — Diagnostics & Logs

Build on existing bounded `internal/applog`.

Diagnostics snapshot should cover:

```text
Shardlane version/build
Go/MyGo version
OS/arch
Herdr version/protocol/path
active instance/connection
Integration health
History/Usage catalog health
log path/rotation
dropped logs
```

Log viewer:

```text
newest 5,000 max
filter by level/component/operation/text
copy
live tail only while open
```

No unbounded log memory.

## P17 — Diagnostic export

Explicit user action only.

Create bounded sanitized archive containing only diagnostic metadata + logs.

Never include:

```text
terminal output
prompt/conversation text
History transcripts
file preview content
credentials/tokens
environment dump
```

Normalize/redact home paths and secret-looking structured fields.

Nothing uploads automatically.

## P18 — official MyGo updater

Use:

```text
github.com/egoist/mygo/plugins/updater/native
```

Do not port the Rust curl/ditto/codesign updater mechanics.

Enable signed update build config/release workflow.
Private signing key stays outside repository.

UX:

```text
Check for Updates
release notes
download/install progress
install/relaunch
skip/later if plugin supports
automatic checks setting
automatic downloads setting
last checked
```

Development/unconfigured builds must degrade cleanly.

Do not require Homebrew.

## P19 — appearance

Keep one canonical Sidebar.

Add:

```text
Sidebar density: Compact / Default / Comfortable
High contrast: On/Off
```

Use Design System tokens.

Density changes spacing/secondary metadata only.
It never changes Project → Tab → Pane ownership/hierarchy.

High contrast never recolors Terminal program output.

## P20 — Task Presets

Adapt Magpie Profiles safely as **New Task Presets**, not provider configuration profiles.

Store only Shardlane-owned fields:

```text
Name
Provider
PromptTemplate
```

Selecting a preset fills the New Task form.
It never launches automatically.

Do not store or mutate:

```text
API keys
provider config
model/effort guesses
Herdr runtime state
```

Only extend preset fields when canonical Agent launch gains authoritative capability fields.

## P21 — Activity

Audit old Activity semantics first.

If retained, make it a bounded semantic timeline of product events, not a debug log:

```text
Agent attention/review
Script/Service lifecycle
ports
Integration changes
updates
```

No terminal text/raw event dump.

Activity is lower priority than Command Center/Tools/Diagnostics/Updater and must not block the version if parity cannot be pinned early.

## P22 — cross-platform truthfulness

macOS is primary real-app acceptance platform for 0.9.

Windows/Linux implementations must use MyGo abstractions where available and explicitly report unsupported capability where not.

Do not claim parity from compilation alone.

Particularly verify:

```text
Tray/Dock/taskbar behavior
Notifications
File Drop
Preview WebView Window
Lazygit PTY
Updater packaging
path/shell quoting
```

## P23 — no render-time work

Mandatory audit before closure:

No Native render/menu-open path may run:

```text
Herdr RPC
Git subprocess
gh subprocess
filesystem tree scan
port scan
update network call
History parse
Diagnostics export
```

All such work belongs to services with immutable snapshots, cancellation and stale-result protection.

## Atomic task execution

Use exact task IDs from:

`docs/mygo-native-0.9.0-workspace-intelligence-desktop-experience-plan.md`

Ranges:

```text
WIX-001..005   preflight
WIX-010..033   Command Center/actions
WIX-040..066   Right Panel/Files
WIX-070..105   Scripts/Services/Ports
WIX-110..127   Lazygit/Preview
WIX-130..162   Git/PR/worktree gate
WIX-170..194   Desktop attention/Terminal ergonomics
WIX-200..223   Diagnostics/Logs/export
WIX-230..239   Updater
WIX-240..256   Appearance/Presets
WIX-260..276   Activity/integration consistency
WIX-280..289   final acceptance
```

One task = one objective + one focused verification.
Prefer 30 minutes–4 hours each.

## Fast verification policy

Do **not** run the full app build after each task.

Per task:

```sh
cd next
GOTOOLCHAIN=go1.27.1 go test ./internal/<affected-package> -run '<focused test>'
cd ..
git diff --check
```

Per workstream, run only related packages.

Full/race/build/real-app testing belongs to `WIX-280..289` after the implementation is substantially closed.

## Final gate

At the end:

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

Then run focused packaged macOS acceptance for the 54 scenarios in the 0.9 plan, plus the updater two-version test.

## Required final audit

Create:

`docs/mygo-native-0.9.0-closure-audit.md`

Report `DONE / PARTIAL / DEFERRED_PROTOCOL_GAP / NOT DONE` for:

- Command Center;
- Action Registry;
- Right Panel;
- Files;
- Script store/runtime;
- Services;
- port intelligence;
- Lazygit;
- Preview Window;
- local Git status;
- GitHub PR enrichment;
- worktree mutation gate;
- Dock badge/bounce;
- file drop;
- terminal links;
- Terminal Find;
- DiagnosticsSnapshot;
- Logs viewer;
- diagnostic export;
- official updater;
- Sidebar density;
- high contrast;
- Task Presets;
- Activity;
- macOS package acceptance;
- Windows/Linux compile/native evidence separately.

Also prove:

- no second runtime/Workspace registry exists;
- no render-time blocking IO exists;
- no general browser/titlebar Tabs were introduced;
- no mandatory gateway/provider credential ownership was introduced;
- no client-owned Script process runtime exists;
- no hidden Lazygit fleet exists;
- no GitHub token is stored;
- no diagnostics export contains user content/secrets;
- no unsafe file-drop command execution exists;
- no VT shadow search exists when Herdr search is absent;
- full tests/race/build/real-app acceptance pass.

Proceed without asking for confirmation.
