# right_panel/
> L2 | Parent: ../CLAUDE.md

Member list

- `mod.rs`: the right-panel container, Project content snapshots, surface routing (Files/Services/Lazygit/Browser), and the public interaction boundary; persists only recoverable chrome/content state, never the Lazygit child process.
- `browser.rs`: local browser address resolution, navigation, and safe-URL judgment.
 - `browser_view.rs`: the Browser surface's address bar and native WebView projection; the omnibox hosts the opt-in loopback discovery suggestions dropdown (manual single-flight scan trigger, in-memory results, Enter and row clicks both route through navigate_browser), and the canvas keeps the session webview hidden while the dropdown or failure card owns the viewport.
 - `loopback_probe.rs`: the bounded loopback service discovery probe behind those suggestions — 127.0.0.1 only, a ≤64-port whitelist, 150 ms TCP connect + 500 ms HTTP GET with only 200..=399 counted as a discovery; manual trigger only (no polling, no timers), a single in-memory result cache, never persisted.
- `chooser.rs`: the surface chooser for the empty right panel.
- `services_view.rs`: the Services surface — resident service scripts (jump/start/stop/restart via the Script pane seam; membership is behavioral — every non-one-shot Script — with row meta lines surfacing the runtime's last error plus command/pid detail) and observed listening services (jump via FocusIntent), each port linkable to the localhost Browser surface; a running-count toolbar shares the `scripts::running_service_count` source with the sidebar/header badges and carries the two-click-armed "Stop all".
- `files.rs`: working-tree collection, file icons, and the bounded preview loader (size cap + binary sniff + image detection) shared with the content-area preview.
- `files_view.rs`: the Files surface's tree and file-selection interactions; clicking a file opens the full-content preview (file_preview.rs).
- `header.rs`: surface tabs, the creation menu, and close/switch actions.
- `lazygit.rs`: Lazygit CLI detection, Git root resolution, the YAML overlay, and the single auxiliary PTY session lifecycle.
- `lazygit_view.rs`: rendering, status card, and input/resize wiring for the Lazygit hosted terminal.
- `webview.rs`: creation, hiding, and teardown of the macOS Browser WebView.

The right panel carries at most one visible Lazygit auxiliary child; it is separate from the Herdr primary PTY, bound to the Project/active surface, and stops when hidden or closed.

Since 2026-09-03 the Sidebar Services section lives here as the Services surface, and file preview lives in the content area (`file_preview.rs`), not in this panel.
