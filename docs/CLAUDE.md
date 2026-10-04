# docs/
> L2 | Parent: ../CLAUDE.md

Members:
- `README.md` — documentation entry point.
- `client-product-architecture.md` — unique architecture source of truth; defines Shardlane product ownership and Herdr runtime ownership.
- `terminal-interaction-spec.md` — terminal interaction-quality contract and manual acceptance for the hosted Herdr TUI surface.
- `performance-engineering.md` — measured performance contract, budgets, methodology, and known lessons.
- `macos-packaging-and-development.md` — macOS `.app` packaging/install, Mobile Web composition, and the cross-repo debug loop.
- `remote-client-architecture.md` — Host Remote API and remote/mobile client architecture.
- `multiplexer-api.md` — Approved Next backend-neutral Multiplexer API: domain catalog, capability model, adapter/registry rules, and the TDD migration/verification strategy.
- `ui-acceptance-testing.md` — UI acceptance harness practice: `SHARDLANE_BIND_INSTANCE` seam, evpost/vocr tools, isolation discipline, ground-truth assertion patterns, and pitfalls.
- `mygo-native-execution-rules.md` — active MyGo migration execution contract: atomic task sizing, verification tiers, package/file boundaries, Native UI-only Chat/History policy, and final-cutover rules.
- `mygo-native-migration-roadmap.md` — active MyGo migration handoff/task ledger with milestone order, task IDs, dependencies, and focused verification.
- `mygo-native-0.3.0-closure-audit.md` — post-0.3 closure audit covering framework currency, UI system drift, History request concurrency, Agent idempotency, operational status and Hook/Integration health.
- `mygo-native-0.4.0-closure-plan.md` — next closure release: MyGo 0.2.7, Design System v2, shared operational state, Provider Integration/Hook Health, and safe Agent launch.
- `mygo-native-0.5.0-agent-workbench-plan.md` — post-0.4 Agent lifecycle/workbench: one card model, unread/review semantics, Agent directory/overview/switcher, notifications, and History Continue before full Chat.
- `mygo-native-0.6.0-conversation-chat-plan.md` — Native Conversation/Chat: session-exact identity, incremental live semantics, safe prompt/queue transactions, interactions, timeline, and Live Handoff.
- `mygo-native-0.7.0-status-center-plan.md` — right-side Status Center + MyGo Tray: shared Agent status snapshot, attention/review/working grouping, and exact quick navigation.
- `reference-magpie-agent-history-usage-audit.md` — external reference audit: menu-bar floating panel, Project-grouped Sessions, safe Trash, Usage/Quota attribution, Gateway concepts to adopt/adapt/reject.
- `mygo-native-0.8.0-history-usage-agent-inspector-plan.md` — grouped History management, capability-gated Trash/Restore/Purge, UsageService, quota and Agent Inspector before the larger workspace-intelligence release.
- `reference-magpie-herdr-gpui-shardlane-0.9-audit.md` — three-way reference audit covering command/workspace UX, Project tools, Git/PR, desktop affordances, diagnostics, updater and explicit reject/defer boundaries.
- `mygo-native-0.9.0-workspace-intelligence-desktop-experience-plan.md` — large post-0.8 release plan for Command Center, Right Panel tools, Services/Ports, Git/PR, desktop attention, Diagnostics/Logs, updater, density/high contrast and Task Presets.
- `reference-godiff-shardlane-0.10-audit.md` — Godiff reference decomposition plus post-0.9 reality audit; documents which landed features are package-only versus actually wired and the current Godiff import/license constraints.
- `mygo-native-0.10.0-git-workbench-primary-surface-plan.md` — removes Lazygit and defines one `/workspace` Terminal/Diff/Commit primary surface, Godiff-inspired Sidebar/Header/Changes UI, safe native commit/branch operations, and inherited 0.9 fixes.
- `libghostty-upgrade.md` — vendored libghostty-vt upgrade and ABI verification runbook.

Rules:
- Shardlane is the client/product brand; Herdr is the real backend/runtime and retains its technical name wherever that is the fact being described.
- Architecture conflicts are resolved in `client-product-architecture.md` first, then subordinate docs are updated.
- Documents must distinguish Current / Approved Next / Protocol Gap. Never write plans as implemented facts.
- The active MyGo migration execution rules and roadmap are an explicit repository-owned exception because multiple implementers must share one authoritative task contract. Architecture facts still belong only in `client-product-architecture.md`; incidental audits/progress evidence may remain outside the repository.

[PROTOCOL]: Update this header on change, then check CLAUDE.md.
