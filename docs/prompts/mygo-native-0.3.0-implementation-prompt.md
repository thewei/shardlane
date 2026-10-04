# Shardlane MyGo 0.3.0 Implementation Prompt

You are continuing the Shardlane/herdr-client MyGo migration in:

`/Users/wilson/Workspaces/wh-studio/herdr-client`

Use the existing Devspace checkout/workspace if one is already open. Work on branch `rewrite/mygo`. Do not create another migration architecture.

## Required reading before changing code

Read, in this order:

1. `AGENTS.md`
2. `CLAUDE.md`
3. `.agents/skills/herdr-client-development/SKILL.md`
4. `docs/client-product-architecture.md`
5. `docs/mygo-native-execution-rules.md`
6. `docs/mygo-native-migration-roadmap.md`
7. `docs/mygo-native-0.2.0-audit.md`
8. `docs/mygo-native-0.3.0-plan.md`
9. `docs/performance-engineering.md`
10. `next/CLAUDE.md`

Treat `docs/client-product-architecture.md` as the sole architecture authority.

## Non-negotiable architecture

- Shardlane is the product/client; Herdr is the sole runtime authority.
- Herdr owns runtime Workspaces/Projects, Tabs, Panes/layouts, Agents, PTYs, terminal IDs, scrollback, persistence, and process lifecycle.
- MyGo Native UI owns presentation only.
- Go application/domain services own product semantics.
- Do not add a client PTY, client Pane layout authority, or second runtime registry.
- Do not use `terminal attach --takeover` automatically.
- Project/Tab/Pane navigation is client-local.
- Herdr `focused_*` fields are bootstrap/fallback facts only.
- Do not call or reintroduce `workspace.focus`, `tab.focus`, or `pane.focus` for ordinary Shardlane navigation.
- Do not subscribe to global focus events as navigation drivers.
- Herdr `session.snapshot.layouts` remains the geometry/topology authority for visible terminals.
- History UI and Chat UI MUST remain Native UI.
- Do not add React, Vite, xterm, WKWebView/private WebView embedding, a separate Chat/History Web window, or a localhost presentation backend.
- WebView work stays frozen until MyGo officially supports a Native-UI-embeddable WebView and a later explicit product decision unfreezes it.

## Current verified baseline

0.2.0 already has:
- MyGo 0.2.5 Native main window;
- native Router;
- Sidebar/Titlebar/Toolbar/dialogs;
- runtime Workspace/Project/Tab/Pane projection from Herdr protocol 22;
- local semantic selection;
- Herdr global-focus isolation;
- native direct terminal attach;
- multi-Pane geometry from Herdr layouts;
- Search;
- bounded structured logging;
- event burst reconciliation with 35 ms quiet window and 140 ms max latency;
- Settings Store + atomic JSON FileStore;
- History domain models;
- History project-key normalization;
- History source locator;
- Claude History parser.

All current Go tests passed before this handoff.

Do not regress these behaviors.

## 0.3.0 objective

Implement the approved scope in `docs/mygo-native-0.3.0-plan.md`:

> Persist real Native UI preferences and deliver real read-only Native History list/detail using Go services, without adding WebView or changing Herdr runtime ownership.

## Priority order

### Phase 1 — Settings integration

Implement:
1. SettingsService over `internal/settings.Store`.
2. Resolve MyGo user-data path in `main.go` and inject the FileStore; do not let `nativeui` resolve filesystem paths itself.
3. Wire General settings Native UI.
4. Wire Terminal settings Native UI.
5. Apply terminal presentation settings to newly created/recreated MyGo Terminal views.
6. Preserve Herdr terminal ID/PTY ownership.
7. Add persistence/relaunch tests.

Do not invent app-global mutable config in Native UI.

### Phase 2 — History core

