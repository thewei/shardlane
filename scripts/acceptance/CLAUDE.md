# acceptance/
> L2 | 父级: ../CLAUDE.md

成员清单

- `capabilities.py` — 无 UI 副作用的能力状态、驱动护栏探针、组合报告，以及供 Agent 执行的只读 Computer Use MCP 探针代码。
- `assertions.py` — 基于 tmux/后端输出的 marker、pane 数量、几何变化和 zoom 断言；不启动进程、不操作 UI。
- `__init__.py` — 对外导出能力探针与后端断言的稳定 Python seam。

法则: 能力可组合·后端为真值·驱动先判定·探针不产生副作用
[PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
