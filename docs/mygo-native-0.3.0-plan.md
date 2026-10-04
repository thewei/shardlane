# Shardlane MyGo 0.3.0 — Native History & Preferences Plan

Status: **Approved-next proposal**
Target version: `0.3.0`
Depends on: MyGo `0.2.0` foundation audit passing
Architecture authority: `docs/client-product-architecture.md`
Execution rules: `docs/mygo-native-execution-rules.md`

## 1. Version goal

0.3.0 should turn the current Native Foundation build into the first build with a complete non-terminal product workflow:

> **Persist preferences and browse real coding-agent History end-to-end using Native UI, while keeping Herdr runtime ownership unchanged.**

0.3.0 is intentionally **not** the Chat release and **not** the final Agent launch release.

This scope is independently valuable, read-only with respect to provider data, and can be verified without introducing risky process-control semantics before the History/domain foundation is stable.

## 2. User-visible requirements

### 2.1 Settings become real

General:
- Appearance: System / Light / Dark.
- Restore window state toggle.

Terminal:
- font family;
- font size;
- line height;
- scrollback limit;
- Option-as-Alt.

Shortcuts:
- display current default shortcut bindings;
- persistence model ready for customization;
- actual custom rebinding may remain disabled if MyGo shortcut APIs do not safely support dynamic overrides.

Behavior:
- preferences survive relaunch;
- corrupt config never prevents app startup;
- terminal presentation settings apply only to future/recreated native terminal views;
- no setting changes Herdr terminal identity, PTY, Pane layout, or runtime state.

### 2.2 Native History list

Route: `/history`

Requirements:
- real provider-derived sessions;
- search;
- project scope;
- provider filter;
- updated-at order;
- title / provider / project / timestamp / short description;
- loading / empty / error states;
- bounded result list;
- no transcript parsing merely to render Sidebar Recent.

### 2.3 Native History detail

Route: `/history/{id}`

Requirements:
- read-only;
- normalized user/assistant/system messages;
- native tool-call cards;
- thinking collapsed by default;
- bounded transcript materialization: max 60 messages;
- 12-message overlap for earlier/later paging;
- long message preview bound: 1,800 chars / 18 lines;
- long thinking preview bound: 700 chars / 10 lines;
- cancellation when switching conversation;
- no WebView;
- no provider source mutation.

### 2.4 Sidebar Recent becomes real

Requirements:
- fed from HistoryService metadata;
- no transcript parse;
- no duplicate runtime authority;
- opening Recent routes to `/history/{id}`;
- bounded item count.

### 2.5 New Task remains safe

The current New Task form stays visible, but Start Agent remains disabled in 0.3.0 unless the complete launch transaction lands behind all required tests.

Permitted 0.3 prework:
- provider capability model;
- StartAgent request DTO;
- idempotency design/tests;
- uncertain-delivery test harness.

Do not enable launch with a partial transaction.

### 2.6 Chat remains Native-only and deferred

No Chat WebView work in 0.3.0.

Allowed:
- UI-independent Conversation model design if needed by History continuation.

Not allowed:
- fake Chat data;
- PTY typing as semantic prompt transport;
- generic prompt fallback for blocked interaction;
- WebView implementation.

## 3. Architecture

```text
MyGo Native UI
│
├─ Settings pages
├─ History list
├─ History detail
└─ Sidebar Recent
     │
     ▼
internal/app
├─ SettingsService
└─ HistoryService
     │
     ├───────────────┐
     ▼               ▼
internal/settings  internal/history
                     │
                     ├─ Provider adapters
                     ├─ Scanner
                     ├─ SQLite catalog / FTS
                     └─ page-addressable transcript cache
```

Important boundaries:
- `nativeui` does not open SQLite or provider files directly.
- `history` does not import MyGo.
- `settings` does not import MyGo.
- `app` coordinates services and cancellation only.
- History remains read-only to external provider files.
- Herdr is not involved in static History reads except when mapping live Agent/session identity for future continuation.

## 4. Storage design

### 4.1 Settings

Use the already implemented `internal/settings.FileStore`.

