# .agents/skills/
> L2 | Parent: ../../CLAUDE.md

成员清单

- `herdr-client-development/SKILL.md` — Shardlane Go/MyGo 原生客户端开发、Herdr 协议纪律与验证流程。
- `shardlane-install/SKILL.md` — 构建工作树最新 Shardlane.app 并覆盖安装、优雅重启、存活校验（`scripts/install.sh`）。
- `shardlane-ui-audit/SKILL.md` — Computer Use 实机巡检 → 代码验证 → 修复 → 复核的循环、F 台账与并发协议。
- `shardlane-ui-polish/SKILL.md` — 统一界面与去 AI 味的规则手册：五条统一不变量（标题只在标题栏、进出内页、单一刷新、两类高亮、同组件同尺寸）、token/文案/动效约定、反模式扫描与打磨循环。

单一来源布局：技能只在 `.agents/skills/` 维护；`.claude/skills` 是指向 `../.agents/skills` 的软链（`.claude/` 在 `.gitignore` 中，软链为本机副本，重建命令 `ln -s ../.agents/skills .claude/skills`）。

（Rust 时代的 ui-acceptance-testing / shardlane-mobile-development / shardlane-hook-selftest 技能随 Rust 实现一起移除，历史版本见 `rewrite/mygo`。）

法则: 技能单一职责·Computer Use 优先·全局输入显式授权·证据可校验
[PROTOCOL]: 变更时更新此头部，然后检查 ../../CLAUDE.md
