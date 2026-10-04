# Shardlane MyGo 0.2.0 — Code & Package Audit

Status: **audited test build**
Audit date: 2026-10-05
Branch: `rewrite/mygo`
Architecture authority: `docs/client-product-architecture.md`
Execution rules: `docs/mygo-native-execution-rules.md`

## 1. Verdict

**0.2.0 is a valid Native Foundation / Terminal test build. It is not the completed herdr-client migration.**

The current implementation meets the intended architecture for the migrated shell/runtime slice:

- MyGo Native UI is the desktop shell.
- No React/Vite/WebView/xterm dependency exists in `next/`.
- History and Chat remain Native-UI-only by policy; no private WebView embedding was introduced.
- Herdr remains the runtime/PTY/layout/process authority.
- visible terminals use Herdr `terminal attach <terminal_id>`; Shardlane owns no PTY.
- Shardlane Project/Tab/Pane navigation is client-local and no longer emits `workspace.focus`, `tab.focus`, or `pane.focus`.
- Herdr global focus events are intentionally not subscribed as shell-navigation drivers.
- terminal geometry is derived from Herdr authoritative layouts for the locally selected Tab.
- event bursts use quiet-window + bounded max-latency reconciliation.
- application logging remains bounded and does not log prompt/terminal/conversation contents.

The build is appropriate for testing the migrated shell, runtime projection, Sidebar/Search, terminal attach, window behavior, basic structural actions, and navigation semantics.

It is **not** appropriate for judging final product parity because History, Settings persistence integration, New Task launch, and Chat are not complete.

## 2. Audit matrix

| Area | Result | Evidence / current state |
|---|---|---|
| MyGo Native shell | ✅ PASS | Native `ui.View`, no Web shell |
| Native Terminal | ✅ PASS | official MyGo Terminal + Herdr direct attach |
| Herdr runtime authority | ✅ PASS | no client PTY/runtime/layout authority |
| local Project/Tab/Pane selection | ✅ PASS | `selectedProjectID/selectedTabID/selectedPaneID`; focus RPCs removed |
| external global-focus isolation | ✅ PASS | regression test: Herdr focus churn does not steal local selection |
| event subscription ownership | ✅ PASS | global focus events excluded |
| event-storm reconciliation | ✅ PASS | 35 ms quiet window + 140 ms max latency |
| Sidebar hierarchy | ✅ PASS for current shell milestone | New Task / Search / History / Agents / Recent / Projects / Workspace / Settings |
| Project/Tab expand state | ✅ PASS | client presentation state, multiple Projects may remain expanded |
| Search | ✅ PASS for runtime projection | Project/Tab/Pane/Agent search, local navigation |
| structural actions | 🟡 PARTIAL | create/rename/close/split/zoom paths exist; full parity/move/reorder acceptance remains |
| Settings storage core | ✅ PASS | Go Store + atomic JSON FileStore + schema/corruption tests |
| Settings UI integration | ❌ NOT DONE | pages are explanatory placeholders; Store not injected/applied |
| History domain models | ✅ PASS | Go normalized models + provider IDs |
| History source locator | ✅ PASS | measured Herdr `id/path` semantics, fail-closed unknown kinds |
| Claude History parser | ✅ PASS | Rust fixture behavior ported for mainline/thinking/tool result |
| History catalog/search | ❌ NOT DONE | no Go SQLite/FTS/catalog service yet |
| Native History list/detail | ❌ NOT DONE | current pages are explicit placeholders |
| Sidebar Recent real data | ❌ NOT DONE | presentation model exists; no HistoryService feed |
| New Task UI | 🟡 SHELL ONLY | form exists; Start Agent intentionally disabled |
| Agent launch transaction | ❌ NOT DONE | provider policy/readiness/idempotency/uncertain-delivery not ported |
| Chat / Conversation UI | ❌ NOT DONE | no product route/service yet |
| Remote/Mobile v2 | ❌ NOT DONE | Rust remains compatibility authority |
| Windows/Linux | ❌ NOT DONE | current package is macOS arm64 only |
| signing/notarization | 🟡 TEST BUILD | ad-hoc signed, not notarized |

## 3. Important audit corrections made before packaging

### 3.1 Client-local navigation

Previous MyGo code still called Herdr global focus RPCs for Sidebar/Search/Terminal navigation. That violated the canonical multi-client architecture.

Corrected model:

