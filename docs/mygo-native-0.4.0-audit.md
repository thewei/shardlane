# Shardlane MyGo 0.4.0 — Operational Closure & UI System Audit

Status: **audited test build**
Audit date: 2026-10-05
Branch: `rewrite/mygo`
Architecture authority: `docs/client-product-architecture.md`
Scope plan: `docs/mygo-native-0.4.0-closure-plan.md`
Input audit: `docs/mygo-native-0.3.0-closure-audit.md`

## 1. Verdict

**0.4.0 delivers the operational-closure slice: both C0 correctness bugs are fixed with deterministic tests, MyGo is upgraded to 0.2.7, one Design System v2 owns metrics/status visuals, one shared operational status model drives every consumer, provider integration health is a real Native surface with strategy-gated actions, and the full Agent launch transaction — idempotency, uncertain-delivery reconciliation, readiness/identity gates, post-commit truthfulness — is ported and tested behind an injectable runtime seam. Start Agent remains disabled because the Herdr launch transport (verified `agent.start`/prompt/readiness protocol adapter) is not ported; the provider picker already reflects real integration health. Chat remains deferred by plan.**

## 2. Required status matrix

| Area | Status | Evidence |
|---|---|---|
| MyGo 0.2.7 | ✅ DONE | `go.mod` pinned v0.2.7; tidy/tests/build pass; render baseline re-verified; isolated interaction smoke launched clean |
| Design System v2 | ✅ DONE | `design_system.go` (typography/spacing/radius/control tokens + semantic StatusTone palette); `ds_components.go` primitives (appPage, pageHeader, sectionHeader, settingsCard, formRow, emptyState, loadingState, inlineNotice, statusGlyph, statusPill, providerBadge, agentRow, toolCard); History/Settings/New Task/Sidebar/Titlebar migrated |
| History latest-wins (CLOSURE-01) | ✅ DONE | cancel + generation latest-intent semantics; `TestHistoryListLatestRequestWins` (slow first request, changed filter, late stale result proven inert, `-race`) |
| History list virtualization | ✅ DONE | `ui.List` + persistent `ListState` (stable keys, selection, keyboard) |
| History detail scroll owner (CLOSURE-03) | ✅ DONE | `ui.Scroll` + `TrackScroll`; earlier→top / later→follow-end; anchor `ScrollIntoView`; `TestHistoryDetailScrollIntentsFollowPaging` |
| Operational status model | ✅ DONE | `operational.go`: normalize (OPS-01), OperationalSummary priority (OPS-02), shared glyph/pill (OPS-03); OPS-05 dynamic sequence test across consumers |
| Header/Sidebar dynamic status | ✅ DONE | Titlebar attention control (reveals Agents section) + reconnect action; Sidebar Agent rows + tree rows consume the shared model; no healthy-state chrome; no polling (OPS-05 test) |
| Integration/Hook health | ✅ DONE | strategy registry (INT-01, Rust-authority port), `herdr integration status` adapter/parser (INT-02), `IntegrationHealthService` states (INT-03), Native Providers page (INT-04), action lifecycle with spinner/disabled duplicate + fresh-audit reconcile + Toast (INT-05), legacy semantics mapped through the strategy authority (INT-06), route-entry/refresh/post-action cadence, no polling (INT-07) |
| Integration actions | ✅ DONE | strategy-gated Install/Update only (Herdr official + managed bridge targets); Deferred/screen-managed rows carry no action; `TestProvidersPageAuditsAndReconcilesActions` |
| Concurrent launch idempotency (CLOSURE-02) | ✅ DONE | single in-flight owner per RequestID, duplicates await/replay, conflicting body → typed conflict, no global mutex across launch, waiter cancellation cannot relaunch; barrier + conflict + waiter-cancel tests, `-race` clean |
| Full Agent launch (LAUNCH-01/02/03) | ✅ DONE | transaction phase machine + fake-runtime uncertain-delivery harness (7 deterministic tests incl. post-commit `AgentCreated` truthfulness and tab-reap rules); Herdr transport adapter ported against the **live bundled protocol schema** (`herdr api schema`, protocol 22): `agent.start`/`agent.prompt`/`agent.wait`/`agent.get`/`tab.create`/`workspace.create`/`pane.process_info`/`agent.send_keys` shapes, discriminators and result wrappers verified; contract tests run the adapter against a scripted unix socket (`TestLaunchTransport*`), and the shell-level E2E (`TestNewTaskLaunchEndToEnd`) drives Start Agent through the real transaction: structure created with `focus:false`, one rename, shell-ready gate, exactly one `agent.start`, identity verification, exactly one prompt, client-local navigation |
| Start Agent enabled (LAUNCH-05) | ✅ DONE | enabled for Ready providers through `LaunchService.StartNewTask` → `LaunchRegistry` (idempotent by draft operation id); success navigates client-locally to the created Tab; committed failures open the created Agent with truthful "Agent created, but …" copy; not-ready providers keep the picker states |
| Chat | ❌ NOT DONE (deferred) | 0.5 boundary per plan |
| Remote/Mobile | ❌ NOT DONE | Rust remains compatibility authority |
| Windows/Linux | ❌ NOT DONE | macOS arm64 only |

