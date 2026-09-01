# Security Policy

## Reporting a vulnerability

Please report security vulnerabilities through **GitHub's private vulnerability reporting** for this repository. Do not open a public issue for anything you believe is exploitable.

Include what you can of the usual details: affected component, reproduction steps, impact, and (if known) a suggested fix. You can expect an initial response within a few days.

## Scope

In scope:

- the Shardlane macOS client (`shardlane` binary, `shardlane-host` application services, `shardlane-remote` Host Remote API server: pairing, access tokens, bind modes, SSH socket bridging);
- the vendored artifacts Shardlane builds and ships: `libghostty-vt` and the patched `vendor/gpui` tree;
- the packaged `Shardlane.app` bundle and its release archives.

Out of scope:

- the **Herdr runtime itself** — Shardlane's backend is a separate project; report Herdr issues to its own maintainers;
- external coding-agent CLIs and providers that run inside Herdr;
- issues that require a non-default, explicitly insecure local configuration.

## Notes

- Release archives are ad-hoc signed and **not notarized**; verify the SHA-256 checksum published next to every release ZIP.
- The Host Remote API binds loopback by default; `local_network` mode widens the bind on purpose and is gated by access-token pairing.
