<!-- Keep the change bounded; one logical change per PR. -->

## What

<!-- What does this PR change, in user-visible or architectural terms? -->

## Why

<!-- Motivation, linked issue, or the defect being fixed. -->

## Verification

<!-- The gates below are required; check what you ran. -->

- [ ] `cargo fmt -- --check`
- [ ] `cargo clippy --locked --workspace --all-targets -- -D warnings`
- [ ] `cargo test --locked --workspace`
- [ ] `cargo build --locked --workspace`
- [ ] Packaging changes: `Shardlane.app` built and structurally verified
- [ ] Native runtime/input/render changes: app smoked (`/tmp/shardlane-lag.log` inspected)
