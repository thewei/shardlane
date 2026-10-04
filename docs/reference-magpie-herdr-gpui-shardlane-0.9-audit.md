# Shardlane 0.9 Reference Audit — Magpie × herdr-gpui × Current MyGo Client

Status: **planning/reference audit**
Audit date: 2026-10-05
Target consumer: `docs/mygo-native-0.9.0-workspace-intelligence-desktop-experience-plan.md`
Architecture authority: `docs/client-product-architecture.md`

Reference snapshots inspected:

- `yetone/magpie` — `460057a` (`main`, 2026-10-05 11:16 +08:00)
- `penso/herdr-gpui` — `6ae54ea` (`main`, 2026-10-04 17:29 -07:00)
- Shardlane current branch — `rewrite/mygo`, post-0.4 tree with 0.5 work in progress
- MyGo framework source — `v0.2.7`

This audit compares **product behavior and architectural patterns**, not source code to copy.
External projects are references only; Shardlane's architecture and Herdr ownership remain authoritative.

## 1. Executive conclusion

0.7 and 0.8 already cover the two biggest Agent-management gaps:

```text
0.7 — Status Center + menu-bar quick panel
0.8 — Project-grouped History + Usage + Agent Inspector
```

The highest-value remaining gap is not another Agent page. It is the **daily desktop-development loop around the Agent**:

```text
find anything quickly
→ understand Project state
→ open Files / Services / Git / Preview
→ react to Agent attention from the OS
→ diagnose failures without a developer build
→ keep the application current
```

Therefore the recommended next large release is:

> **Shardlane MyGo 0.9.0 — Workspace Intelligence & Desktop Experience**

It should replace the old thin “Desktop Tools” milestone with one coherent workspace experience.

## 2. Product boundary that must not move

The three projects solve different problems.

### Shardlane

```text
Herdr desktop client/product
Project / Tab / Pane / Agent orchestration
Native development workspace
```

### herdr-gpui

```text
Native Herdr client
strong terminal/workspace desktop UX
Git/PR, ports, notifications, updater, diagnostics
```

### Magpie

```text
Agent/provider/model gateway + manager
profiles, usage, quota, sessions, backup/sync
menu-bar-first management UX
```

Shardlane can absorb UX and domain separation from both references, but must not become:

- a second Herdr runtime;
- a mandatory LLM gateway;
- a general-purpose browser;
- a second Tab presentation system;
- a provider credential manager merely to show useful Agent state.

## 3. What Shardlane already has or already plans

Do not duplicate these in 0.9.

### Landed / current migration foundation

- MyGo Native shell;
- Native Terminal through official MyGo terminal/Ghostty plugin;
- canonical Sidebar / Project → Tab → Pane hierarchy;
- Native Settings;
- Native History MVP;
- bounded structured `applog` with rotation;
- Provider integration health;
- safe Agent launch transaction;
- Design System v2/shared Native components.

### 0.5

- AgentCardModel / AgentDirectory;
- runtime phase, attention, sendability, unread/review axes;
- Agent Workbench and MRU switching;
- meaningful Agent notifications;
- Continue/Resume planning.

### 0.6

- Native Conversation/Chat;
- exact live Conversation identity;
- structured interactions;
- follow-up queue;
- Handoff/context transfer.

### 0.7

- right-top Status Center;
- macOS menu-bar Native quick panel;
- exact Agent quick navigation;
- compact Agent usage/quota projection.

### 0.8

- History grouped by normalized Project;
- safe Trash/Restore/Purge capability model;
- UsageService;
- Agent/Session/Project/Provider/Model summaries;
- optional quota service;
- Native Usage page;
- Agent Inspector.

0.9 must consume these services instead of inventing new copies.

## 4. Three-way capability matrix