Additional notes:
- `ui.Form`/`Field` adopted in Settings (General/Terminal) and New Task (DS-05); the Workspace terminal toolbar is token-typed (DS-04).
- `ui.Outline` spike (DS-06): **PASS on parity** (client-local disclosure, selection, status pills, context menus — `TestOutlineSpikeParity`), production **NO-GO recorded for 0.4**: migrating mid-release couples `reconcileTreeExpansion` with a second expansion state machine; revisit as an early 0.5 task.
- New Task title heuristic is a simplified first-line derivation; the full `agent_titles.rs` quality heuristic is a separate port.
- Working-state anti-flicker (RENDER-05) is implemented as immediate rendering (underlying state was never delayed); the optional presentation delay is not added.

## 3. Verification performed

```sh
cd next
GOTOOLCHAIN=go1.27.1 go test ./...                                    # PASS (8 packages)
GOTOOLCHAIN=go1.27.1 go test -count=1 -race ./internal/agent/... ./internal/app/...  # PASS
GOTOOLCHAIN=go1.27.1 go tool mygo build                              # PASS
git diff --check                                                     # worktree PASS (see note)
git diff --cached --check                                            # flags whitespace-only EOF blank
                                                                     # lines inside docs the parallel
                                                                     # agent STAGED; fixing the index
                                                                     # requires staging rights
```

### Render evidence (RENDER-06)

`SHARDLANE_RENDER_SMOKE=<dir>` renders real frames from the presentation
code: workspace, History list, History detail, Settings General/Terminal/
Providers, New Task picker, and working/blocked/done operational statuses.
The Providers frame shows the strategy authority per row (Herdr official /
screen-managed / unsupported) with health pills and the gated Update action.

### Isolated real-app smoke (temporary HOME, seeded Codex source)

Launched `Shardlane.app` 0.4.0 via LaunchServices with a temporary `HOME`:
process alive, catalog indexed the seeded session, FTS populated, log
grep for the transcript body returned zero matches, clean shutdown; the
user's running instance was untouched.

### Forbidden-surface audit

- no WebView/WKWebView/xterm/React/Vite under `next/`;
- no `workspace.focus`/`tab.focus`/`pane.focus` navigation (negative tests pin exclusion);
- no global focus-event subscriptions (negative test pins exclusion);
- no `--takeover` (negative test pins it);
- `exec` sites: Herdr CLI discovery/session control, `herdr integration` audit/install (service layer, never render), the Codex permission-flag probe equivalent stays upstream — no render-path filesystem/subprocess/SQLite/RPC work (audited);
- runtime Agent status (Herdr projection) and integration/hook health (strategy + CLI audit) remain separate models.

## 4. Package

- DMG: `next/build/darwin-arm64/Shardlane 0.4.0.dmg` (~7.0 MB)
- SHA-256: `af60c0b2dac65b63dde05f48c1a5eb2ebb02a458b5a21baf93379433e2974029`
- version `0.4.0`, identifier `com.whstudio.shardlane.next`, macOS 13.0+, ad-hoc signature, not notarized. Internal test build; the Rust client remains the shipped default.

## 5. Remaining architectural blockers

1. **Live agent.start acceptance**: the transport is schema-verified and contract-tested against a scripted socket; a live end-to-end launch of a real provider CLI in an isolated runtime remains on the real-device acceptance checklist (provider auth makes it non-deterministic in CI).
2. **Provider enablement settings**: the capability projection currently treats every non-hidden provider as enabled; per-provider user enablement belongs to a later Settings slice.
3. **Full title heuristic**: `agent_titles.rs` quality port for rename labels (cosmetic).
4. **ui.Outline migration**: spike-passed; deferred to 0.5 to keep one expansion state machine during the release.
