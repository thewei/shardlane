# Shardlane Documentation

Shardlane architecture and engineering documentation. `client-product-architecture.md` remains the unique architecture source of truth. The active MyGo migration execution rules and roadmap are intentionally repository-owned so parallel implementers share one task contract; incidental audits/progress evidence may still live outside the repository.

Note: documents that describe the removed Rust/GPUI desktop (packaging scripts, vendored libghostty-vt, Host Remote API, Multiplexer seam, Rust latency harnesses) remain here as historical reference for the `rewrite/mygo` and `main` branches; the Go/MyGo Native UI client is the only implementation on this branch.

## Read first

1. **`client-product-architecture.md`** — canonical architecture source of truth. Product/runtime ownership, layer boundaries, dependency/version policy, projection model, the Agent-history boundary, packaging, and the development workflow.
2. **`terminal-interaction-spec.md`** — terminal interaction-quality contract for the hosted Herdr TUI surface, with manual acceptance criteria.
3. **`performance-engineering.md`** — measured performance engineering contract: budgets, methodology, and known lessons.
4. **`macos-packaging-and-development.md`** — macOS `.app` packaging/install, app icon configuration, Mobile Web composition, and the cross-repo development loop.
5. **`remote-client-architecture.md`** — Host Remote API and remote/mobile client architecture.
6. **`libghostty-upgrade.md`** — how the vendored libghostty-vt static library is upgraded and ABI-verified (companion to [`vendor/ghostty-vt/PIN.md`](../vendor/ghostty-vt/PIN.md)).
7. **`multiplexer-api.md`** — Approved Next backend-neutral Multiplexer API contract (domain catalog, capabilities, adapters/registry, TDD migration and verification).
8. **`ui-acceptance-testing.md`** — how to drive the real app in scripted acceptance tests (Computer Use MCP contract, bind seam, evpost/vocr tools, isolation, ground-truth assertions, and composable capability probes).
9. **`mygo-native-execution-rules.md`** — active migration execution rules: small task sizing, verification tiers, Native UI policy, logging/privacy boundaries, and cutover discipline.
10. **`mygo-native-migration-roadmap.md`** — active implementation/handoff roadmap with milestone order, atomic task IDs, dependencies, and quick verification.
11. **`mygo-native-0.2.0-audit.md`** — audited test-build status, package evidence, known gaps, and manual test scope.
12. **`mygo-native-0.3.0-plan.md`** — next-version technical plan for Native Settings + real Native History.
13. **`prompts/mygo-native-0.3.0-implementation-prompt.md`** — reusable implementation handoff prompt for the 0.3.0 work.
14. **`mygo-native-0.3.0-closure-audit.md`** — post-0.3 closure audit: framework, Design System, History concurrency, Agent idempotency, dynamic status and Hook/Integration gaps.
15. **`mygo-native-0.4.0-closure-plan.md`** — next closure release plan for MyGo 0.2.7, UI system unification, operational status, integration health and safe Agent launch.
16. **`prompts/mygo-native-0.4.0-closure-implementation-prompt.md`** — executable 0.4 implementation handoff prompt.
17. **`mygo-native-0.5.0-agent-workbench-plan.md`** — post-0.4 Agent lifecycle/workbench plan: cards, attention/review markers, Agent directory, MRU switcher, Continue/Resume.
18. **`prompts/mygo-native-0.5.0-agent-workbench-implementation-prompt.md`** — executable 0.5 Agent Workbench handoff prompt.
19. **`mygo-native-0.6.0-conversation-chat-plan.md`** — Native Conversation/Chat plan: exact identity, live decoder, prompt delivery, queue, interactions, timeline and Handoff.
20. **`prompts/mygo-native-0.6.0-conversation-chat-implementation-prompt.md`** — executable 0.6 Native Conversation/Chat implementation prompt.
21. **`mygo-native-0.7.0-status-center-plan.md`** — right-top Status Center and cross-platform Tray plan for Agent status aggregation and exact quick navigation.
22. **`prompts/mygo-native-0.7.0-status-center-implementation-prompt.md`** — executable 0.7 Status Center implementation prompt.
23. **`reference-magpie-agent-history-usage-audit.md`** — current Magpie reference audit for menu-bar quick panel, Project-grouped Sessions, Trash/Restore, Usage/Quota and gateway boundaries.
24. **`mygo-native-0.8.0-history-usage-agent-inspector-plan.md`** — post-0.7 management/insight release: grouped History, Trash, Usage, quota and Agent Inspector.
25. **`prompts/mygo-native-0.8.0-history-usage-agent-inspector-implementation-prompt.md`** — executable 0.8 implementation handoff prompt.
26. **`reference-magpie-herdr-gpui-shardlane-0.9-audit.md`** — three-way product/architecture audit of Magpie, herdr-gpui and current Shardlane; adopt/adapt/defer/reject decisions for the next large desktop release.
27. **`mygo-native-0.9.0-workspace-intelligence-desktop-experience-plan.md`** — large 0.9 plan: Command Center, Project tools, Services/Ports, Git/PR, desktop attention, Diagnostics, updater, appearance and Task Presets.
28. **`prompts/mygo-native-0.9.0-workspace-intelligence-desktop-experience-implementation-prompt.md`** — executable 0.9 implementation handoff prompt.
29. **`reference-godiff-shardlane-0.10-audit.md`** — current Godiff UX/source-structure audit plus a source-level reality audit of the landed 0.9 implementation; records import/license constraints and inherited wiring gaps.
30. **`mygo-native-0.10.0-git-workbench-primary-surface-plan.md`** — 0.10 plan: remove Lazygit, add unified Terminal/Diff/Commit primary surfaces, native changed-file review/commit/branch workflows, Godiff-inspired shell styling, and inherited 0.9 correctness closure.
31. **`prompts/mygo-native-0.10.0-git-workbench-primary-surface-implementation-prompt.md`** — executable 0.10 implementation handoff prompt.

## Rules

- Architecture conflicts are resolved in `client-product-architecture.md` first, then subordinate documents are updated.
- Documents must distinguish Current / Approved Next / Protocol Gap. Never write plans as implemented facts.
- External reference projects do not define Shardlane architecture and do not appear as product identities or permanent architecture dependencies.
