# Shardlane Documentation

Public documentation for Shardlane. Internal working documents (iteration plans, audits, handoffs, and progress evidence) are kept in a private engineering archive outside this repository; none of them are required to build, run, or contribute to Shardlane.

## Read first

1. **`client-product-architecture.md`** — canonical architecture source of truth. Product/runtime ownership, layer boundaries, dependency/version policy, projection model, the Agent-history boundary, packaging, and the development workflow.
2. **`terminal-interaction-spec.md`** — terminal interaction-quality contract for the hosted Herdr TUI surface, with manual acceptance criteria.
3. **`performance-engineering.md`** — measured performance engineering contract: budgets, methodology, and known lessons.
4. **`macos-packaging-and-development.md`** — macOS `.app` packaging/install, app icon configuration, Mobile Web composition, and the cross-repo development loop.
5. **`remote-client-architecture.md`** — Host Remote API and remote/mobile client architecture.
6. **`libghostty-upgrade.md`** — how the vendored libghostty-vt static library is upgraded and ABI-verified (companion to [`vendor/ghostty-vt/PIN.md`](../vendor/ghostty-vt/PIN.md)).

## Rules

- Architecture conflicts are resolved in `client-product-architecture.md` first, then subordinate documents are updated.
- Documents must distinguish Current / Approved Next / Protocol Gap. Never write plans as implemented facts.
- External reference projects do not define Shardlane architecture and do not appear as product identities or permanent architecture dependencies.
