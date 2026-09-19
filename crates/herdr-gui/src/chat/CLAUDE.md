# chat/

> L2 | 父级: ../CLAUDE.md

Chat 语义 sidecar 的状态域与 GUI 域：同一 Herdr Agent 的只读投影 + 安全 prompt 通道，
永不启动 provider 进程、永不成为第二会话权威、永不解析 TUI/ANSI。

## 成员清单

- mod.rs: 模块根；声明 model/approvals/surface 并导出 WorkSurfaceMode/ChatUi。
- model.rs: Chat 纯状态机（绑定身份、provider 归一、发送状态机、pending 提交对账、
  审批投影接线、rows 投影缓存）；零 GPUI、零 I/O。
- approvals.rs: 审批请求生命周期纯状态机（Waiting/Stale/Sent/Done/Unknown）、
  RAW 网格文本哈希（FNV-1a）、永不盲发守卫 guard_matches（未捕获签名/非 Waiting
  一律拒绝）、facts 到本地投影的 reconcile；键序列只来自 provider adapter 的已验证规则。
- surface.rs: GUI 域（ChatUi + ShardlaneApp chat 方法组）：绑定解析、live 同步 worker、
  虚拟化转录渲染、composer、审批 chip（Stale/Unknown 禁用 + 整行提示）、守卫通过后经
  既有 PTY 输入 seam（queue_terminal_text 到 TUI_TARGET）重放键序列，并以 RAW 帧
  哈希变化确认 Done（3s 超时进 Unknown）。

## 审批（#2）铁律

永不盲发：守卫不匹配即 Stale 且一个键都不发；provider 审批形态不可稳定解码则
该 provider 不出 chip。keys 的唯一权威在 herdr-history 的 codex adapter
（vendor keymap 默认 + CODEX_HOME config.toml 覆盖）。

[PROTOCOL]: 变更时更新此头部，然后检查 ../CLAUDE.md
