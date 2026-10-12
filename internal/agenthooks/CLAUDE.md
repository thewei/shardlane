# agenthooks/
> L2 | 父级: /CLAUDE.md

成员清单
script.go: 内嵌通用 Agent hook 脚本模板（v5，与已验证在装脚本逐字节一致）与所有权/版本标记
agenthooks.go: Hook Doctor 领域包：Provider 诊断（脚本版本+接线完整性）、安全幂等安装（原子写入、拒绝覆盖外来脚本、备份、最小化改写 ~/.claude/settings.json 与 ~/.gemini/config/hooks.json 的 hooks 段）
agenthooks_test.go: 诊断/安装/幂等/外来文件保护/版本升级回归（临时 HOME，全路径注入）

边界
- 只写两类文件：自有脚本（~/.shardlane/hooks/）与 provider 配置的 hooks 段；其余内容逐值保留
- Aider 无 hook 机制，诊断为 unsupported（状态由 Herdr 屏幕检测兜底），永不安装
- 脚本本身不记日志、失败静默（fail-open）；生命周期权威仍在 Herdr

[PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
