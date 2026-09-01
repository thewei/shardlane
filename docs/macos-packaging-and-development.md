<!--
[INPUT]: Cargo.toml bundle metadata, cargo-bundle, assets/app-icon, scripts/, and optional herdr-mobile/dist
[OUTPUT]: The single operational manual for Shardlane macOS packaging/install/run/debug
(one-command release, packaged-app CLI discovery, Mobile Web/Remote port configuration)
[POS]: Release and cross-repo development runbook under docs/; does not change client/runtime ownership
-->

# Shardlane Mac App: Packaging, Installation, and Mobile Web Development Loop

Status: **Current / verified 2026-08-29 (Universal 2 orchestration; target build pending toolchain download)**

This document answers only two practical questions: how to get a genuinely installable
`Shardlane.app`, and how to develop and troubleshoot the Mac native shell and the built-in
Mobile Web with the shortest loop. The product boundary is still governed by
`client-product-architecture.md`: Shardlane is the Mac client/brand, Herdr is the sole
runtime authority, and Mobile Web is the Web build target of the same Mobile client.

## 1. Single packaging configuration

The app identity does not live in a second, drift-prone JSON/YAML file. The only
configuration is `[package.metadata.bundle]` in the repo-root `Cargo.toml`:

| Setting | Current value | What it produces |
| --- | --- | --- |
| `name` | `Shardlane` | `.app` name, menu/release identity |
| `identifier` | `dev.shardlane.app` | `CFBundleIdentifier` |
| `icon` | `assets/app-icon/shardlane-{16,32,128,256,512}{,@2x}.png` | cargo-bundle composes `Contents/Resources/Shardlane.icns` |
| `version` | inherited from `[package] version` | `CFBundleShortVersionString`/release version |

Do not write another copy of the app name, Bundle ID, or icon path inside scripts.
`crepus.toml` owns the dev target (`shardlane`) only, not the release identity.

The icon sources are a set of text-free, alpha-bearing multi-resolution PNGs; the
filenames keep `@2x` so `cargo-bundle 0.11.0` emits a full ICNS with Retina icon
variants. If you ever replace the icon, replace this set of source images, keep the
metadata path in sync, and rerun the structural verification.

## 2. Packaging and installation

### 2.0 One-command release (recommended daily entry point)

```sh
scripts/release-macos.sh
```

One command runs: static gates (fmt/clippy/test; `--skip-tests` to skip) → Mobile Web
static bundle readiness (auto `pnpm install + export:web` when `dist/` is missing;
`--build-web` forces a rebuild) → Universal 2 release `.app` (arm64+x86_64) packaging,
signing, structural verification, and install into `${SHARDLANE_INSTALL_ROOT:-~/Applications}`
(`--no-install` to skip) → `dist/Shardlane-macos-universal2.zip` + SHA-256 (`--no-archive`
to skip). Other options: `--sign IDENTITY`, `--mobile-root PATH`, `--without-web`,
`--arm64` (explicitly use the single-architecture fallback).

It is only an orchestration of `package-macos.sh` / `archive-macos.sh` / `bundle-web.sh`;
artifact rules, app identity, and gate details still live in the individual scripts and
Cargo metadata — no second configuration file.

### 2.1 Step-by-step packaging

Install the tools once:

```sh
cargo install crepuscularity-cli --version 0.16.0
cargo install cargo-bundle --version 0.11.0 --locked
```

### 2.2 Packaged-app CLI discovery (important)

A `.app` launched from Finder/Dock only inherits launchd's system PATH
(`/usr/bin:/bin:/usr/sbin:/sbin`), which excludes `~/.local/bin` (herdr installed via wax),
`~/.cargo/bin` (wax itself), and other user directories. Shardlane resolves CLIs through
`shardlane-host`'s `resolve_user_cli`/`herdr_cli_path`: process PATH → login-shell PATH
(`zsh -lic`, cached in-process) → user-level bin directories as fallback, so the packaged
app can launch/connect Herdr even when double-clicked. If the startup self-check reports
`herdr: NOT FOUND` (see `/tmp/shardlane-lag.log`), first confirm in a terminal that
`command -v herdr` resolves, then restart the app.

For packaged-app verification, simulate a Finder launch with a minimal environment:

```sh
env -i HOME="$HOME" PATH=/usr/bin:/bin:/usr/sbin:/sbin \
  ~/Applications/Shardlane.app/Contents/MacOS/shardlane &
grep -a "herdr:" /tmp/shardlane-lag.log | head -1
```

### 2.3 Step-by-step packaging commands

Package only the Mac native app (does not require the Mobile Web repo):

```sh
scripts/package-macos.sh
```

Release shape, embed the Mobile Web, and install into the current user's Applications
(Universal 2 by default):

