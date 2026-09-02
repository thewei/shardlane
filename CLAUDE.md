# Shardlane — native macOS workspace for coding agents

Rust + GPUI 0.2.2 + gpui-component 0.5.1 + Crepuscularity GPUI + vendored libghostty-vt + Herdr runtime APIs

<directory>
assets/ - packaged Shardlane brand artwork
crates/ - Shardlane application and read-only Agent-history code
ui/ - thin Crepuscularity shell composition templates
vendor/ - bundled native runtime artifacts (ghostty-vt) + patched gpui 0.2.2 (see vendor/gpui/PACING-PATCH.md, wired via [patch.crates-io])
scripts/ - tiered verification and composable UI acceptance entrypoints (verify.sh: check/unit/history/ui/fast/full)
docs/ - public architecture, interaction, performance, packaging, and remote documentation
.agents/skills/ - project-local engineering workflow/checklists
.github/ - CI and release workflows
</directory>

<config>
Cargo.toml - Shardlane package/workspace/dependency policy
crepus.toml - Shardlane development target
build.rs - links bundled libghostty-vt
AGENTS.md - repository engineering rules and required reading
README.md - developer setup, run, checks, packaging, and product scope
THIRD_PARTY_NOTICES.md - required notices for third-party material only
scripts/package-macos.sh - reproducible macOS build/sign/verify/install entrypoint
scripts/release-macos.sh - one-click release: gates + Mobile Web + install + dist archive
scripts/archive-macos.sh - reproducible macOS app ZIP/checksum entrypoint
docs/macos-packaging-and-development.md - packaging, Mobile Web composition, and debug runbook
docs/client-product-architecture.md - unique architecture source of truth
docs/ui-acceptance-testing.md - isolated real-app acceptance, Computer Use MCP, native fallback, and ground-truth evidence
scripts/acceptance-capabilities.py + scripts/acceptance/ - no-GUI capability inventory, MCP probe, and composable backend assertions
.github/workflows/checks.yml - macOS CI gates plus no-GUI UI acceptance/unit preflight
.agents/skills/herdr-client-development/SKILL.md - Shardlane engineering workflow
.agents/skills/ui-acceptance-testing/SKILL.md - reusable UI acceptance driver/isolation/evidence workflow
</config>

Product rule: **Shardlane is the client and brand. Herdr is the backend/runtime.** Herdr remains authoritative for workspaces, tabs, panes, layouts, agents, scrollback, persistence, terminal sessions, and process lifecycle. Shardlane owns native macOS presentation, local interaction state/preferences, and the read-only Agent-history browsing/index layer.

Implementation rule: do not hand-roll capabilities while mature owners exist. Search in order: Herdr API → vendored libghostty-vt → gpui-component → GPUI → proven compatible implementation → custom code. Missing runtime APIs are fixed at the Herdr boundary rather than bypassed with a parallel terminal/process path.

Shell rule: Shardlane has one canonical Sidebar implementation. Do not reintroduce legacy alternate Sidebar layouts or reference-project-specific presentation modes.

Principles: minimal · stable · navigation-first · version-exact · mature-API-first
