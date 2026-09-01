# right_panel/
> L2 | Parent: ../CLAUDE.md

Member list

- `mod.rs`: the right-panel container, Project content snapshots, surface routing, and the public interaction boundary; persists only recoverable chrome/content state, never the Lazygit child process.
- `browser.rs`: local browser address resolution, navigation, and safe-URL judgment.
- `browser_view.rs`: the Browser surface's address bar and native WebView projection.
- `chooser.rs`: the surface chooser for the empty right panel.
- `file_viewer.rs`: the Project file content viewer.
- `files.rs`: working-tree collection, file icons, and file reading.
- `files_view.rs`: the Files surface's tree and file-selection interactions.
- `header.rs`: surface tabs, the creation menu, and close/switch actions.
- `lazygit.rs`: Lazygit CLI detection, Git root resolution, the YAML overlay, and the single auxiliary PTY session lifecycle.
- `lazygit_view.rs`: rendering, status card, and input/resize wiring for the Lazygit hosted terminal.
- `webview.rs`: creation, hiding, and teardown of the macOS Browser WebView.

The right panel carries at most one visible Lazygit auxiliary child; it is separate from the Herdr primary PTY, bound to the Project/active surface, and stops when hidden or closed.
