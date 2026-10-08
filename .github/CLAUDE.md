# .github/
> L2 | 父级: ../CLAUDE.md

成员清单

- `workflows/pages.yml` — GitHub Pages 部署 `site/`（静态站点，无构建步骤）。
- Go 检查门（`checks.yml`）暂缺：`go.mod` 携带本地 `../mygo` replace 补丁，CI checkout 解析不到该依赖；待上游合入 LocalDragSelect、删除 replace 后恢复（`go test ./...` + `go tool mygo build`）。
- `ISSUE_TEMPLATE/` — issue forms for bug reports and feature requests.
- `pull_request_template.md` — review checklist and required verification commands.

法则: CI 不启动真实 GUI · 发布流程与检查流程分离 · 本地 replace 未解除前不加 Go CI
[PROTOCOL]: 变更时更新此头部，然后检查 ../CLAUDE.md
