# Shardlane

Shardlane is a native macOS workspace for coding agents, built with Rust, GPUI, gpui-component, and libghostty-vt.

Shardlane is the **client/product**. **Herdr remains the backend runtime** and owns workspaces, tabs, panes, terminal sessions, agents, persistence, layout state, and process lifecycle. Shardlane projects that runtime into a native macOS interface; it does not replace or duplicate Herdr.

## Download

Prebuilt macOS Apple Silicon binaries are published on GitHub Releases:

- [Download the latest release](https://github.com/thewei/shardlane/releases/latest/download/Shardlane-macos-aarch64.zip) — ad-hoc signed and not notarized; right-click the app and choose Open on first launch to approve it in Gatekeeper.
- [Release index](https://github.com/thewei/shardlane/releases) · [Project site](https://thewei.github.io/shardlane/)

Shardlane requires the [Herdr](https://herdr.dev) runtime. If it is missing, the app offers to install it with `wax install herdr`.

## Requirements

- macOS with Xcode / macOS SDK
- stable Rust
- `herdr`
- bundled `libghostty-vt`
- `crepus` for the hot-reload development loop
- `wax` for installing Herdr when it is missing
- `cargo-bundle` 0.11.0 for building the macOS `.app`

Install development tools once:

```sh
cargo install waxpkg
cargo install crepuscularity-cli --version 0.16.0
cargo install cargo-bundle --version 0.11.0 --locked
```

If `herdr` is missing, Shardlane attempts `wax install herdr`.

## Development

```sh
SDKROOT="$(xcrun --show-sdk-path)" crepus dev --bin shardlane
```

Direct Cargo run:

```sh
SDKROOT="$(xcrun --show-sdk-path)" cargo run --locked --bin shardlane
```

## Checks

```sh
cargo fmt -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
cargo build --locked --workspace
git diff --check
```

Real-app UI acceptance uses the isolated Computer Use/MCP workflow in
[`docs/ui-acceptance-testing.md`](docs/ui-acceptance-testing.md) and
[`.agents/skills/ui-acceptance-testing/SKILL.md`](.agents/skills/ui-acceptance-testing/SKILL.md):

```sh
# No-GUI capability inventory (safe to run from any Agent/CI worker).
scripts/acceptance-capabilities.py check --json
# Print the read-only probe to execute through the host MCP.
scripts/acceptance-capabilities.py mcp-snippet
scripts/verify.sh ui
scripts/mux-acceptance.sh --driver computer-use --prepare --keep
```

`check` reports each local capability as `available`, `blocked`, `unknown`, or
`unavailable`. The Computer Use MCP itself must be probed by the Agent host with
`mcp__node_repl__js`; a shell process cannot declare or emulate that connector.
In Codex desktop, enable the Computer Use plugin/server/skill toggles first;
custom MCP servers belong to the host `~/.codex/config.toml`, not this repo.

Native CGEvent/System Events pacing or scroll measurements are explicit
real-device fallbacks and require `SHARDLANE_UI_DRIVER=native
SHARDLANE_ALLOW_GLOBAL_INPUT=1`; shared-display screenshots/video additionally
require `SHARDLANE_ALLOW_GLOBAL_CAPTURE=1`.

## macOS app bundle

One-click release (gates + Mobile Web + install + archive):

```sh
scripts/release-macos.sh
```

Or use the individual reproducible entrypoints (the bundle name, identifier, and icon are configured once in `Cargo.toml`):

```sh
scripts/package-macos.sh
scripts/package-macos.sh --release --with-mobile-web --mobile-root ../herdr-mobile --install
```

For the individual cargo-bundle steps and the cross-repo development/debug loop, read [`docs/macos-packaging-and-development.md`](docs/macos-packaging-and-development.md).

Bundle identity:

- App: `Shardlane.app`
- Executable: `shardlane`
- Bundle identifier: `dev.shardlane.app`

Tags matching `v*` build one arm64 (Apple Silicon) ad-hoc-signed `Shardlane.app` release archive (`Shardlane-macos-aarch64.zip`) and SHA-256 checksum. It is not notarized; first launch on another Mac requires the user to explicitly allow the app in Finder/System Settings.

To create the same archive locally after building a release app:

```sh
scripts/package-macos.sh --release --target aarch64-apple-darwin
scripts/archive-macos.sh \
  --app target/aarch64-apple-darwin/release/bundle/osx/Shardlane.app \
  --output-dir dist \
  --architecture aarch64
(cd dist && shasum -a 256 -c Shardlane-macos-aarch64.zip.sha256)
```

The GitHub Actions `release` workflow runs this archive step automatically for `v*` tags and uploads both files to the GitHub release.

## Architecture

Ownership is intentionally narrow:

1. **Herdr** — runtime/backend authority.
2. **libghostty-vt** — terminal semantics.
3. **gpui-component / GPUI** — native desktop UI and interaction primitives.
4. **Shardlane shell** — presentation, navigation, local settings, and read-only coding-agent history browsing.

Runtime capabilities missing from Herdr must be added at the Herdr boundary rather than implemented as a second local runtime inside Shardlane.

## Scope

- native macOS client
- Herdr socket/runtime integration
- libghostty-backed terminal rendering
- workspace/tab/pane navigation
- coding-agent history and resume flows through Herdr
- local right-panel web preview only (not a general browser)
- optional packaged Mobile Web companion served by the Host Remote API
- no plugin marketplace
- no cloud account layer
- no telemetry

## Contributing

Development setup, the verification gates every change must pass, and review expectations live in [CONTRIBUTING.md](CONTRIBUTING.md). Bug reports and feature requests use the issue templates.

## Security

Report vulnerabilities privately through GitHub's vulnerability reporting — see [SECURITY.md](SECURITY.md). Do not open public issues for anything exploitable.

## License

Shardlane is source-available under the [PolyForm Noncommercial License 1.0.0](LICENSE): free for personal, learning, research, and other noncommercial use; any commercial use requires a separate commercial license from the author. Forks and derived works must retain the copyright and license notices. Embedded third-party material keeps its own license — see [`THIRD_PARTY_NOTICES.md`](THIRD_PARTY_NOTICES.md).
