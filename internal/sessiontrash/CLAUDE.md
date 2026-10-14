# internal/sessiontrash/
> L2 | 父级: ../../CLAUDE.md

Sessions 回收站：Shardlane 搬动 Provider 会话文件的**唯一**位置，且只往返于自己拥有的回收站目录（`<userDataDir>/SessionTrash/<id>/`）。删除 = 把会话 `.jsonl` 从 Provider 自己的历史根目录搬进 `<id>/`，旁边一份只含 `{version, agent, original, deleted_at_ms}` 的清单（**不含任何会话内容**）；放回 = 搬回精确的原路径且从不覆盖；永久删除/清空只删除回收站里合法 id 的条目目录，从不碰别的。回收站列表的标题/项目/计数在读取时经 `history.SummarizeSource` 从被搬走的文件现读，磁盘上不存在第二份由会话派生的文字。无 UI、无 Herdr 依赖；`internal/history` 保持只读，本包拥有文件操作，history 拥有"会话是什么"。

成员清单

- `trash.go`：Store（`New(dir, roots)`；`Trash/Restore/Delete/Empty/Count/List/Dir`）与 `Item`。安全面——只接受落在**同一 Provider** 某个来源根内的绝对 `.jsonl` 路径（词法 `within` + 解析符号链接后的父目录 `realWithin`），源必须是普通文件（`Lstat`，拒绝符号链接/目录），`BusyWindow`（60 秒）内刚写过的文件拒绝（`ErrBusy`）；条目 id = `<unixMillis>-<8 hex>` 经 `validID` 校验，从不当路径用；先写清单再搬文件（崩溃安全），无载荷的孤儿条目由 `List` 清理；`moveFile` 用 `os.Link` + `os.Remove` 原子不覆盖（`ErrExists`），跨卷回退 `copyThenRemove`（O_EXCL、fsync、保留 mtime、成功后才删源）；放回时重新校验清单里的原路径，缺失的项目目录只在词法校验通过后重建；列表行按 (路径, mtime, size) 缓存。
- `trash_test.go`：往返（含 mtime 保持）、忙碌保护、路径守卫（符号链接、符号链接项目目录、`..`、错 Provider、根/目录/空/相对路径）、不覆盖、重建缺失项目目录、被篡改清单拒绝（含不存在的父目录不建文件夹）、永久删除/清空只动条目与非法 id、列表新到旧与孤儿清理与不可读载荷、条目形状（0700）与清单不泄漏内容、并发（-race）、跨卷复制回退（含不覆盖）、失败回滚。

法则: 只搬、不读正文·只在自己的目录里删·放回不覆盖·路径永远先校验再动
[PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