```sh
scripts/package-macos.sh \
  --release \
  --universal \
  --with-mobile-web \
  --mobile-root ../herdr-mobile \
  --install
```

`--install` is an explicit action; the script only replaces `Shardlane.app` under
`SHARDLANE_INSTALL_ROOT` (default `~/Applications`) and never touches other apps.
Without Developer ID credentials the script uses ad-hoc signing, suitable for local runs
and structural verification; with credentials, pass them in:

```sh
scripts/package-macos.sh --release --sign "Developer ID Application: Your Name (TEAMID)"
```

The script reports success only after the following gates:

1. `cargo build --locked` (Universal 2 builds `aarch64-apple-darwin` and
   `x86_64-apple-darwin` separately);
2. `cargo bundle --format osx` (the arm64 bundle serves as the resource template);
3. Universal 2 mode merges the two executables with `lipo -create`, **signing only
   after the merge completes**;
4. Recursively checks every Mach-O inside the bundle (Universal 2 must contain both
   `arm64` and `x86_64`);
5. Checks `Contents/MacOS/shardlane` is executable;
6. Checks `CFBundleIdentifier = dev.shardlane.app` and a valid `Info.plist`;
7. Checks the `.icns` was generated;
8. `codesign --verify --deep --strict` with the ad-hoc/specified identity;
9. If `--with-mobile-web` is enabled, checks `Contents/Resources/mobile-web/index.html` exists.

Artifact locations:

```text
target/debug/bundle/osx/Shardlane.app
target/release/bundle/osx/Shardlane.app
target/<target-triple>/release/bundle/osx/Shardlane.app
target/aarch64-apple-darwin/release/bundle/osx/Shardlane.app  # final Universal 2 bundle
```

Real signing, notarization, Gatekeeper, and installation on external machines after
notarization remain release-credential/manual gates; a passing local ad-hoc check does
not mean release signing is done.

### 2.4 Mobile Web / Remote port configuration

The Remote API and Mobile Web share one listening port (serving `/api/v1` and the static
Web together, same-origin, no CORS). Default `8757`; when multiple apps/services conflict
on the same Mac, change the port directly in **Settings → Mobile → Port** (1024–65535;
Enter submits, restarts the listener, and persists `remote.port` in
`~/.shardlane/config.json`). Port semantics:

