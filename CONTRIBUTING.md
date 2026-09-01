# Contributing to Shardlane

Thank you for your interest in improving Shardlane. This document covers the setup, the verification gates every change must pass, and what reviewers look for.

## Ground rules

- **Shardlane is the client; Herdr is the runtime.** Herdr stays authoritative for workspaces, tabs, panes, terminal sessions, agents, scrollback, persistence, and process lifecycle. Read [`docs/client-product-architecture.md`](docs/client-product-architecture.md) before your first change — it is the single architectural source of truth.
- Prefer mature owners over hand-rolled code, in this order: Herdr API → vendored libghostty-vt → gpui-component → GPUI → a proven compatible implementation → custom code.
- Do not invent Herdr protocol methods or socket shapes; inspect the live API instead.

## Development setup

macOS with the Xcode SDK and a stable Rust toolchain are required. One-time tool installs (and what `herdr` is) are documented in the [README](README.md#requirements).

Run the app for local smoke testing with:

```sh
SDKROOT="$(xcrun --show-sdk-path)" crepus dev --bin shardlane
```

## Verification gates

Every change must pass before review:

```sh
cargo fmt -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
cargo build --locked --workspace
git diff --check
git diff --cached --check
```

Additional expectations:

- Packaging changes must also build and structurally verify `Shardlane.app` (`scripts/package-macos.sh`).
- Native runtime/input/render changes should be smoked in the app; the lag trace lands in `/tmp/shardlane-lag.log`.
- Terminal behavior changes should satisfy [`docs/terminal-interaction-spec.md`](docs/terminal-interaction-spec.md).

## Submitting changes

- Keep the change bounded and describe what you verified, not only what you changed.
- A few focused commits with clear messages beat one large dump.
- For behavior changes, include the manual-acceptance steps you ran.

## Reporting issues

Bug reports and feature requests use the issue templates. Security vulnerabilities must **not** go through public issues — see [`SECURITY.md`](SECURITY.md).