Implement:
1. Codex adapter with Rust fixture parity.
2. Gemini adapter only if it does not delay the required Claude+Codex MVP.
3. Shardlane-owned disposable SQLite catalog.
4. source identity tracking;
5. session metadata/list path;
6. FTS/search;
7. fixed 64-message transcript page cache;
8. seq → normalized message index;
9. changed-source scanner that parses each changed source once;
10. HistoryService.

History is read-only to provider source files.

Use a cross-platform non-CGo SQLite approach unless you have measured evidence that another choice is required.

Do not access SQLite/provider files from `internal/nativeui`.

### Phase 3 — Native History UI

Implement:
1. real `/history` list;
2. search/project/provider filters;
3. loading/empty/error states;
4. real `/history/{id}` detail;
5. Native message/tool/thinking blocks;
6. max 60 materialized messages;
7. 12-message paging overlap;
8. bounded long-message/thinking preview;
9. obsolete-load cancellation;
10. Sidebar Recent from HistoryService metadata only.

Do not parse full transcripts merely to render Recent.

Do not introduce WebView for Markdown. If a Markdown feature cannot be represented cleanly by current MyGo Native UI, render a readable Native/plain-text degradation and document the gap.

### Phase 4 — Agent launch prework only

If time remains after Settings + History MVP:
- provider capability model;
- StartAgent request/result DTO;
- idempotency contract;
- uncertain-delivery fake-socket harness.

Do NOT enable the Start Agent button until provider policy, readiness, idempotency, and uncertain-delivery reconciliation are all implemented as one complete transaction.

Do not start Native Chat in this version.

## Implementation discipline

Use atomic Task IDs from the roadmap.

Each task must:
- have one objective;
- normally fit 30 minutes–4 hours;
- use focused verification;
- preserve user/staged changes;
- avoid staging/committing/pushing unless explicitly asked;
- update roadmap evidence/status when complete.

Prefer:
1. pure Go unit tests;
2. fixtures/golden tests;
3. fake Herdr socket tests;
4. MyGo headless UI tests;
5. focused real-app smoke.

Do not run full build/E2E after every task.

Run `git diff --check` after task changes.

Only near the 0.3.0 package gate run:

```sh
cd next
GOTOOLCHAIN=go1.27.1 go test ./...
GOTOOLCHAIN=go1.27.1 go tool mygo build
```

Then perform an isolated app smoke and a focused real History/Settings smoke.

## Performance constraints

Preserve:
- event watcher: quiet-window + bounded max latency;
- event-driven idle behavior;
- visible-only presentation work;
- no expensive hidden transcript rendering;
- bounded History windows;
- cancellable obsolete History tasks;
- no-op updates remain no-op;
- bounded structured logs.

History:
- page cache fixed at 64 normalized messages;
- UI ≤ 60 messages;
- 12-message overlap;
- long body default ≤ 1,800 chars / 18 lines;
- thinking default ≤ 700 chars / 10 lines;
- Recent must be metadata-only;
- never keep the full transcript in Native UI state.

## Logging/privacy

Allowed:
- operation names;
- stable IDs;
- counts;
- durations;
- error metadata.

Never log:
- terminal output;
- prompt text;
- conversation bodies;
- provider secrets;
- credentials;
- auth headers;
- environment dumps.

## Required final audit for this implementation cycle

Before handing back:
1. run full Go tests;
2. run `git diff --check`;
3. build the MyGo app/DMG;
4. verify there is no React/Vite/WebView/xterm dependency in `next/`;
5. verify no global Herdr focus RPC was reintroduced;
6. verify History/Settings are real, not placeholder rows;
7. verify Start Agent remains disabled unless the complete transaction is ready;
8. provide a concise matrix of DONE / PARTIAL / NOT DONE;
9. provide the exact DMG path + SHA-256;
10. do not claim the Rust client can be removed.

Proceed with implementation without asking for confirmation. If a non-safety ambiguity appears, choose the option that best preserves the canonical architecture and record the assumption in the roadmap rather than stopping.
