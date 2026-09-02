---
name: ui-acceptance-testing
description: Use when driving or auditing Shardlane macOS UI acceptance, hosted Herdr TUI/multiplexer behavior, OCR or screenshot evidence, Computer Use/MCP connection discovery, or isolation. Prefer app-scoped Computer Use through mcp__node_repl__js and @oai/sky; require explicit opt-in before any global CGEvent/System Events fallback.
---

# UI Acceptance Testing

这是一条“先隔离、再驱动、后断言”的真实应用验收链。它只验证用户可见行为，运行时事实仍由 Herdr/tmux/API 提供；不要把截图或 OCR 当成唯一真相。

## 先读与选驱动

进入真实 UI 验收前读取：

1. [`docs/ui-acceptance-testing.md`](../../../docs/ui-acceptance-testing.md)；
2. [`docs/client-product-architecture.md`](../../../docs/client-product-architecture.md) 的运行时边界；
3. 如果测 Terminal/滚动/节奏，再读 [`docs/performance-engineering.md`](../../../docs/performance-engineering.md)。

驱动顺序固定为：

1. Herdr/tmux/Remote API 等无 UI 断言；
2. Computer Use MCP（`mcp__node_repl__js` + `@oai/sky`），按 App 定向；
3. 原生 CGEvent/System Events，仅在真实硬件节奏、trackpad 或渲染采样不可由 Computer Use 表达时使用，并先设置 `SHARDLANE_UI_DRIVER=native SHARDLANE_ALLOW_GLOBAL_INPUT=1`；需要共享屏幕截图/视频时再加 `SHARDLANE_ALLOW_GLOBAL_CAPTURE=1`。

若当前 Agent 没有 `mcp__node_repl__js`，不要用 shell 的 `osascript`/CGEvent
冒充 Computer Use；先报告 Computer Use connector 不可用，并停在后端/API
断言或等待具备该 MCP 的 Agent。

## 能力探针与组合

先运行无 GUI 能力清单：

```sh
scripts/acceptance-capabilities.py check --driver computer-use --json
scripts/acceptance-capabilities.py mcp-snippet
```

在 Codex 桌面端先启用 Computer Use 插件、server/skill 开关并处理系统权限，
再将 `mcp-snippet` 输出交给宿主的 `mcp__node_repl__js`；`target=mac` 且
`missing=[]` 才能把 `computer-use:mcp` 记为 `available`。`check` 不能从
shell 观察宿主 MCP，因而在未合并导出名时诚实返回 `unknown`；可重复传入
`--mcp-export <name>` 合并结果。`--strict` 只在希望 unknown/blocked 也使
清单失败时使用。

新场景按能力组装：从 `scripts.acceptance.capabilities` 选择探针，从
`scripts.acceptance.assertions` 选择 marker/pane/geometry/zoom 断言，再用
`acceptance-evidence.py` 写控制面事件；这些 Python seam 不启动进程、不发
UI 事件，适合单元测试和其他 Agent 复用。只有完整 mux 验收才调用
`scripts/mux-acceptance.sh --prepare`；动作之后仍须后端断言和 evidence，
不能因能力清单通过而跳过真实行为验证。

## Computer Use 流程

### Sky/GPUI 驱动坑清单（2026-09-02 实测）

- **先激活再发键盘**：app 不是 frontmost 时所有键盘事件会落到当前前台应用。
  用 `NSRunningApplication.activate()`（swift helper）或先点击窗口激活，再驱动。
- **修饰键 chord 可达，纯文本不可达**：`press_key({key: "cmd+b"})` 走真实事件
  路径（GPUI 全局快捷键生效）；`type_text` 与非修饰键（"Return"/单字符）对
  GPUI 自绘 surface 不产生 PTY 字节（疑似 AX setValue 路径，GPUI 未实现）。
  终端输入回显测试必须用原生驱动（evpost）或人工。
- **chord 键名大小写敏感**："cmd+b"/"Return" 合法；"Cmd+b"/"enter"/"escape"
  报 keyNotFound。
- **GPUI 窗口不暴露内部 AX 元素**：AX 树只有红绿灯按钮，所有控件只能按像素
  坐标点击；set_value/AXPress 对内部控件全部失效。
- **vocr 坐标系不一致**：全窗裁剪（0,0）输出 window-relative 坐标；带偏移裁剪
  （x>0 或 y>0）输出 screen 坐标。每次点击前用同一基准现抓坐标，不要跨调用
  复用。
- **窗口底边 ~50px 内点击会被 Sky 拒绝**（windowNotFoundAtPosition），底部
  控件（如 sidebar footer chip）点到向内偏移的位置。
- **同一 bundle id 只能有一个实例**：launch 前杀旧实例（mux-acceptance.sh
  已自动打包 dev bundle 并清场）；直接 exec 裸二进制不注册 LaunchServices
  坐标系，必须走打包后的 .app 路径。
- **bind 启动首屏是 New Agent 页（overlay）**：点击 sidebar 里已绑定的项目行
  是设计上的 no-op（open_or_jump_project 跳本窗口），不会退出该页；工作面
  被覆盖时终端输入必然无焦点。退出途径：提交任务、或（修复后）backend 无
  agent 能力时 bind 直接落终端工作面。输入回显测试前先确认工作面可见
  （OCR 应看到 shell prompt，而不是 What-should-we-do heading）。
- **合成鼠标点击触发不了 GPUI 自定义 on_click**（sidebar 行、tab strip、
  NewTabKind 选择器）：HID tap、clickState、PostToPid 三种投递实测均无效；
  合成点击只能聚焦 gpui-component 的 Input。驱动 Pane/Tab 操作走 AppKit
  菜单栏 AXPress（System Events click menu item，GPUI 菜单栏是原生 NSMenu，
  AX 全通），配 tmux/herdr 后端断言。
