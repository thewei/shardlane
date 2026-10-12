# apphook/
> L2 | 父级: /CLAUDE.md

成员清单
event.go: 核心事件契约定义，StatusEvent 与 SessionEvent 及 Handler 回调模型
server.go: 进程内 Unix Domain Socket 服务，多协议载荷（JSON-RPC、直接 JSON、Envelope、Tab 文本）解析与状态规范化
server_test.go: Hook 服务全协议格式解析、死锁防范与 Socket 生命周期回归测试

[PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
