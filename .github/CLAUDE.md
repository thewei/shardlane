# .github/
> L2 | 父级: ../CLAUDE.md

成员清单

- `workflows/checks.yml` — macOS Go 门禁：`go test ./...`、`go tool mygo build`、app bundle 结构校验（vendored `third_party/mygo` replace 使 CI 自包含）。
- `workflows/pages.yml` — GitHub Pages 部署 `site/`（静态站点，无构建步骤）。
- `ISSUE_TEMPLATE/` — issue forms for bug reports and feature requests.
- `pull_request_template.md` — review checklist and required verification commands.

法则: CI 不启动真实 GUI · 发布流程与检查流程分离 · 门禁使用仓库内 vendored 依赖
[PROTOCOL]: 变更时更新此头部，然后检查 ../CLAUDE.md
