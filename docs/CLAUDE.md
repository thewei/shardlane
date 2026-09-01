# docs/
> L2 | Parent: ../CLAUDE.md

Members:
- `README.md` — documentation entry point.
- `client-product-architecture.md` — unique architecture source of truth; defines Shardlane product ownership and Herdr runtime ownership.
- `terminal-interaction-spec.md` — terminal interaction-quality contract and manual acceptance for the hosted Herdr TUI surface.
- `performance-engineering.md` — measured performance contract, budgets, methodology, and known lessons.
- `macos-packaging-and-development.md` — macOS `.app` packaging/install, Mobile Web composition, and the cross-repo debug loop.
- `remote-client-architecture.md` — Host Remote API and remote/mobile client architecture.
- `libghostty-upgrade.md` — vendored libghostty-vt upgrade and ABI verification runbook.

Rules:
- Shardlane is the client/product brand; Herdr is the real backend/runtime and retains its technical name wherever that is the fact being described.
- Architecture conflicts are resolved in `client-product-architecture.md` first, then subordinate docs are updated.
- Documents must distinguish Current / Approved Next / Protocol Gap. Never write plans as implemented facts.
- Internal iteration plans, audits, handoffs, and progress evidence live in a private engineering archive outside this repository; do not reference them from public documents.
