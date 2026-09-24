# .github/
> L2 | 父级: ../CLAUDE.md

成员清单

- `workflows/checks.yml` — macOS Rust gates plus the no-GUI UI acceptance capability/unit preflight.
- `workflows/release.yml` — tagged release matrix plus a publish job that assembles the GitHub release and the backward-compatible latest.json (flat top-level url kept for 0.1.16 clients, plus a platforms map). Releases ship macOS-only (arm64/x86_64 app zips) since v0.1.17; the Linux x86_64 and Windows x86_64 jobs stay defined but `if: false`. Portability groundwork has landed — per-target vendored archives (ABI-gated, PIN.md), the shardlane-history unix-permission split, and a vendored gpui-symbols patch fixing its non-macOS stub — but the rest of the workspace (herdr-gui / GPUI backends) is still unvalidated on native linux/windows runners; port and validate there before flipping the two `if` lines, and the windows job's live-herdr named-pipe socket-verification merge gate returns with it.
- `workflows/vendor-ghostty-vt.yml` — workflow_dispatch producer for the per-target vendored libghostty-vt archives via `scripts/vendor-ghostty-vt.sh` (ABI-gated) on macOS/Linux/Windows runners; artifacts are committed under `vendor/ghostty-vt/lib/<triple>/`.
- `ISSUE_TEMPLATE/` — issue forms for bug reports and feature requests.
- `pull_request_template.md` — review checklist and required verification commands.

法则: CI 复用仓库脚本·不启动真实 GUI·发布流程与检查流程分离
[PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
