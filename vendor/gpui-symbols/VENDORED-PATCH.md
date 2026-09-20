# Vendored patch: gpui-symbols 0.6.1

Source: https://github.com/AprilNEA/gpui-symbols @ v0.6.1 (crates.io latest,
2026-01-21). Vendored 2026-09-20.

## Why

The non-macOS `platform::render_sf_symbol` stub kept the pre-0.6 four-argument
signature while `symbol.rs::render_rgba` calls seven arguments, so the crate
fails to compile on every non-macOS target (E0061). Upstream main is unchanged
as of 2026-09-20; no fixed release exists.

## The patch (one hunk)

`src/platform/mod.rs`: the `#[cfg(not(target_os = "macos"))]` stub now takes
the full seven-argument signature (`_weight`, `_symbol_scale`,
`_rendering_mode` added, still returning `None`). macOS rendering is
untouched.

## Removal condition

Delete this directory and the `[patch.crates-io] gpui-symbols` entry in the
root `Cargo.toml` as soon as crates.io ships a gpui-symbols release whose
non-macOS stub accepts seven arguments (verify with
`cargo check --target x86_64-unknown-linux-gnu -p gpui-symbols` against the
registry version first).