- `RemoteConfig::default()` and serde-missing fields → `DEFAULT_REMOTE_PORT` (8757);
- an explicit `0` in the config file is normalized to 8757 at load time (the server
  bind layer's `0=ephemeral` is for explicit test construction only);
- tests/CI isolation override via the `SHARDLANE_REMOTE_PORT` environment variable
  (spawned copies only, never persisted).

When listening fails (port occupied), the Mobile page's Listener row shows a failure
state; switch ports. Rescan the QR on the phone (the address carries the new port).

### 2.5 Release archive without a paid developer account

Without Developer ID credentials you can still publish an ad-hoc-signed ZIP; it suits
internal testing and manual installation by trusted users, and is not equivalent to
Apple notarization. The archive entry point only accepts an already-built and
verified `.app`:

```sh
scripts/archive-macos.sh \
  --app target/aarch64-apple-darwin/release/bundle/osx/Shardlane.app \
  --output-dir dist \
  --architecture universal2
```

Output:

```text
dist/Shardlane-macos-universal2.zip
dist/Shardlane-macos-universal2.zip.sha256
```

Verify from inside `dist/`:

```sh
cd dist
shasum -a 256 -c Shardlane-macos-universal2.zip.sha256
```

`.github/workflows/release.yml` runs the same archive script on `v*` tags and uploads
the ZIP and checksum to the GitHub Release. Release notes must state clearly that this
is an ad-hoc, non-notarized build; never upload or request any third-party Developer ID
private key (`.p12`).

## 3. Mobile Web composition

`herdr-mobile`'s `pnpm export:web` produces `dist/`. It is not a second Mac runtime and
must never start Herdr/PTY on the Web side:

```text
Shardlane.app
└── Contents/Resources/mobile-web/   ← herdr-mobile/dist (static files)
    ├── index.html
    ├── _expo/
    └── assets/
```

`scripts/bundle-web.sh` is the only cross-repo copy entry point:

```sh
# Development: copy into target/mobile-web and print SHARDLANE_WEB_BUNDLE
scripts/bundle-web.sh ../herdr-mobile

# Packaging: copy into the already-built app's Resources/mobile-web
scripts/bundle-web.sh ../herdr-mobile --app target/release/bundle/osx/Shardlane.app
```

`web_bundle_path()` resolves in this order:

1. `SHARDLANE_WEB_BUNDLE` (development override);
2. the packaged `Contents/Resources/mobile-web`;
3. with no static bundle, the Web surface is not mounted.

When Remote Access is enabled, the Remote server serves that directory same-origin with
`/api/v1`; static assets need no Bearer, while API/WS still require authentication.
Never write tokens into the Web bundle, localStorage, or plain logs.

## 4. Fastest development loops

### A. Mac native shell / GPUI

```sh
SDKROOT="$(xcrun --show-sdk-path)" crepus dev --bin shardlane
```

Verify windows, Sidebar, Settings, Terminal, and the Remote toggle here first. For native
issues, check `/tmp/shardlane-lag.log` first; record panics, native crashes, hangs, and
clean exits separately — do not conflate the four with "Web white screen".

### B. Mobile Web UI hot reload

```sh
cd ../herdr-mobile
pnpm install                 # first time or on lockfile changes
pnpm web                     # Expo Web hot reload, daily UI loop
pnpm typecheck
pnpm lint
pnpm test
```

`pnpm export:web` is for release/static integration verification, not the preferred loop
for every UI change; the Tamagui/React Compiler first export is slow. When you need the
same-origin Host topology, start `prox run web` per `herdr-mobile/docs/web-pwa-local-domain.md`;
do not hand-write another proxy or business service.

### C. Real Mac Host + Web integration

1. Start A; enable Remote Access in Shardlane Settings → Mobile/Remote.
2. Verify the Host first:

   ```sh
   curl -i http://127.0.0.1:8757/api/v1/hello
   curl -i -H "Authorization: Bearer <token>" \
     http://127.0.0.1:8757/api/v1/bootstrap
   ```

3. To test the static bundle:

   ```sh
   cd ../herdr-mobile && pnpm export:web
   cd ../herdr-client && scripts/bundle-web.sh ../herdr-mobile
   SHARDLANE_WEB_BUNDLE="$PWD/target/mobile-web" \
     SDKROOT="$(xcrun --show-sdk-path)" crepus dev --bin shardlane
   ```

4. Open the Host-served Web or the `prox` same-origin address at phone width in a
   browser; use DevTools only for Console/Network/Storage, and never copy credentials
   into screenshots or logs.

Debug by slicing along the boundary:

```text
Demo Web → static dist → Host hello/bootstrap → Agent/Pane API → WS events → Mac UI
```

Open layers left to right. This immediately distinguishes RN/Web render errors, static
export path errors, CORS/auth errors, Remote DTO errors, and Herdr runtime errors.

## 5. Debug evidence checklist

| Symptom | First checkpoint | Do not |
| --- | --- | --- |
| `.app` double-click shows no icon / stale icon | `Contents/Resources/*.icns`, `CFBundleIconFile`, Finder cache | Manually copying the icon into the bundle and forgetting Cargo metadata |
| Web 404/blank | `dist/index.html`, `SHARDLANE_WEB_BUNDLE`, `Contents/Resources/mobile-web` | Writing another Web server inside the Mac app |
| `401/403` | whether the token only travels via Authorization/WS subprotocol, whether Remote is enabled | Putting the token in URL query, localStorage, or logs |
| Browser CORS | development Origin allowlist; production prefers Host same-origin | Opening up `*` arbitrarily |
| API returns data but the UI does not refresh | bootstrap/queries, event bridge, polling fallback | Letting the Web own Herdr/PTY state itself |
| Mac-side latency/hang | `/tmp/shardlane-lag.log`, Herdr socket, visible-pane projection | Triggering a full `visible_state()` per output byte |

Minimal acceptance order for cross-repo changes:

```sh
# herdr-mobile
pnpm typecheck && pnpm lint && pnpm test && pnpm export:web

# herdr-client
cargo fmt -- --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
cargo build --locked --workspace
scripts/package-macos.sh --release --with-mobile-web --mobile-root ../herdr-mobile
git diff --check
git diff --cached --check
```

Native runtime/terminal performance smokes must isolate both `HERDR_SOCKET_PATH` and a
temporary `HOME`/config/state; otherwise test data reusing the user's existing Herdr
state cannot serve as a product baseline.

## 6. Design discipline

- Mac app, Remote API, and Mobile Web all consume the same Host/Core; do not duplicate
  the runtime.
- Mobile Web is only an Agent-first Host client; Terminal is an advanced fallback and
  must not take over the Mac's terminal controller.
- The static Web artifact is a packaging-time input; business logic belongs to
  `herdr-mobile`, serving/auth belongs to Shardlane Remote.
- Any new configuration goes to the existing Cargo/package or Mobile configuration
  owner first; never build a second manifest for a name or icon.
- Use mature APIs and existing verification scripts before adding custom native/web bridges.
