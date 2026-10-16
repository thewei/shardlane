# herdr/
> L2 | 父级: /CLAUDE.md

成员清单
actions.go: Herdr Workspace/Tab/Pane 的权威 mutation RPC 适配器；支持 workspace.move、tab.move、pane.swap、pane.move、pane.resize 以及 pane.zoom 显式 mode（toggle|on|off，live schema PaneZoomMode）
actions_contract_test.go: SplitPane RPC 与 mutation → snapshot 时序契约；ZoomPane 显式 mode 线格式与非法 mode 本地拒绝契约
reorder_contract_test.go: Workspace/Tab/Panes 排序、跨 Tab 移动与分栏调整 RPC 的 method、字段、回读与输入校验契约
herdr.go: Herdr Manager、PATH/常见安装位置优先的 CLI 发现（env → PATH → 安装位置 → 登录 shell）及受限时 session list（6 秒）；RunCLI 是应用侧唯一的 CLI 一次性调用入口（runCLIEnv 是其 env 覆盖内部门），ErrCLINotFound 是“没装 Herdr”的哨兵；MinSupportedProtocol 与 ErrProtocolTooOld 是协议兼容门（pingRPC/ping 分离，EnsureRunning 遇旧存活服务器绝不二次拉起），ProbeRuntime 把会话 socket 的活体身份暴露给设置页；UpgradeRuntime(ctx, session) 跑 `herdr update --handoff` 后回 ping 验证，旧服务器若活着经 `server stop`（sessionEnv 以 HERDR_SESSION 定域）请下台，升级语义全归 Herdr CLI；普通 `herdr update` 因安装目录不可写失败时（updateNotWritable 认 CLI 原话），自动经 RunAuthorizedUpdate 走系统授权（Linux pkexec --disable-internal-agent / macOS osascript administrator privileges，GUI 不经手凭据）在原位完成升级；仅当授权也被拒或无授权代理才返回 ErrUpdateNotWritable，由界面给 UpdateAsRootCommand（sudo herdr update --handoff）终端出路
instances.go: Session 列举与实例身份的解析、管理
instances_test.go: Session 实例发现与边界回归
rpc.go: socket RPC 请求/响应、超时与交付不确定性边界
ref.go: Herdr runtime identity 归一化
projection.go: session.snapshot 到客户端一次性投影的解码、Splits 分栏与布局结构；协议门共用 herdr.go 的 protocolError（旧协议错误可被 errors.Is 识别）
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
herdr_test.go: Manager/协议基础回归（含 ErrProtocolTooOld 哨兵与 EnsureRunning 对旧存活服务器只 ping 一次、绝不二次拉起的回归）
run_cli_test.go: RunCLI 经解析出的二进制执行、失败时带上 stderr 的回归；UpgradeRuntime 决策表（兼容交接不动 stop、旧服务器存活则 stop、命名 session 定域、无服务器不 stop、更新失败浮出 stderr）
actions_test.go: Herdr 操作基础测试

[PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
