<!-- Keep the change bounded; one logical change per PR. -->

## What

<!-- What does this PR change, in user-visible or architectural terms? -->

## Why

<!-- Motivation, linked issue, or the defect being fixed. -->

## Verification

<!-- The gates below are required; check what you ran. -->

- [ ] `GOTOOLCHAIN=go1.27.1 go test ./...`
- [ ] `GOTOOLCHAIN=go1.27.1 go tool mygo build`
- [ ] Packaging changes: `Shardlane.app` built and structurally verified
