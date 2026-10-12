# gitworkbench/
> L2 | 父级: ../../CLAUDE.md

Git workbench 是 Shardlane 的本地 Git 域服务：Runner 负责仓库读取与受控变更，nativeui 只能消费快照及显式执行操作。本模块不持有 Herdr Workspace/Pane 权威，也不提供展示组件。

成员清单
- `branches.go`：读取本地分支、只读远程跟踪引用，校验并安全切换/创建本地分支。
- `branches_test.go`：本地分支发现及当前分支测试。
- `remote_branches_test.go`：读取真实 Git refs/remotes、忽略符号 HEAD 的契约测试。
- `cache.go`：按仓库根目录缓存 Git 变更快照。
- `changes.go`：工作树变更查询入口。
- `changes_parsing.go`：Git 状态和变更输出解析。
- `changes_snapshot.go`：变更快照的构建与稳定字段；numstat 先行，超 `LargeDiffLines` 的文件排除出批量读取并延迟（Deferred）。
- `changes_untracked.go`：未跟踪文件补充与界限；超行数/4 MiB 的延迟、大文件二进制嗅探、按需整读（≤16 MiB）。
- `commit.go`：受保护的提交事务。
- `commit_prompt.go`：提交消息提示词生成规则。
- `commit_prompt_preview.go`：生成提示词的隐私限制与预览。
- `commit_prompt_preview_test.go`：提示词预览边界回归。
- `commit_prompt_test.go`：提交提示词规则测试。
- `context.go`：Git 仓库根定位和上下文边界。
- `generated.go`：仓库生成文件辅助规则。
- `helpers_test.go`：测试仓库准备工具。
- `large_diff.go`：大 diff 延迟——批量读取的 pathspec 排除（重命名双侧）、读取溢出整体延迟、延迟文件指纹戳（尺寸+时间）、`ReadFileDiff` 按需读单文件（含两侧 hunks，≤16 MiB）。
- `large_diff_test.go`：真实仓库回归——超阈值/超 4 MiB 延迟、超读取上限不再令快照失败、重命名/未跟踪/提交快照、按需读取与过大错误。
- `highlight.go`：语法高亮适配。
- `history_actions.go`：历史提交撤销/恢复的安全约束。
- `history_actions_test.go`：历史操作边界测试。
- `models_test.go`：领域模型约束测试。
- `mutation_matrix_test.go`：Git 操作拒绝/保护矩阵测试。
- `operations.go`：同步、Stash、Tag、Worktree、含完整父 SHA 的历史提交及 sequencer 操作，供原生历史图谱使用。
- `operations_test.go`：真实 Git 操作和保护回归。
- `commit_parents_test.go`：真实 Git 提交父 SHA 读取回归，支持历史图谱连接。
- `patch.go`：Diff/hunk 补丁构建和应用。
- `repository.go`：Git 仓库元数据读取。
- `runner.go`：带超时、错误分类与单仓库串行的 Git 命令边界。
- `sequencer.go`：merge/revert/cherry-pick 状态探测和完成/中止。
- `sequencer_test.go`：sequencer 事务状态测试。
- `tree.go`：变更文件的目录树、折叠与过滤模型。
- `tree_bench_test.go`：大目录树性能基线。
- `tree_empty_test.go`：空树、过滤边界测试。
- `worddiff.go`：文本差异细粒度 word ranges。

[PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
