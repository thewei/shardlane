# Herdr Attach 通道鼠标事件转发规格（F144 选项 a）

> 状态：**已归档为未来对齐项**（2026-10-06 用户最终拍板：客户端「真终端语义」改造先行，见台账 F144；
> 本规格所描述的 daemon 侧输入路由若未来落地，可让程序级鼠标事件不再依赖客户端 conn 层转发，
> 届时客户端再评估收敛）。规格内容保持不变，供 herdr 侧参考。
> 客户端对应缺口：`docs/ui-computer-use-findings-2026-10-06.md` F144（终端拖选无反应）。
> 铁律遵守：本规格**不发明任何新协议方法/套接字形状**——全部落在既有 `herdr terminal attach`
> 字节流通道上；每个行为断言都附 0.9.3 实机探针证据。

## 1. 背景

Shardlane MyGo 客户端的可见 Pane 全部经 `herdr terminal attach <terminal_id>` 承载。
MyGo 终端视图自带的拖选/右键/滚轮语义被客户端的 `mouseModeFilter` + 滚轮接管子层
（`next/internal/nativeui/mousemode_filter.go`、`terminal.go` paneCard）主动禁用，原因是
attach 通道**输入方向丢弃鼠标字节**（下探针）。副作用是视图收不到任何指针事件 →
拖选永远无法发生（F144 根因，完整推理见台账）。

## 2. 实测真值（herdr 0.9.3，2026-10-06，scratch 会话探针）

| 探针 | 结果 |
| --- | --- |
| 普通 ASCII + `\r` 写入 attach stdin | ✅ 到达 pane PTY（pane 内 `cat` 回显，`pane read` 可见） |
| SGR 滚轮 `\x1b[<64;10;5M` / 按压 `\x1b[<0;10;5M` 写入 attach stdin | ❌ 静默丢弃（cat 一无所获；与 2026-10-05 探针一致） |
| daemon→client 渲染方向 | 不变：仍无条件下发 `?1000h/?1002h/?1003h` 强制上报 |

结论：**输入传输层活着，死的只是"鼠标形状的输入"的解析与路由**。这是 herdr 侧一个小而清晰的缺口：
attach 输入解析器认得普通字节（→PTY），但不认得鼠标序列（→垃圾桶）。

## 3. 提议语义（attach 字节流级别，无新 RPC）

daemon 的 attach 输入解析器在现有"普通字节 → pane PTY"之外，增加对鼠标序列的识别与路由：

```
attach stdin 字节流
  ├─ 非鼠标字节            → 现状不变：直写 pane PTY
  └─ 鼠标序列（SGR/X10）   → 解码 → 按 pane 程序的 mouse-tracking 态路由：
       ├─ 程序未开 tracking（普通 shell、less/vim 默认）
       │    ├─ wheel        → 复用 daemon 自身 TUI 的滚轮语义 = 移动 pane 权威
       │    │                 视口（等价现 pane.scroll），下帧 attach 重渲染
       │    └─ press/click  → 复用 daemon 自身 TUI 的点击语义 = 聚焦该 pane
       │                     （与内置 TUI 的 click-to-focus 一致）
       └─ 程序已开 tracking（pi/agent TUI、vim :set mouse=a、htop…）
            └─ 全部事件      → 原样透传 pane PTY（SGR 编码、pane 栅格坐标），
                              程序自己消费——与用户直接坐在 daemon 内置 TUI 前等价
```

要点：
- **路由真值是 pane 程序的 tracking 态**，不是 attach 客户端的。daemon 解析 PTY 流渲染单元格，
  程序的 DECSET 1000/1002/1003 本来就在它的解析路径上，状态现成。
- 坐标系：客户端视口栅格 == pane 栅格（attach 的 `Resize` 跟随视图栅格，已是现状），
  SGR 坐标 1:1 可用，无需换算。
- 事件在「客户端按下–移动–释放」期间到达的顺序必须保序（拖选手势依赖）。

## 4. 编码与能力协商

- 客户端发送 **SGR（DECSET 1006）编码**（MyGo 终端视图 `vt.MouseEncoder` 的默认输出）；
  urxvt/1005/1016 像素编码 daemon 可拒绝（忽略），客户端不依赖。
- 客户端不猜 daemon 版本：以 session.snapshot 既有 `protocol` 字段（或等价能力位）判定
  转发可用后才翻转行为（见 §6）。daemon 落地时提升该值即可。

## 5. 兼容性

- 旧客户端从不发送鼠标字节 → 输入解析新增分支对它们零影响。
- 渲染方向的强制 tracking 下发保持不变（旧客户端继续靠客户端侧 filter 剥离）。

## 6. 客户端退役清单（daemon 落地并经探针验收后执行）

1. `attachConn` 移除 `mouseModeFilter`：tracking 下发放行 → 视图进入原生上报模式。
2. `paneCard` 移除滚轮接管子层（HandleInput 覆盖层）——**F144 根因随之消失**，视图恢复
   指针事件：拖选（tracking 关时本地手势）、上报（tracking 开时 SGR）。
3. 退役 `scrollSurface` / `sendAgentWheel` 的 UI 层接管（daemon 已按 §3 路由滚轮）。
4. 右键语义：tracking 开时右键=程序事件；应用 Pane 菜单改挂 ⌘+右键或长按（实现期定）。
5. 回归：`TestAttachTrackingModesNeverForwardRightClick` 与 mousemode_filter 全套测试按新
   契约改写/退役；台账 F144 复核后关账。

## 7. 验收探针（daemon 侧交付验收用，可在 scratch 会话复跑）

1. pane 跑 `cat`；attach stdin 写 `\x1b[<64;10;5M` → `pane read` 必须出现原样 SGR 字节
   （现状：无）。等效于程序 tracking 场景。
2. 普通 shell pane：attach 写 SGR 滚轮 → `session.snapshot` 中该 pane 的
   `scroll.offset_from_bottom` 变化（现状：不动）。
3. 普通 ASCII 输入路径不回归（`hello-attach\r` 仍到 PTY）。
