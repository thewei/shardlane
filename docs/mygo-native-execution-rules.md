# MyGo Native Migration — Execution Rules

Status: **Active execution contract**
Architecture authority: `docs/client-product-architecture.md`
Roadmap/task authority: `docs/mygo-native-migration-roadmap.md`
Applies to: `next/` and all migration work replacing the Rust desktop/Host/History/Remote implementation.

## 1. Purpose

This document defines **how** the MyGo migration is executed. It does not redefine product/runtime ownership. If this document conflicts with `client-product-architecture.md`, the architecture document wins and this file must be corrected.

The migration optimizes for small independent changes, fast local feedback, explicit task evidence, parallel work, and one complete acceptance pass near cutover instead of expensive full validation after every task.

## 2. Non-negotiable ownership

- Herdr remains the sole authority for runtime Workspaces/Projects, Tabs, Panes/layouts, Agents, PTYs, terminal identities, scrollback, persistence, and process lifecycle.
- Go application/domain packages own Shardlane product semantics.
- MyGo Native UI owns presentation only.
- History is a read-only projection over external provider sources plus Shardlane-owned disposable index/cache.
- Remote/Mobile APIs reuse the same Go domain services; they do not create a second runtime.
- No client-owned PTY, Pane layout model, runtime registry, or provider process lifecycle.
- No automatic `terminal attach --takeover`.
- For the 0.10 Native target, Lazygit is removed. `/workspace` has one `WorkspacePrimarySurface` owner for Terminal / Diff Review / Commit; the contextual Right Panel is Changes / Files / Services and may not create a competing center-content owner.

## 3. Native UI policy

The current migration target is **Native UI first and Native UI only for product pages**.

Required Native UI surfaces:
- Sidebar / Titlebar / Toolbar;
- Workspace/Project/Tab/Pane navigation;
- Terminal;
- New Task;
- Search;
- History list;
- History detail;
- Chat / Conversation;
- Settings;
- Activity;
- Git Workbench / Files / Services / local preview chrome.

### WebView freeze

History UI and Chat UI MUST remain MyGo Native UI for the current migration.

Do not add:
- a Chat Web window;
- a History Web window;
- private AppKit/WKWebView embedding;
- a localhost HTTP backend for desktop presentation.

Web presentation may be reconsidered only when:
1. MyGo ships an official supported WebView element/surface that embeds cleanly in the Native UI architecture; and
2. a later explicit product decision unfreezes this rule.

Even then, Web presentation may call only the same Go application/domain services through official MyGo bindings/plugins and may never own runtime truth.

## 4. File and package boundaries

Target dependency direction:

```text
nativeui
   │
   ▼
app services
   │
   ├── history
   ├── agent
   ├── conversation
   ├── settings
   └── remote DTO/application seams
   │
   ▼
herdr adapter
   │
   ▼
Herdr
```

Rules:
- `internal/nativeui` must not become the application service layer.
- New UI code should call an app/domain service, not protocol methods directly, once that service exists.
- `internal/herdr` must not import MyGo UI.
- Domain packages must not import `mygo/ui`.
- Platform-specific code belongs under a platform seam, not in domain services.

### File size guidance

- target production file: < 250 LOC;
- > 300 LOC: review for extraction;
- > 450 LOC: split before adding more behavior;
- one file should have one primary responsibility;
- reusable appearance belongs in `theme.go`, `icons.go`, `components.go`.

## 5. Atomic task contract

Every implementation task must have:

1. **Task ID** — e.g. `HIST-04`.
2. **Single objective**.
3. **Inputs/owners** — old Rust module, fixture, protocol method, or service dependency.
4. **Output** — exact file/API/UI capability produced.
5. **Non-goals** — what the task deliberately does not solve.
6. **Fast verification** — command or deterministic assertion.
7. **Completion evidence** — focused tests / fixture parity / headless UI assertion.
8. **Dependencies** — other task IDs, if any.

Target task duration:
- normal: 30 minutes–4 hours;
- if likely > 4 hours, split before implementation;
- if a task crosses two domain boundaries, split it.

## 6. Verification tiers

### L1 — per task: mandatory, fast

Run only tests relevant to the changed capability.

Examples:

```sh
go test ./internal/history -run TestCatalog
go test ./internal/agent -run TestReadiness
go test ./internal/nativeui -run TestHistoryRoute
go test ./internal/remote -run TestBootstrapGolden
```

Always run:

```sh
git diff --check
```

Goal: seconds to low tens of seconds.

### L2 — milestone/local integration

At the end of a small milestone, run only affected packages:

```sh
go test ./internal/history/...
go test ./internal/nativeui -run 'History|Recent'
```

A small real-app smoke is allowed when the changed behavior cannot be established headlessly, but it must stay focused to 1–3 actions.

### L3 — final cutover only

Do not run after every task.

Final verification includes:
- `go test ./...`;
- `go tool mygo build`;
- real macOS application acceptance;
- terminal/IME/multi-Pane acceptance;
- History/Chat/New Task complete flows;
- Remote/Mobile contract suite;
- performance;
- packaging;
- Windows;
- Linux;
- soak/leak testing.

## 7. Test strategy

Prefer, in this order:

1. pure table-driven unit tests;
2. fixture/golden tests;
3. fake Herdr socket/server tests;
4. MyGo headless UI tests;
5. small isolated real-app smoke;
6. final real-device acceptance.

Do not make a task depend on full E2E if a lower level can establish correctness.

### Compatibility fixtures

Old Rust code is a behavior oracle until cutover.

Reuse:
- `crates/shardlane-remote/tests/fixtures`;
- Rust History adapter test fixtures;
- Rust Host transaction semantics;
- mobile Zod fixtures/contracts.

Do not transliterate implementation line-by-line. Pin externally observable behavior.

## 8. Performance rules

- hidden surfaces do no expensive presentation work;
- work scales with visible projection, not full retained source size;
- History transcript presentation is bounded;
- obsolete History/page requests are cancellable;
- event bursts reconcile with quiet-window + bounded maximum latency where required;
- no fixed high-frequency polling over asynchronous sources;
- no-op state changes remain no-op;
- logging is bounded and must not become hot-path blocking IO.

## 9. Logging contract

Application logs use `internal/applog`.

Default:
- active file: 2 MiB;
- backups: 3;
- approximate max on disk: 8 MiB;
- format: structured JSON;
- default level: info;
- debug: `SHARDLANE_LOG_LEVEL=debug`.

Allowed:
- operation name;
- runtime IDs;
- counts;
- durations;
- error metadata;
- connection/reconciliation lifecycle.

Forbidden:
- terminal bytes/output;
- user prompt text;
- conversation bodies;
- credentials;
- auth headers;
- provider secrets;
- environment dumps.

## 10. Git/task hygiene

- preserve user/staged changes;
- no reset;
- no staging/commit/push unless explicitly requested;
- one implementation commit should normally correspond to one Task ID;
- avoid commit messages like `continue migration` / `misc fixes`.

Preferred:

```text
feat(history): HIST-04 parse Claude sessions
feat(nativeui): HIST-10 render history route
feat(agent): AGENT-05 add readiness gate
test(remote): API-15 pin golden fixtures
```

## 11. Parallel development lanes

### Lane A — Native UI
Shell → Settings UI → History UI → New Task UI → Chat UI → Tools.

### Lane B — Core Domain
Settings → History → Agent Launch → Conversation.

### Lane C — Compatibility
Remote API v2 → golden fixtures → Mobile contracts.

### Lane D — Platform
transport/platform seams → Windows/Linux → packaging.

UI and domain can run in parallel after service interfaces are frozen.

## 12. Task completion checklist

A task is DONE only if:

- [ ] objective implemented;
- [ ] focused verification passes;
- [ ] behavior ownership remains correct;
- [ ] no second runtime/state authority introduced;
- [ ] no new oversized file;
- [ ] logs follow privacy/bounds contract;
- [ ] `git diff --check` passes;
- [ ] roadmap status/evidence is updated when the task is part of the active migration ledger.

## 13. What to postpone

Until final cutover, do not repeatedly spend time on:
- full DMG packaging for every task;
- full app acceptance for every task;
- pixel-perfect sweep after every small change;
- all-platform real-device testing after every task;
- deleting Rust references early;
- speculative abstractions;
- WebView presentation for Chat/History;
- rewriting Herdr terminal/runtime responsibilities.

## 14. Final cutover rule

Rust GUI/Host/History/Remote compatibility implementations are removed only after:
1. MyGo client is feature-parity or has explicit product removals;
2. contract compatibility passes;
3. macOS final acceptance passes;
4. promised Windows/Linux support gates pass;
5. performance/soak gates pass;
6. MyGo becomes the default client.