- **tmux 断言目标是 attach client 的 CURRENT window**：多 window session 里
  第一个 window 永远不会被 client resize/输入（window-size=latest 只作用于
  viewed window）；capture-pane / display-message 一律用 session 名（current
  window），不要硬编码窗口序号。
- **前台竞争期激活会被静默忽略**：用户在用机器时 activate / System Events
  frontmost 都可能失效，keystroke 会落进别的 app。激活后必须回读 frontmost
  验证，失败重试或放弃该轮断言。

在一个长生命周期终端中准备隔离运行时：

```sh
scripts/verify.sh ui
SHARDLANE_UI_DRIVER=computer-use \
  scripts/mux-acceptance.sh --driver computer-use --prepare --keep
```

命令会创建临时 `HOME`、专用 Herdr Unix socket、专用 tmux socket、唯一 lag log，启动一个 Shardlane 进程，然后打印 `manifest.env` 并在 `SHARDLANE_ACCEPTANCE_TIMEOUT_SEC`（默认 900 秒）内等待 `PASS`/`FAIL` 标记。它不会调用 CGEvent、`osascript` 或 OCR。读取 manifest 后，用 `mcp__node_repl__js` 驱动其中的 `APP_PATH`：

```js
globalThis.sky ??= (await import("@oai/sky")).sky;
let state = await sky.get_app_state({app: "/absolute/path/from/manifest"});
nodeRepl.write(state.text);
```

每个动作后重新调用 `get_app_state`，重新解析 AX `element_index`；优先 `element_index`，坐标只在 AX 不可用时使用。动作完成后在普通终端用 manifest 中的 tmux socket 做后端断言，再创建 `$RUNTIME_DIR/PASS` 或 `$RUNTIME_DIR/FAIL`，让准备进程回收它创建的进程与目录。完整 MCP 例子和确认边界见 [`references/computer-use-mcp.md`](references/computer-use-mcp.md)。

Computer Use 是本仓库的默认 UI 驱动，因为调用面按 App 定向且不需要本仓库自己合成全局事件；当前公开 Sky API 没有“虚拟显示器/虚拟光标已启用”的可查询标志，因此不得把“virtual cursor”写成未经服务证明的断言。若动作退化为全局输入、无法证明目标 App 或窗口隔离，停止并改用无 UI 断言或请求用户显式授权原生路径。

App 定向只隔离验收 Agent 的输入，不会阻止用户亲自改变同一个测试窗口；用户可以继续使用其他 App，但应避免触碰被验收窗口。

## 断言与证据

- 输入回显：`tmux capture-pane` 中出现测试 marker；marker 必须是本次运行生成的非敏感值。
- resize：`tmux display-message` 的 pane 宽高变化。
- Split/Close/Zoom：`list-panes` 数量和 `window_zoomed_flag`。
- bind/连接：隔离 lag log 中的 `bind.connected`/`bind.ok`/`bind.err`。
- OCR/截图：只作为定位和诊断；`vocr --pid <APP_PID>` 捕获目标窗口并返回 `text<TAB>x,y,w,h`，不得扫描整块用户屏幕；无 PID 的 `vocr` 还需要 `SHARDLANE_ALLOW_GLOBAL_CAPTURE=1`。

`mux-acceptance.sh` 的每个阶段写入 `scripts/acceptance-evidence.py` 生成的
`evidence.jsonl`；native pacing/scroll 回退继续使用各自的 bounded trace 与
result 文件。事件只包含 driver、phase、status、短消息和数值指标，不写
Terminal 文本、Prompt、路径之外的个人数据或密钥。结束前运行：

```sh
scripts/acceptance-evidence.py summary "$RUNTIME_DIR/evidence.jsonl" \
  --require-phase runtime --require-phase bind --require-phase surface \
  --require-phase input --require-phase resize --require-phase menu \
  --require-phase switch
```

通过条件是：隔离 manifest 完整、所有必需阶段有 `pass`、后端断言与 UI 动作一一对应、evidence 校验无 invalid 行，且准备进程已经回收自己的 PID。证据字段见 [`references/evidence-schema.md`](references/evidence-schema.md)。

## 原生回退护栏

`keyrepeat-pacing-ab.sh`、`scroll-tracking-ab.sh`、`scrollbar-continuity-ab.sh` 和 `evpost` 会移动真实指针、抢前台并注入全局事件；它们默认拒绝运行。只有明确的硬件/渲染测量才这样调用：

```sh
SHARDLANE_UI_DRIVER=native SHARDLANE_ALLOW_GLOBAL_INPUT=1 \
SHARDLANE_ALLOW_GLOBAL_CAPTURE=1 \
  scripts/keyrepeat-pacing-ab.sh run before --samples 3
```

原生路径必须在单个阻塞脚本内完成，使用临时 `HOME` + 专用 socket，结束后恢复输入源并清理自己的 PID；不要与用户交互或另一个 UI smoke 交错。`report`、纯 unit/contract test 和后端 CLI 断言不需要该授权。

## 完成检查

验收完成当且仅当：驱动选择和授权边界已记录；运行时与持久化状态隔离；动作后的 AX 状态已重新读取；每个 UI 行为都有独立后端真值；evidence JSONL 可校验；失败时保留 `--keep` 运行目录；最后通过项目规定的 fmt/clippy/test/build/diff 检查。不要用“截图看起来正确”替代任一条件。