Main application resolves the MyGo user-data directory and injects the Store into the application service layer.

Rules:
- one versioned JSON settings file;
- atomic temp-file + rename;
- file mode 0600 on Unix;
- corrupt file → defaults + visible/logged recoverable error;
- future schema → safe refusal + defaults, never destructive rewrite.

### 4.2 History catalog

Use SQLite as a disposable Shardlane-owned derived index/cache.

Recommended Go driver: a cross-platform, non-CGo driver unless measured build/runtime constraints justify another choice.

Schema should preserve current Rust behavior, not its implementation shape:

```text
sessions
- key PK
- id
- agent
- title
- project_path
- project_key
- project_name
- file_path
- created_at
- updated_at
- message_count
- size_bytes
- git_branch
- model
- tokens_used
- archived
- source
- description
- source_identity fields

transcript_page_meta
transcript_page_cache
transcript_message_index
FTS tables for session/message search
```

External provider files are never modified.

## 5. History provider scope for 0.3.0

### Required
- Claude Code.
- Codex.

### Optional if completed without delaying Native History MVP
- Gemini.

### Deferred
Other provider adapters can land task-by-task after the service/catalog contracts are stable.

The provider list in `history.AgentID` remains complete so unknown/unported providers fail explicitly rather than being misidentified.

## 6. Scanner and cache behavior

### Scan

```text
resolve configured/default roots
→ list session refs
→ compare source identity (mtime/size/path/native id)
→ parse changed source once
→ update session metadata + FTS
→ write page cache
→ drop full parsed transcript
```

Rules:
- no repeated parse of the same changed source for index + cache;
- missing provider root = legitimate empty source, not fatal;
- unreadable existing root = scan error; do not destructively clean catalog from incomplete observation;
- scan work runs off presentation path;
- UI receives immutable/bounded DTOs;
- scan cancellation supported on app shutdown/reconfiguration.

### Page cache

Contract:
- fixed page size: 64 normalized messages;
- UI window: max 60 messages;
- overlap: 12;
- `seq → message_index` lookup;
- source identity invalidates metadata/pages/index together.

## 7. Application services

### 7.1 SettingsService

Suggested interface:

```go
type SettingsService interface {
    Current() settings.Settings
    Update(ctx context.Context, mutate func(*settings.Settings) error) (settings.Settings, error)
}
```

Responsibilities:
- load once at startup;
- validate;
- serialize updates;
- persist;
- publish presentation-safe updated snapshot.

### 7.2 HistoryService

Suggested interface:

```go
type HistoryQuery struct {
    ProjectKey string
    Provider   history.AgentID
    Search     string
    Limit      int
}

type ConversationWindowRequest struct {
    ConversationID string
    AnchorSeq      *int64
    StartIndex     *int
    Limit          int
}

type HistoryService interface {
    List(ctx context.Context, q HistoryQuery) ([]history.SessionSummary, error)
    Recent(ctx context.Context, limit int) ([]history.SessionSummary, error)
    Open(ctx context.Context, req ConversationWindowRequest) (history.TranscriptWindow, error)
    Search(ctx context.Context, query string, projectKey string, limit int) ([]history.SearchHit, error)
}
```

No MyGo types in the interface.

## 8. Native UI design

### 8.1 History list

Layout:
- page title + search;
- compact filter row;
- scrollable list;
- provider icon/badge;
- title;
- project;
- relative/absolute updated timestamp;
- one-line description.

Interaction:
- click row → detail route;
- Cmd+F focuses History search while on History route if practical;
- Back/Forward use Router history;
- preserve list scroll/search when returning from detail where MyGo Router state supports it.

### 8.2 History detail

Native blocks:
- user message;
- assistant message;
- system/meta block;
- tool call/result;
- thinking disclosure;
- paging controls.

Do not emulate browser Markdown layout. Use Native UI typography and a small normalized block model.

If MyGo text primitives cannot render one Markdown construct, degrade that construct to readable plain/native text in 0.3.0 rather than introducing WebView.

## 9. Atomic task plan

### Settings lane

