# internal/history/
> L2 | 父级: ../../CLAUDE.md

History 是 Shardlane 的只读 Provider 会话目录；扫描 Provider 原生文件，Catalog 是纯内存的列表行索引（标题/计数/项目/≤240 字描述行），**永不保存会话正文**：没有转录缓存、没有消息全文索引、没有数据库，阅读一律由 `HistoryService.Open` 现读原始会话文件。不会承接 Herdr 的 Agent/PTY 生命周期，也不会修改来源历史文件。调用方向为 `nativeui → HistoryService → Catalog / Scanner`，同层解析器不能反向依赖 UI。Sessions 的删除（移入回收站/放回/永久删除）由同级 `internal/sessiontrash` 搬动文件，本包仍只读：只提供 `Forget`（文件被搬走后立即摘除行与路径占用）与 `SummarizeSource`（从单个来源文件读回一行列表摘要，供回收站列表）。

成员清单

- `models.go`：Provider AgentID、会话元数据、消息及来源类型，统一跨解析器的数据语言。
- `service.go`：HistoryService 查询/扫描/阅读入口；`Forget` 把已离开来源目录的会话摘出 Catalog；Project/Provider 范围在 LIMIT 前应用（Project = 该文件夹及其所有子文件夹，按路径边界匹配：`/work/ab` 不属于 `/work/a`），暴露完整 Provider 列表及各 Provider 会话总数（Sessions 标签计数，元数据只读，不受列表上限影响；`ProviderCountsIn(项目)` 给出一个项目内的同类计数）；`Open` 每次解析原始会话文件并切出一个有界窗口（无任何落盘缓存）；`ScanWithProgress` 在 `Scan` 之上提供分批进度（每 100 个可查询来源通知一次，供 UI 渐进刷新列表）。
- `catalog.go`：纯内存 Catalog：Put/Forget/RemoveMissing/KnownFiles 写入面（Forget 按会话 key 摘行并释放文件路径占用，未知 key 忽略）、列表行描述行提取；文件路径唯一，`(mtime,size)` 为扫描变更信号。
- `catalog_read.go`：只读元数据查询、Provider 集合与各 Provider 非归档会话计数（可限定项目：`ProviderCountsIn`）、Project/Provider 范围过滤（Project = 文件夹及其子文件夹，`withinProject` 是列表/搜索/计数共用的唯一规则；`ProjectContains` 把同一规则给不经 Catalog 的回收站行复用）与 metadata 搜索（标题/项目/路径/id/Provider，大小写不敏感）；同一遍历内完成排序与 LIMIT。
- `legacy.go`：一次性删除旧版遗留的 `history.sqlite3`（含 WAL/SHM/journal）——旧版把会话正文复制进 FTS/页缓存，现已无读者。
- `scanner.go`：多 Provider 来源目录协调与增量索引（变更文件解析一次、只保留列表行元数据）；`SummarizeSource` 解析单个来源文件并给出一行列表摘要（不入 Catalog）；解析走至多 4 个 worker 的小池，行按目录列举顺序提交，结果与顺序扫描一致；首个失败取消本轮且不做清理；每提交 `scanBatchSize`（100）行在锁外通知一次进度观察者（含完成时的尾部一批）。
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
- `catalog_test.go`：内存 Catalog 的写入/排序/搜索/路径唯一/并发（-race）契约与描述行提取；Forget 同时释放行与路径。
- `legacy_test.go`：旧 SQLite 目录清理（只删 history.sqlite3 家族、幂等）。
- `claude_test.go`：Claude 会话解析行为。
- `codex_test.go`：Codex 会话解析行为。
- `history_test.go`：跨 Provider 解析与历史模型语义。
- `live_test.go`：增量读取/解码的边界行为。
- `pi_test.go`：Pi Provider 解析和 live 增量回归。
- `scanner_test.go`：来源扫描、去重与索引协调；同 mtime 大小变化仍重读；每 100 个来源一批的上报契约；SummarizeSource 读回一行摘要。
- `service_test.go`：HistoryService 列表/读取/扫描契约；Open 始终反映原始文件的当前内容（追加后无需重扫）；Forget 后所有读取面一致。
- `service_filter_test.go`：Provider/Project 提前过滤、完整 Provider 集合和各 Provider 计数（排除归档）的回归；Project 范围含子文件夹且不误收同前缀兄弟目录（列表/搜索/计数同一规则）与 `ProjectContains` 表驱动。

[PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
