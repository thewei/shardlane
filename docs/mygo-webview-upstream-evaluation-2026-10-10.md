# MyGo Native UI × WebView — 2026-10-10 upstream decision record

Status: **Investigated / not adopted. Superseded 2026-10-11**: MyGo v0.4.1 shipped the official embedded `ui.WebView` (gate 1 met, cross-platform backends, docs, tests), and the product owner approved Chat-first adoption — recorded in `docs/client-product-architecture.md` (Decision 2026-10-11) and roadmap N19. The constraints below that survive the decision: presentation-only bridge, app-content origin only, no localhost HTTP backend, no private embedding, real-app acceptance before retiring the native Chat. This is framework capability reconnaissance, not permission to unfreeze Shardlane Chat/History WebView presentation.

## Current verified facts

- Shardlane uses MyGo **v0.3.7** (the upstream release of 2026-10-09 UTC), with the two local deltas documented in `third_party/mygo/LOCAL-PATCH.md`. The upstream release contains **no official `ui.WebView` for embedding HTML inside `WindowOptions.Content`**.
- Official whole-window WebView presentation exists: a `WindowOptions.URL` page uses the system engine. It is not the same as an element nested among MyGo Native UI components.
- Three **unmerged** implementations exist. The upstream API is not settled; none should be treated as a stable first-party dependency.

| Candidate | API and scope | Evidence and gaps |
|---|---|---|
| [#60](https://github.com/egoist/mygo/pull/60) | `Window.NewWebView` plus `ui.WebView(c, view)`, macOS/Windows/Linux | CI success for the three platforms and Windows ARM64 at inspection; documents that overlays paint behind hosted web content, scrolling clips are unsupported, and native page capture excludes the web view. No maintainer review/merge. |
| [#189](https://github.com/egoist/mygo/pull/189) | Declarative `ui.WebView`, load HTML/URL, callbacks, optional bridge, macOS implementation | Windows/Linux left for later. New tests/build claimed by the author but without broad successful PR checks or a maintainer review at inspection. |
| [#200](https://github.com/egoist/mygo/pull/200) | Imperative `Window.NewWebView`/`NewOffscreenWebView`, explicit placement, message isolation, macOS/Windows/Linux | Claims device E2E on all three OSes, adds snapshots/offscreen HTML; **not** a ready-made declarative `ui.WebView` element and no upstream approval/merge. Known cross-platform differences in transparency, navigation frames, and rounded corners. |
| [#121](https://github.com/egoist/mygo/pull/121) | Upstream-maintainer-authored generic `Window.NewNativeView` + `ui.HostView` | Potential reusable hosting substrate; macOS and Windows CI failures at inspection and not merged. Not itself a WebView widget. |

Related upstream requests: [#148](https://github.com/egoist/mygo/issues/148), [#188](https://github.com/egoist/mygo/issues/188), and [#105](https://github.com/egoist/mygo/issues/105) (Native UI roadmap).

These are snapshot observations as of 2026-10-10, not promises about later merges. PR descriptions report the contributors' testing; that does not equal an upstream compatibility guarantee or Shardlane integration acceptance.

## Product decision

**Do not cherry-pick #60, #189, or #200 into `third_party/mygo` now.** Retain the official released Native UI and the smallest two local compatibility patches. A third, large, unsettled platform-specific fork would create substantial rebasing, focus/keyboard, layout, overlay, bridge, accessibility, lifecycle and rendering risk; most Shardlane surfaces (Terminal, Chat, History, Git) do not require HTML.

If a need arises for HTML-specific content, prefer the existing **separate** official MyGo WebView window for local Project Preview while awaiting stable embedded support. No private AppKit/WKWebView injection and no new desktop localhost backend.

Chat/History remain Native UI per `docs/client-product-architecture.md`, `docs/mygo-native-execution-rules.md` and project `AGENTS.md`. Official embedded availability alone does not lift that freeze: an explicit product decision is required.

## Gate for reconsideration

1. Upstream merges and releases a documented, cross-platform embedded WebView or generic host API with a committed public contract, lifecycle and backend support (macOS required; Linux/Windows checked).
2. Confirm composition with native popovers, menus, dialogs, tooltips, scrolling/clipping, resize/DPI, keyboard/IME, focus/shortcuts, accessibility, captures and window-close cleanup.
3. Establish safe navigation, origin, script permissions and Go bridge isolation: local Preview must not inherit privileged Host/Herdr calls.
4. Isolate an optional Preview-only spike with tests for repeated route changes, split panes, external URL rejection, memory/process cleanup and crash handling. Do not use Chat/History as the spike.
5. Measure startup/idle/resize cost against Native UI baseline and run real macOS app acceptance. Adopt only after a separate, explicit product choice.

[PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
