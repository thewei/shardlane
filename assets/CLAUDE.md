# assets/
> L2 | Parent: ../CLAUDE.md

Member inventory

- `app-icon/shardlane-{16,32,128,256,512}{,@2x}.png` — multi-resolution ladder for the Shardlane macOS app icon, rendered from `shardlane-d-dark.svg` (qlmanage 1024px master + sips downscale, 2026-09-09); `scripts/package-macos.sh` force-rebuilds `.app/Contents/Resources/Shardlane.icns` from this ladder via iconutil, overriding whatever cargo-bundle generated.
- `app-icon/shardlane-d-{dark,light}.svg` — D-concept vector marks, the icon/brand source of truth (same artwork as the website logo); the dark variant is the ladder master.
- `app-icon/redesign/kingfisher-concept.png` — 1254px AI-illustrated kingfisher master artwork, retired from the ladder on 2026-09-09 when the app icon was aligned to the D-concept brand mark; kept as design history.

Boundary: this directory holds product branding/packaging input assets only. It does not carry GPUI in-app icons or the Mobile Web PWA icons.
