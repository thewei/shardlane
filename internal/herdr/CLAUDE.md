# herdr/
> L2 | 父级: /CLAUDE.md

成员清单
actions.go: Herdr Workspace/Tab/Pane 的权威 mutation RPC 适配器；拖放经 workspace.move、tab.move、pane.swap、pane.move 提交后重新读取 snapshot
actions_contract_test.go: SplitPane RPC 与 mutation → snapshot 时序契约
reorder_contract_test.go: Workspace/Tab/Panes 排序与跨 Tab 移动 RPC 的 method、字段、回读与输入校验契约
herdr.go: Herdr Manager、PATH/常见安装位置优先的 CLI 发现及受限时 session list（6 秒）
instances.go: Session 列举与实例身份的解析、管理
instances_test.go: Session 实例发现与边界回归
rpc.go: socket RPC 请求/响应、超时与交付不确定性边界
ref.go: Herdr runtime identity 归一化
projection.go: session.snapshot 到客户端一次性投影的解码和布局结构
projection_test.go: snapshot 数据与 runtime 标识映射回归
bridge.go: SSH Unix socket 转发、远程 RPC 通道生命周期
events.go: Herdr 增量事件订阅与投影校正触发
events_test.go: 订阅事件字段与时序的协议回归
launch_transport.go: Agent 启动与无焦点 Tab 创建的安全流程
launch_transport_test.go: Agent 启动事务与远程边界契约
process_info.go: Pane 前台进程元数据适配
process_info_test.go: 前台进程元数据回归
machines.go: 远程机管理及状态入口，启动 machine list 读取 6 秒 deadline、防止阻塞工作区
machines_test.go: 机器配置与状态回归
search_gate.go: 搜索、复制等 Herdr 搜索协议适配
search_gate_test.go: 搜索消息边界回归
scroll_contract_test.go: Herdr scroll 消息与状态协定
herdr_test.go: Manager/协议基础回归
actions_test.go: Herdr 操作基础测试

[PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