| ID | Scope | Verify |
|---|---|---|
| SET-03 | SettingsService load/update | service unit tests |
| SET-04 | inject user-data FileStore from main | temp path/integration test |
| SET-05 | General Native UI controls | headless UI |
| SET-06 | Terminal Native UI controls | headless UI |
| SET-07 | apply terminal presentation settings | terminal-option mapping test |
| SET-08 | preferences survive restart | isolated service/file integration |

### History core lane

| ID | Scope | Verify |
|---|---|---|
| HIST-05 | Codex adapter | Rust fixture parity |
| HIST-06 | optional Gemini adapter | Rust fixture parity |
| HIST-07A | catalog schema/migration | temp SQLite tests |
| HIST-07B | session upsert/source identity | catalog tests |
| HIST-07C | page cache + seq index | bounded random-window tests |
| HIST-08 | FTS/search | fixture search parity |
| HIST-09A | scanner | changed/unchanged/missing-root tests |
| HIST-09B | HistoryService list/recent | service tests |
| HIST-09C | HistoryService open window | 60-message bound tests |

### Native History lane

| ID | Scope | Verify |
|---|---|---|
| HIST-10A | Native History list shell | headless UI |
| HIST-10B | loading/error/empty/filter states | headless UI |
| HIST-11 | presentation block/window model | pure tests |
| HIST-12A | Native detail message blocks | headless UI |
| HIST-12B | earlier/later paging | headless + service tests |
| HIST-12C | cancellation on conversation switch | deterministic async test |
| HIST-14 | Sidebar Recent from service metadata | headless UI |

### Agent prework lane

| ID | Scope | Verify |
|---|---|---|
| AGENT-01 | provider capability model | fixture/table tests |
| AGENT-02 | StartAgent request/result DTO | serialization/unit tests |
| AGENT-03 | idempotency contract | fake runtime transaction tests |
| AGENT-06A | delivery-uncertain harness | fake socket test |

Do not enable Start Agent in 0.3.0 unless AGENT-01..06 full launch path is finished.

## 10. Verification strategy

Per task:
- focused package/test only;
- `git diff --check`.

Per milestone:
- affected package tests;
- headless Native UI tests.

Before 0.3.0 package:
```sh
cd next
GOTOOLCHAIN=go1.27.1 go test ./...
GOTOOLCHAIN=go1.27.1 go tool mygo build
```

Then:
- isolated app launch with fake Herdr;
- real History source smoke using copied/read-only fixtures;
- real app manual History list/detail;
- relaunch Settings persistence;
- ensure no WebView process/surface was added;
- verify logs contain no transcript bodies.

## 11. Performance acceptance

0.3.0 must meet:

- History list query: target < 50 ms on warm local catalog for typical query.
- cached 60-message window read: target < 50 ms regression ceiling.
- Native History detail materializes ≤ 60 messages.
- no full transcript held in UI state.
- switching conversations cancels obsolete request/result application.
- Sidebar Recent does not parse transcript files.
- no continuous polling when idle.
- event watcher retains 35 ms quiet / 140 ms max reconciliation behavior.
- no regression of idle terminal visibility behavior.

## 12. Definition of done

0.3.0 is DONE when:

- [ ] Settings load/save is wired to real Native UI.
- [ ] terminal appearance settings apply without changing Herdr ownership.
- [ ] Claude + Codex History appear in real Native History list.
- [ ] History search works.
- [ ] Native History detail opens real normalized transcript.
- [ ] transcript window remains bounded.
- [ ] Sidebar Recent uses HistoryService metadata.
- [ ] provider source files are never mutated.
- [ ] no Chat/History WebView exists.
- [ ] full Go tests pass.
- [ ] MyGo build succeeds.
- [ ] isolated startup smoke passes.
- [ ] manual Native History smoke passes.
- [ ] package remains an internal beta; Rust client is not removed.

## 13. Explicitly deferred to 0.4+

- full Agent launch enablement if transaction not complete;
- Native Chat;
- Conversation live projection;
- interaction resolution;
- follow-up queue;
- context transfer/live handoff;
- full Remote/Mobile v2 Go replacement;
- Windows/Linux production packaging;
- Rust client deletion.