| Capability | Magpie | herdr-gpui | Current / planned Shardlane | 0.9 decision |
|---|---|---|---|---|
| Menu-bar quick panel | strong | status/menu patterns | 0.7 planned | already covered |
| Agent status center | partial | strong | 0.5/0.7 | already covered |
| History/session management | strong | not central | 0.8 planned | already covered |
| Usage/quota | strong | native usage/quota display | 0.8 planned | already covered |
| Agent Inspector | provider-oriented detail | agent/workspace detail | 0.8 planned | already covered |
| Unified command palette | limited TUI picker | **strong** | no MyGo unified palette | **ADOPT** |
| Project Files | no | limited client scope | old Rust Right Panel | **MIGRATE** |
| Lazygit | no | no dedicated product surface | old Rust Right Panel | **MIGRATE** |
| Services/listening ports | gateway health, not Project ports | **strong** | old Rust Scripts/Services | **MIGRATE + IMPROVE** |
| Local web preview | no | browser tab system | old Rust local Preview | **ADAPT** |
| Git branch/ahead/behind | no | **strong** | old Rust `git_status.rs` | **MIGRATE + IMPROVE** |
| GitHub PR readiness | no | **strong** | not closed in MyGo | **ADAPT / optional integration** |
| System notifications | indirect | **strong** | 0.5 planned | close desktop loop only |
| Dock/app badge | no | **strong** | absent | **ADOPT** |
| Custom notification sound | no | strong | old Rust support | **DEFER unless supported cleanly** |
| Logs viewer | ordinary logs | **strong** | bounded logs only | **ADOPT** |
| Diagnostics export | partial health | strong diagnostics patterns | absent | **ADOPT** |
| Self-update / release notes | **strong** | **strong** | old Rust updater; MyGo official plugin available | **ADOPT / MIGRATE** |
| File drag/drop to Terminal | no | **strong** | absent; MyGo officially supports file drop | **ADOPT** |
| Terminal links | no | strong | MyGo terminal already supports links | **VERIFY / expose** |
| Terminal Find | no | strong (`pane.copy_search`) | not wrapped in Go | **CAPABILITY-GATED** |
| Keyboard Copy Mode | no | strong | no MyGo parity | **DEFER** |
| Sidebar densities | no | many layouts | one canonical Native Sidebar | **ADAPT: density only** |
| Theme catalog | basic | large theme picker | app theme + DS | **ADAPT minimally** |
| High contrast | no | strong | not complete | **ADOPT** |
| Agent setting Profiles | **strong** | no | launch request lacks model/effort | **ADAPT as Task Presets only** |
| Backup/restore/sync | **strong** | no | absent | **DEFER from 0.9 core** |
| Launch at login | strong | not central | no verified MyGo API | **DEFER / platform capability gap** |
| General browser tabs | no | strong | product non-goal | **REJECT** |
| Tabs in titlebar | no | newest feature | Sidebar owns Tab presentation | **REJECT** |
| Mandatory model gateway | core | no | product non-goal | **REJECT** |
| Provider credential ownership | core | optional GitHub only | no | **REJECT for Agent runtime** |
| Remote devices/Teleport | no | strong | later Remote/Platform milestone | **DEFER** |
| Remote file copy | no | strong | later Remote milestone | **DEFER** |
| CPU/memory always-on status | no | strong | no bottom status bar | **ADAPT to Diagnostics only** |

## 5. Highest-value feature: Unified Command Center

### herdr-gpui reference

Its command palette unifies:

- workspace/host navigation;
- Agents;
- terminal Panes;
- GUI commands;
- configured daemon commands;
- local Project discovery;
- fuzzy search and category filters.

This is a major UX improvement because it reduces reliance on knowing where a feature lives.

### Shardlane adaptation

Create a **Command Center**, not another full Search page.

It should index only already-known presentation/domain snapshots:

```text
Projects
Tabs
Panes
Agents
recent History sessions
Services/Scripts
App actions
Settings destinations
```

Search ranking:

```text
exact
→ word prefix
→ substring
→ fuzzy
```

Filters:

```text
All
Navigation
Agents
Projects
Commands
History
```

Suggested shortcuts:

```text
Cmd/Ctrl + Shift + P → All
Cmd/Ctrl + P         → Navigation
```

Keep the existing full Search route for transcript/content search.
The Command Center is action/navigation search, not an FTS replacement.

### Important ownership rule

Palette rendering performs no Herdr RPC, filesystem scan, Git command, or History parse.
Sources push immutable catalog snapshots into the Command Center service.