```text
Herdr snapshot focused_*     = bootstrap/fallback fact only
Shardlane selected*          = this window's semantic selection
Herdr session layouts        = authoritative geometry/topology
```

Removed from the MyGo client:
- `Manager.FocusProject`
- `Manager.FocusTab`
- `Manager.FocusPane`
- shell calls to `workspace.focus`
- shell calls to `tab.focus`
- shell calls to `pane.focus`
- subscriptions to `workspace.focused`
- subscriptions to `tab.focused`
- subscriptions to `pane.focused`

Structural mutations remain Herdr-owned. A user-initiated create/split may explicitly adopt the authoritative mutation result, but ordinary snapshots never steal local navigation.

### 3.2 Event burst handling

Previous watcher behavior was approximately:

```text
event
→ sleep 35 ms
→ session.snapshot
→ repeat for next event
```

That could cause repeated snapshots during event storms.

Current behavior:

```text
first dirty event
→ collect until 35 ms quiet
→ force reconciliation no later than 140 ms
→ one session.snapshot
```

Pane membership changes still restart the pane-scoped event subscription after reconciliation.

### 3.3 Cross-instance local selection reset

Switch/create/delete Workspace paths now clear local Project/Tab/Pane selection when instance identity changes, preventing a coincidentally identical runtime ID in another Herdr session from reusing stale client selection.

## 4. Verification performed

### Go test suite

```sh
cd next
GOTOOLCHAIN=go1.27.1 go test ./...
```

Result: PASS.

Packages passing:
- `internal/applog`
- `internal/herdr`
- `internal/history`
- `internal/nativeui`
- `internal/settings`

### Diff hygiene

```sh
git diff --check
```

Result: PASS.

### Production-style MyGo build

```sh
cd next
GOTOOLCHAIN=go1.27.1 go tool mygo build
```

Result: PASS.

Artifacts:
- `next/build/darwin-arm64/Shardlane.app` — about 14.4 MB
- `next/build/darwin-arm64/Shardlane 0.2.0.dmg` — about 5.6 MB

### Isolated real `.app` launch smoke

A temporary HOME and fake Herdr protocol-22 runtime were used. The test did not touch the user's real Herdr data.

Verified chain:
1. app launch through macOS LaunchServices;
2. `herdr session list --json`;
3. Herdr socket ping;
4. `session.snapshot`;
5. `events.subscribe`;
6. native terminal child command `terminal attach term-1`;
7. application process remained alive.

Result: PASS.

## 5. Test package

Primary local test package:

```text
/Users/wilson/Downloads/Shardlane-0.2.0-mygo-arm64.dmg
```

Build-tree copy:

```text
next/build/darwin-arm64/Shardlane 0.2.0.dmg
```

SHA-256:

```text
636dd52b064c4bd403f0e86b5add1e0cf3adccb65bec06c80595b6d7e8e02459
```

Bundle:
- version: `0.2.0`
- identifier: `com.whstudio.shardlane.next`
- target: macOS arm64
- minimum macOS: 13.0
- signature: ad-hoc test signature
- notarization: not performed

## 6. What to test manually in 0.2.0

Focus manual testing on the implemented slice:

1. launch/relaunch and window restoration;
2. Workspace switch/create/rename/delete;
3. Sidebar Project/Tab/Pane hierarchy;
4. expand multiple Projects without focus stealing;
5. switch Projects/Tabs/Panes and confirm only this Shardlane window changes;
6. Search → open Project/Tab/Pane/Agent;
7. native terminal typing, resize, scrollback, copy/paste;
8. multi-Pane split and resize;
9. zoom/unzoom;
10. rename/close Project/Tab/Pane;
11. fullscreen and rapid Tab/Pane switching;
12. CJK/IME candidate interaction;
13. disconnect/reconnect behavior;
14. logs remain bounded and contain no terminal/prompt content.

Do **not** treat the following as 0.2.0 failures:
- History showing a migration placeholder;
- Settings not yet persisting UI choices;
- Start Agent being disabled;
- Chat route not existing.

Those are explicitly 0.3+ work.

## 7. Release recommendation

**Recommendation: distribute 0.2.0 only as an internal test build.**

Do not replace the current Rust client yet.

The next version should complete one coherent daily-use workflow rather than adding more shell chrome. The recommended 0.3.0 scope is defined in `docs/mygo-native-0.3.0-plan.md`.
