# assets/
> L2 | Parent: ../CLAUDE.md

Member inventory

- `app-icon/shardlane-{16,32,128,256,512}{,@2x}.png` — multi-resolution source images for the Shardlane macOS app icon; cargo-bundle composes them into `.app/Contents/Resources/Shardlane.icns`.
- `app-icon/shardlane-d-{dark,light}.svg` — theme-ready D-concept vector marks for in-app embedding; these are opt-in and do not change the cargo-bundle PNG source set.
- `app-icon/redesign/kingfisher-concept.png` — 1254px AI-illustrated kingfisher master artwork; the source from which the multi-resolution ladder above is generated (tile cropped to alpha bbox, normalized to the Apple icon grid with transparent margins). Swift/origami-crane candidates were removed after the kingfisher direction was chosen on 2026-09-03.

Boundary: this directory holds product branding/packaging input assets only. It does not carry GPUI in-app icons or the Mobile Web PWA icons.
