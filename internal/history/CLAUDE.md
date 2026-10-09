# internal/history/
> L2 | 父级: ../../CLAUDE.md

History 是 Shardlane 的只读 Provider 会话目录；扫描 Provider 原生文件，Catalog 索引和分页缓存属于本客户端，但不会承接 Herdr 的 Agent/PTY 生命周期，也不会修改来源历史文件。调用方向为 `nativeui → HistoryService → Catalog / Scanner`，同层解析器不能反向依赖 UI。

成员清单

- `models.go`：Provider AgentID、会话元数据、消息及来源类型，统一跨解析器的数据语言。
- `service.go`：HistoryService 查询/扫描服务入口；Project/Provider 范围在 SQL LIMIT 前应用，并暴露完整 Provider 列表。
- `catalog.go`：SQLite Catalog 打开、表结构、会话写入与缓存生命周期。
- `catalog_read.go`：只读元数据查询、Provider 集合、Project/Provider 范围过滤与搜索；数据库负责最终排序和 LIMIT。
- `catalog_pages.go`：Transcript 分页缓存的查询与存储。
- `catalog_scan.go`：Catalog 行扫描和元数据解码。
- `scanner.go`：多 Provider 来源目录协调与增量索引。
- `scan_utils.go`：增量扫描的文件及目录遍历辅助。
- `path_key.go`：Project 路径归一化，用于索引过滤。
- `locator.go`：来源会话 identity / 定位契约。
- `parse_utils.go`：解析器共用安全裁剪、路径与时间工具。
- `claude.go`：Claude Code 会话流解析器。
- `claude_blocks.go`：Claude 原生消息块映射。
- `codex.go`：Codex rollout 会话流解析器。
- `codex_blocks.go`：Codex 工具与内容块映射。
- `pi.go`：Pi / Oh My Pi 会话解析器。
- `live.go`：Provider 会话的只读增量 decoder / 追尾接口。
- `catalog_test.go`：Catalog 写入、查询和索引基本契约。
- `claude_test.go`：Claude 会话解析行为。
- `codex_test.go`：Codex 会话解析行为。
- `history_test.go`：跨 Provider 解析与历史模型语义。
- `live_test.go`：增量读取/解码的边界行为。
- `pi_test.go`：Pi Provider 解析和 live 增量回归。
- `scanner_test.go`：来源扫描、去重与索引协调。
- `service_test.go`：HistoryService 列表/读取/扫描契约。
- `service_filter_test.go`：Provider/Project 提前过滤和完整 Provider 集合的回归。

[PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
