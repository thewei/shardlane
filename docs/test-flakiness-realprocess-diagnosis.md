# 满载下真实进程/真实 socket 测试间歇超时——诊断记录（W9, 2026-09-19）

基线：11481fc；herdr 0.9.1；macOS 8 核。本记录是三个负载相关失败测试的
根因证据与测试层修复依据；生产代码行为未改动。

## 失败一：loopback_events events_socket_ready_ping_pong（:126 event frame timeout）

**性质已变：不再依赖负载。** 11481fc 单跑即可确定性复现（裸跑 2/3 失败，
每次恰在 5s 窗口整点超时，hub 一帧未发）。

证据链（临时插桩 + 独立探针，均已移除）：

1. 裸 socket 订阅（python）同一隔离 server：tab.create 后 tab_created/
   pane_created/layout_updated 正常推送——herdr 0.9.1 的事件名称与线格式
   （{"event":"tab_created","data":{...}}，下划线事件名 + 点号订阅词表）与
   shardlane-host 的 HerdrEvent 解析、refreshes_navigation_projection 完全兼容。
2. 插桩 remote 事件 hub：subscribe 成功（收到 ack + health_changed(true)），
   snapshot 正确指向隔离实例，但 rx 永远收不到事件；hub 的两个 reader 线程
   直到测试结束 server 被 kill 才 EOF——连接一直健康，纯粹没有数据。
3. **定时实验定位注册延迟**：订阅后立即（0s）触发 tab create → 事件 2/2
   丢失；订阅后 ≥1.5s 再触发 → 4/4 送达。herdr 0.9.1 的 events.subscribe
   注册是异步生效的，ack ≠ 已进入事件路由。

结论：旧断言"订阅后立刻触发变更，5s 内必须收到结构事件"依赖了旧版 herdr
同步注册行为；0.9.1 下快跑必失败，慢（满载）反而可能通过——与"负载抖动"
的历史表象相反。修复：断言改为"变更 → 结构事件最终送达"（必要时重触发
tab create，最多 3 轮 × 4 帧 × 10s），并放宽 ready/pong/握手窗口。

## 失败二：shared_tui open_is_idempotent…（:1653 restart panic）

archive 留档的 panic 消息为 "previous Herdr TUI child did not exit within
5000ms"——production 的 RESTART_REAP_TIMEOUT fail-closed 路径。
机制证据：

- portable-pty 0.9.0 的 std::process::Child kill 实现 = 先 SIGHUP、250ms 轮询、
  再 SIGKILL；herdr tui 收 SIGHUP 立即退出（探针 3/3，8 核 yes 满载下同为 0ms）。
- 单测试 + 8 核满载 yes 压力 3/3 通过 → 纯 CPU 饱和不足以触发。
- 仅跑 shared_tui::tests（同二进制 3 个测试并行，各自拉真实子进程连接同一
  用户 herdr server）5 轮 3 败 → 触发条件是同进程内多个真实子进程并行。

同一并行复现还抓到第二失败模式：startup_replay_does_not_freeze… 的
"first subscriber at spawn time sees empty prefix"——open() 返回后、首订前，
reader 线程可能已把真实子进程的 DECSET burst 写入前缀，纯调度竞态。

修复：三个真实子进程测试用进程级 Mutex 串行化；空前缀场景改为
"赢则验证、输则换新子进程重试（≤5 次）"；ends_with 放宽为 contains
（真实子进程字节可与合成 marker 交错，契约是包含而非尾部位置）。

## 失败三：herdr.rs workspace_state 类（scripted fake socket）

scripted_herdr_server_actions / scripted_request_recorder 旧实现：spawn 线程
bind 后忙等 socket 文件出现，上限 2s；bind 失败时线程静默返回。满载下线程
调度 + bind 延迟超 2s → 下游 connect 立即 SocketUnavailable panic。修复：
显式 bind 握手（channel）+ 15s 死限 + bind 失败显式 panic，消除静默与竞态。

## 修复边界与验证

- 全部改动限于测试层（测试文件、tests/common、测试模块内常量）；生产代码
  零改动（git diff 可证）。
- 验收：人为负载（8×yes + 全 workspace 并行测试）连续 3 轮无三家失败；
  空载全绿；fmt/clippy/test/build 四门禁全过。
