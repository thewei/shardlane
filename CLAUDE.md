# Shardlane — native macOS workspace for coding agents (Go / MyGo Native UI)

Current implementation: Go 1.27.1 + MyGo Native UI (pinned v0.2.15 plus the local `../mygo` replace patch) + official MyGo Terminal/Ghostty; Herdr remains the runtime authority. The previous Rust/GPUI implementation lives only on `rewrite/mygo`/`main`.

<directory>
internal/ - Go application/domain packages (nativeui presentation, herdr adapter, agent/history/gitworkbench domains, settings, applog, ...)
resources/ - MyGo packaging resources (app icon)
site/ - public site (landing + downloads page) published to GitHub Pages (static HTML/CSS, no build step)
docs/ - architecture source of truth, MyGo migration contract/roadmap, interaction/performance/audit documentation
.agents/skills/ - project-local engineering workflow/checklists
.github/ - CI workflows, issue/PR templates
</directory>

<config>
go.mod + go.sum + mygo.json - Go toolchain (1.27.1), MyGo pin (v0.2.15 + replace ../mygo), and packaging policy
AGENTS.md - repository engineering rules and required reading
README.md - developer setup, run, checks, packaging, and product scope
docs/client-product-architecture.md - unique architecture source of truth
docs/mygo-native-execution-rules.md - active migration execution contract
docs/mygo-native-migration-roadmap.md - active migration roadmap and task order
.github/workflows/pages.yml - GitHub Pages deployment for site/
</config>

Product rule: **Shardlane is the client and brand. Herdr is the backend/runtime.** Shardlane must not introduce a second Workspace registry, pane runtime, terminal authority, or process lifecycle owner. Herdr remains authoritative for workspaces, tabs, panes, layouts, agents, scrollback, persistence, terminal sessions, and process lifecycle. Shardlane owns native macOS presentation, local interaction state/preferences, and the read-only Agent-history browsing/index layer.

Implementation rule: do not hand-roll capabilities while mature owners exist. Search in order: Herdr API → MyGo native components/official plugins → a proven compatible implementation → custom code. Missing runtime APIs are fixed at the Herdr boundary rather than bypassed with a parallel terminal/process path.

CI note: GitHub Actions Go gates are intentionally absent while `go.mod` carries the local `../mygo` replace patch (CI checkouts have no sibling mygo checkout); reintroduce `.github/workflows/checks.yml` (go test + mygo build) once the patch is upstreamed and the replace is removed.

Principles: minimal · stable · navigation-first · version-exact · mature-API-first