## 6. Right Panel: migrate the old Shardlane product, not a generic tool drawer

The old Rust client already has a well-defined Project-scoped Right Panel:

```text
Files
Services
Lazygit
Browser/Preview
```

It also already established a critical rule:

> Files and Lazygit follow the selected Tab's cwd; a Project does not own a single universal cwd.

0.9 should preserve this rule.

### Project-scoped surface state

Each Project may remember lightweight presentation state:

```text
active surface
Files expansion / selected path
Services filter
Lazygit open state
Preview target
panel width
```

Heavy process/window resources are not kept alive for every Project.

## 7. Files surface

### Adopt from old Shardlane

- Project/selected-Tab-rooted tree;
- expandable directories;
- full-content preview for bounded text files;
- selected-path persistence per Project.

### Improve

- virtualized/flattened tree projection;
- explicit binary/large-file state;
- “Reveal in Finder/Explorer” / “Copy Path” using platform APIs;
- refresh driven by explicit action + bounded file watcher when safe;
- loading/error/permission states using shared components.

### Safety

Initial 0.9 Files remains **read-only**.

Do not add editor/save semantics in this release.

Bounds:

- do not recursively scan unopened directories;
- skip traversal through directory symlinks by default;
- cap preview bytes;
- never block UI on filesystem reads.

## 8. Services & Port Intelligence

This is one of the strongest overlaps between old Shardlane and herdr-gpui.

### Old Shardlane already has

- Script definitions;
- resident vs one-shot command semantics;
- process monitoring;
- observed listening services;
- PID/command/port presentation;
- jump to owning Terminal;
- localhost opening.

### herdr-gpui adds a stronger UX pattern

- listening ports appear directly with Workspace context;
- a port is actionable;
- remote loopback can later be tunneled;
- scanning is bounded and ownership-aware.

### 0.9 product model

```go
type ServiceItem struct {
    ID         string
    ProjectID  string
    TabID      string
    PaneID     string
    Name       string
    Status     ServiceStatus
    PID        int
    Ports      []ListeningPort
    Command    string
    Source     ServiceSource // Script | Observed
}
```

Actions:

```text
Running Script/Service → Open Terminal
Stopped Script         → Start
Port                    → Open Preview
Stop                    → only when the Script service owns the process lifecycle
Observed process        → read-only
```

Do not invent process ownership for an observed port.

## 9. Scripts

The old Rust client owns Script semantic definitions only because Herdr has no protocol-native Script resource.

Preserve the same boundary in Go:

```text
Shardlane owns definition/presentation
Herdr owns Tab/Pane/PTY/process execution
```

0.9 should port:

- Script model/store;
- one-shot vs resident service;
- start/stop transaction;
- Project association;
- runtime reconciliation;
- port observation;
- New Task Script/Command affordances only where still useful.

Do not use `os/exec` as a shadow Project process manager for ordinary Script execution.

## 10. Lazygit

The old architecture already permits one bounded auxiliary tool process.

0.9 should implement Lazygit with the **official MyGo Terminal plugin**:

```text
selected Tab cwd
→ resolve Git root
→ locate/certify lazygit
→ spawn one auxiliary native terminal
→ destroy on panel close / Project switch / app close
```

Rules:

- at most one Lazygit auxiliary session per window;
- no per-Project background fleet;
- missing CLI is a blocking empty state with install guidance;
- old/outdated CLI may be shown as “update recommended” only if compatibility is measured;
- normal Herdr Terminal identity is never replaced by the Lazygit PTY.

## 11. Local Preview: preserve the product, change the migration implementation

Old Shardlane embedded a narrow local-only WebView in the Right Panel.

MyGo 0.2.7 still exposes a choice per Window:

```text
URL/Page (WebView window)
OR
Content (Native UI window)
```

There is no official Native-UI-embeddable WebView element in `ui`.

Therefore 0.9 must not recreate private platform embedding.

### Approved 0.9 fallback

Until MyGo ships an official embedded WebView:

```text
Right Panel Services/Port action
→ open/reuse project-scoped Preview Window
→ MyGo WebView Window
```

The Preview Window is still a Shardlane local-preview surface, not a general browser.

Scope:

- `localhost`, `127.0.0.1`, `[::1]` and explicitly verified local dev targets only;
- navigation outside preview scope opens the system browser;
- no arbitrary web search;
- no bookmark/history product;
- no privileged Go bindings exposed to preview content;
- browser-profile/cookie isolation is not claimed unless MyGo provides a supported page-profile API.

When official embedded WebView support exists, the same PreviewService may be rendered back inside the Right Panel.

## 12. Git Workspace Intelligence

### Core 0.9 — local Git facts

Migrate and strengthen old `git_status.rs`:

```text
Git root
branch
changed files
additions/deletions
ahead/behind
clean/dirty
last refresh
```

Use one UI-independent `GitStatusService` with stale-while-refresh behavior.

No Git commands from render/menu-open code.

### Sidebar/Header presentation

Keep it restrained:

```text
portal
main ↑2 ↓1 · +34 −8
```

Do not turn every row into a dashboard.

### Worktree operations

Herdr must remain owner for workspace/worktree lifecycle.

Only expose worktree create/open/remove actions when a verified Herdr protocol/CLI contract exists in the Go adapter.
No direct client-side checkout mutation is authorized by this audit.

## 13. GitHub PR Intelligence — useful, but optional

herdr-gpui demonstrates that PR readiness can greatly reduce context switching:

```text
PR number/state
review decision
checks
mergeability/conflict
additions/deletions
open on GitHub
```

Shardlane should adopt **read-only PR enrichment first**.

### Authentication boundary

Do not build a new token vault in 0.9 solely for this feature.

Preferred first adapter:

```text
GitHub official `gh` CLI detected + authenticated
→ read PR metadata
```

If unavailable:

```text
local Git intelligence still works
PR enrichment hidden/unavailable
```

No token is copied into Shardlane.

### No mutation in first slice

Initial PR actions:

```text
Open on GitHub
Copy PR URL
Refresh
```

Do not merge/approve/comment from Shardlane in 0.9 core.

## 14. Desktop Attention: complete the 0.5 + 0.7 loop

0.5 owns semantic notification eligibility.
0.7 owns StatusCenterSnapshot/menu-bar quick status.

0.9 should connect those to official MyGo desktop affordances:

```text
System Notification
Dock badge
optional Dock bounce for urgent NeedsAttention
Menu Bar quick panel
in-app marker/toast
```

### Badge

Use a single count derived from existing status semantics, for example:

```text
NeedsAttention + ReviewPending
```

Working does not increase the badge.

### Bounce

Only for a new urgent NeedsAttention transition while the app is inactive, and throttled.
Never bounce for every status event.

### Sound

Do not port a private/custom audio stack merely for parity.
MyGo 0.2.7 has no official audio plugin among its official plugins (`fetch`, `terminal`, `updater`, `websocket`).
Custom notification sound remains a deferred capability unless an approved native/platform API is selected later.

## 15. File Drop → Terminal

MyGo 0.2.7 officially supports file drops in Native UI windows and elements.
The terminal plugin supports semantic Paste.

0.9 should add local file-path drop to the target Terminal.

Rules inspired by herdr-gpui:

- target the Pane under the pointer, not merely the focused Pane;
- paste quoted paths only;
- never press Enter;
- cap path count and total pasted text;
- reject control characters;
- never read file contents for a simple path drop;
- remote file upload is not part of 0.9.

Initial recommended limits:

```text
256 paths
64 KiB quoted text
```

Windows quoting must not pretend POSIX quoting is correct. If the active shell semantics cannot be proven, disable path-drop there rather than paste unsafe text.

## 16. Terminal links

MyGo's official Terminal already contains URL recognition/opening support.

0.9 should verify packaged behavior and expose consistent affordances rather than adding another link parser.

Acceptance includes:

- Cmd-click/click behavior defined by the plugin;
- URL opens via default browser;
- no privileged local action encoded in arbitrary terminal text.

## 17. Terminal Find / Copy Mode

### Find

herdr-gpui uses a semantic Herdr scrollback API (`pane.copy_search`) rather than parsing rendered cells in the client.

Current Go Herdr adapter does not yet wrap that method.

Therefore 0.9 planning status is:

```text
CAPABILITY-GATED
```

First task must verify the installed target Herdr protocol and exact request/response contract.

If supported:

- add typed adapter;
- Native find bar;
- current/total match count;
- next/previous navigation;
- no typed query reaches Terminal input.

If unsupported:

- record protocol gap;
- do not implement a client-side VT/text-search shadow index.

### Copy Mode

Keyboard whole-scrollback copy mode is not a 0.9 core requirement.
Native selection/copy/paste parity is sufficient.

## 18. Diagnostics & Logs Center

The new client already has bounded structured logs, but users currently have no first-class surface to inspect them.

herdr-gpui's Logs window is a strong reference.

### 0.9 Diagnostics page/window

Show:

```text
App version/build
MyGo version
Go version
OS / architecture
Herdr CLI path/version/protocol
active Workspace/session
connection status
Integration health summary
History/Usage catalog status
log path / rotation policy
recent dropped-log count
```

### Log viewer

- newest bounded records, target 5,000;
- severity/component/operation filter;
- text search;
- copy selected row/details;
- export sanitized logs;
- live tail only while the view is open;
- no unbounded in-memory log accumulation.

### Diagnostic bundle

Optional explicit user action:

```text
Copy Diagnostics
Export Diagnostic Bundle
```

Bundle may contain:

```text
diagnostics.json
sanitized recent logs
configuration schema/version facts
```

It must not contain:

- terminal output;
- prompt/conversation bodies;
- History transcripts;
- API keys/tokens;
- environment dumps;
- provider secrets.

Home paths should be normalized/redacted where feasible.
Nothing is uploaded automatically.

## 19. Official MyGo Updater + What's New

Both reference clients invest heavily in update UX.
The old Rust Shardlane also has a substantial custom staged updater.

MyGo 0.2.7 now has an official Native updater plugin built on signed `mygo.Updater` releases.

0.9 should migrate to the official plugin rather than port the Rust curl/ditto/codesign state machine line-for-line.

Required UX:

```text
Check for Updates…
Update available
Release notes
Download/install progress
Install / Relaunch
Skip version / Later where plugin supports it
Automatic checks preference
Last checked
```

Policy:

- automatic checks may default on for packaged release builds;
- automatic install/download should remain explicit/user-controlled unless product policy changes;
- development/unconfigured builds show updates unavailable, not an error loop;
- use signed MyGo update configuration generated by the release pipeline;
- do not introduce Homebrew as a required runtime dependency.

## 20. Appearance: absorb the good parts, keep one product identity

herdr-gpui has many sidebar layouts. Shardlane should not copy nine layouts.

The user already chose a macOS-like, shadcn-influenced compact product language.

Adopt only orthogonal presentation preferences:

```text
Sidebar density: Compact / Default / Comfortable
High contrast: On/Off
Appearance: System / Light / Dark
```

One canonical Sidebar component tree remains.
Density changes spacing, not information ownership or hierarchy.

High contrast applies to Shardlane UI semantic colors only, never rewrites Terminal program colors.

## 21. Adapt Magpie Profiles as Shardlane Task Presets

Magpie Profiles snapshot every Agent's provider/model configuration.
Shardlane should **not** copy that behavior because current `StartAgentRequest` does not own provider model/effort configuration.

The safe adaptation is a Shardlane-owned **New Task Preset**:

```go
type TaskPreset struct {
    ID             string
    Name           string
    Provider       history.AgentID
    PromptTemplate string
}
```

Optional future fields may be added only when the canonical Agent launch capability owns them.

Do not store:

- provider API keys;
- provider config file mutations;
- model/effort guesses;
- Herdr runtime state.

UX:

```text
New Task
Presets: Review · Fix tests · Investigate
```

Selecting a preset fills the form; it does not launch automatically.

This is a useful Magpie-inspired convenience without changing runtime ownership.

## 22. Backup / Restore / Sync

Magpie's encrypted backup/sync system is useful but broad.

Do not put WebDAV/S3 sync into 0.9 core.

Potential later Shardlane-owned export scope:

```text
settings
shortcut preferences
appearance
task presets
non-secret local presentation state
```

Explicitly excluded by default:

```text
Herdr runtime/session state
provider-owned History
provider credentials
terminal scrollback
Conversation bodies
Git credentials
```

This belongs after 0.9 unless a later product decision promotes it.

## 23. Launch at Login / tray-only startup

Magpie makes this a polished menu-bar workflow.
It becomes relevant after 0.7.

However no high-level official MyGo launch-at-login API was found in the audited 0.2.7 surface.

Status for 0.9:

```text
DEFER / PLATFORM CAPABILITY GAP
```

Do not add private LaunchAgent/Registry/autostart code just to match Magpie without a separate decision.

## 24. General browser tabs — reject

herdr-gpui has sophisticated browser tabs and agent/browser skill integration.

Shardlane architecture explicitly says general-purpose browsing is a non-goal.

Keep only Project-rooted local Preview.

Do not add:

- browser tab strip;
- bookmarks/history product;
- generic remote browsing;
- browser annotations;
- arbitrary website automation surface.

## 25. Tabs in titlebar — reject

The newest herdr-gpui reference includes tabs-in-titlebar work.

Shardlane already has an explicit architectural decision:

> The Sidebar is the canonical Tab presentation owner.

Adding titlebar tabs would create two navigation owners and reintroduce the ambiguity the migration is removing.

Do not adopt it.

## 26. Remote devices / Teleport — defer

herdr-gpui's saved devices, SSH tunnels, remote file copy and Teleport are strong reference material for the later Remote/Platform milestone.

0.9 may design service interfaces so remote implementations can be added later, but it must not expand its scope into:

- SSH credential UX;
- remote Herdr install/update;
- remote file copy;
- remote workspace Teleport;
- cross-device account stores.

## 27. CPU / memory status — adapt, not chrome

herdr-gpui shows CPU/memory continuously.
Shardlane intentionally does not keep a permanent full-width status bar.

If useful, system/resource information belongs in:

```text
Diagnostics
Activity
Project Inspector
```

Do not introduce periodic CPU/memory sampling merely to decorate normal chrome in 0.9.

## 28. Adopt / Adapt / Defer / Reject summary

### ADOPT / MIGRATE in 0.9 core

- unified Command Center;
- Files;
- Services;
- Scripts;
- Lazygit;
- listening-port intelligence;
- local Git status;
- Dock badge/attention integration;
- local File Drop → Terminal;
- Diagnostics/Logs Center;
- official MyGo Updater + What's New;
- Sidebar density + high contrast;
- Task Presets.

### ADAPT with capability boundary

- Local Preview → separate project-scoped MyGo Preview Window until embedded WebView exists;
- GitHub PR readiness → read-only optional adapter, preferably `gh` CLI first;
- Terminal Find → only with verified Herdr semantic API;
- worktree actions → only through verified Herdr API;
- remote port tunnel → later Remote adapter.

### DEFER

- custom sound stack;
- WebDAV/S3 backup sync;
- launch at login;
- remote devices/Teleport/file copy;
- whole-scrollback keyboard Copy Mode;
- provider/model profile mutation.

### REJECT

- mandatory model gateway;
- provider credential ownership for Agent runtime;
- tabs in titlebar;
- general browser tabs;
- second Workspace/Project registry;
- render-time Git/network/filesystem commands;
- client-owned Project process manager.

## 29. Why 0.9 should be a large release

0.5–0.8 build the Agent semantic core.
0.9 is the first release where those capabilities become one complete desktop workflow.

The user-visible improvement is larger than any single new page:

```text
Before
I know an Agent is running, but I still leave the app for many development tasks.

After 0.9
I can find the Agent/Project/action instantly, inspect files/services/Git,
open the local app preview, react from notifications/menu bar,
and diagnose/update Shardlane without leaving the product.
```

That is a coherent large-version milestone.

## 30. Recommended version boundary after 0.9

Do not call 0.9 the final cutover.

Recommended remaining path:

```text
0.9  Workspace Intelligence & Desktop Experience
→ Remote/Mobile v2 convergence
→ cross-platform/package acceptance
→ 1.0 Final Cutover
```

1.0 should mean the Rust desktop path is no longer required for ordinary Shardlane use, not merely that the Native UI has many features.
