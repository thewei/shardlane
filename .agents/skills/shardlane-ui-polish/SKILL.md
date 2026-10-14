---
name: shardlane-ui-polish
description: Use when polishing or unifying the Shardlane MyGo client's UI/UX — removing "AI flavor" (self-explaining copy, leaked internals, gradients, heavy icons, per-row buttons), unifying page Headers / back navigation / refresh / highlight colors / icon buttons / spacing / radius / dividers / motion across pages, adding or restyling a page or shared component, or when the user says 界面不统一 / 去 AI 味 / 重复按钮 / Header 不一样 / 高亮色不一致. Holds the design rulebook (tokens, title-bar model, one-refresh and two-class highlight rules, copy conventions), anti-pattern scan commands, regression guards, and the audit → fix → evidence → gate loop. Real-device driving quirks and the F-ledger live in shardlane-ui-audit; deploying lives in shardlane-install.
---

# Shardlane UI 打磨：统一界面 · 去 AI 味

一句话：**一个概念只有一个入口、一处标题、一种外观；系统惯例优先于新点子。**
每条规则都挂着守卫测试或 `rg` 扫描——没有可验证手段的规则不写进来。

## 先读 / 分工

1. `AGENTS.md`（owner 阶梯：Herdr API → MyGo 原生 → 成熟做法 → 自写；Shardlane 只做呈现）。
2. 视觉单一来源：`internal/nativeui/design_system.go`（token）、`gorex_style.go`（图标按钮/开关脸/卡）、`ds_components.go`（页面原语）、`route_header.go` + `titlebar.go`（标题栏）。
3. 设备驱动坑、F 台账、并发协议 → `shardlane-ui-audit`；覆盖安装 → `shardlane-install`。本技能只管「规则 + 打磨循环」。

## 五条统一不变量（违反任一条即缺陷）

| # | 不变量 | 唯一实现 | 守卫 |
|---|---|---|---|
| 1 | **标题只在标题栏，且是一行大字**：Changes/Sessions/Files/Hosts/Settings/Inspector 的标题 = 标题栏里一行 `Title`(20) 加粗；子页前缀灰色父级（`Settings ›`、`Sessions ›`，会话详情的父级可点回列表）；页面动作紧随其后；**没有 crumb 胶囊、没有第二行上下文**——路径/配置路径放标题悬停提示（`titleTip`），会话事实（项目 · Agent · 模型）同行灰字。只有 Terminal/Chat 是面包屑 + cwd 双行。页面只画内容，从 `pageTopInset()`（12）起；侧栏不回声标题 | `routeHeader()`/`routeHeaderBlock`/`headerActions`（Terminal/Chat 走 `breadcrumbRow` + cwd 行）；`titlebarHeight` 全路由同一 42 下限、标题同字号同行 → 进出内页标题不跳；内页侧栏列表由 `sidebarInnerNav` 统一内缩，子导航不再二次 `Padding`（Hosts 曾因此低 14、窄 9） | `unified_header_test.go`（含 `TestInnerTitlesAreOneLargeRow`、`TestInnerSidebarsShareOneInset`） |
| 2 | **进出内页同一套路**：进入 = **侧栏顶部四个等宽图标视图标签**（Changes/Sessions/Files/Hosts，每条路由同一位置；再点亮着的回 Terminal，Sessions 内页先回列表）；退出 = **侧栏页脚唯一返回键**，一律叫 `Back`（tooltip 写去向：会话详情→Sessions，其余→Terminal）；页内不再放第二个返回；⌘W 只关 Tab；窗口标题 = `Shardlane — <路由>` | `sidebarViewTabs`/`gorexIconTab`、`showViewHistory/Files/Hosts`、`sidebarBackRow`、`syncWindowTitle` | `navigation_semantics_test.go`、`TestSidebarViewTabs*`、`TestSessionBarLeadsBackToTheList` |
| 3 | **同类动作每个上下文只有一个**：Refresh 是图标按钮、不是导航行；同一目的地只有一个名字（Files/Sessions/Hosts，菜单/⌘K/tooltip/窗口标题一致）；信息只出现一次（窗格卡头不复述标题栏已有的 cwd） | Workspace/Files 侧栏项目头图标；Hosts 侧栏列表头图标；Sessions 工具条图标；Agents 设置工具条图标；`paneCardCaption`/`titleSpellsOutDir` | `TestEachRouteHasOneRefreshAndItIsAnIcon`、`pane_header_visuals_test.go` |
| 4 | **高亮只有两类，各一种脸**：*按钮*处于开（开关/视图工具/模式）= 中性芯片；*行*被选中（导航/树/列表/Agent 卡）= `SidebarSelect`。业务代码不再自涂 Accent 底 | `gorexToggleFace(b,k,on)`（按下 `k.pressed` / 悬停 `k.hover` / 开 `k.tabActive`）；行走 `rowTint`；"当前分支/当前引用"是状态标记（accent 字 + 0.11 底），不是选中 | `toggle_face_test.go` |
| 5 | **同一组件同一尺寸**：图标按钮只有两档——工具栏 28/16（`gorexIconBtn/Glyph`）、密集 24/14（`gorexPaneBtn/Glyph`），全走 `gorexIconButton`（等分一行的标签条走同脸的 `gorexIconTab`）；文字按钮只有 regular 与 `compactButton`（Micro 10）；"…" 文本按钮一律换图标按钮/`gorexPulldown`；空态 `emptyState`/`workspaceEmptyBanner`、加载 `loadingState`、提示 `inlineNotice`、分组 `settingsGroup/formRow`、内页列宽 `contentColumn(760 表单 / 1040 列表)` | 见左 | `copy_style_test.go`（空态标题）+ 下方扫描 |

## 去 AI 味配方（本项目真实做过的，按维度）

| 维度 | AI 味症状 | 做法 |
|---|---|---|
| 画布层次 | 渐变、浮卡、粗边框、大阴影、紫橄榄配色 | 平面中性画布 + 整高 macOS vibrancy 侧栏（无材质回退不透明）；Apple 中性灰 + 系统蓝强调；深度只靠表面对比，**只有终端 pane 是卡片**，内页是平面全幅 `gorexContentCard`；Chat WebView 调色板对齐原生 |
| 字体 | 字号/字重满地飘（650/700/.Bold()、8~16 乱字号） | 只用 `Typography()`：Title 20 / Heading 16 / Section 13 / Body 12 / BodySmall 11 / Caption 10.5 / Micro 10；字重只用 400/500/600/700，**正文最多 semibold，bold 只给页标题和数字**；重点 = 深浅色 + 字号 + 字重的小步进（参考 Pulse）；窗格头标题 12 semibold + 路径 11 regular 弱化色 |
| 图标 | Lucide 粗描边、机器人头像、文本字符冒充图标（↻ ← ↑↓ −） | 全部细线 1.5（iOS 风格）；彩色圆角方块 `iconTile` 只标目的地/类别（设置导航、Agent 列表、Sessions 行），状态一律 `StatusTone`；刷新/返回/展开用 SVG |
| 布局 | 互斥项并排按钮、整页拉伸、窄窗挤成单字列 | 互斥项 → Segmented tabs（Git 侧栏 Local Changes/All Commits、Theme/Density、Sessions Agent 标签条；只有一个来源不显示）；设置页 = macOS 分组列表（原生 Switch、右对齐紧凑控件、行内分隔线）；内页居中最大宽度，宽窗长边距；窄窗 `Wrap().Basis()` 让操作换行到消息下方；侧栏统一对齐网格 + 折叠结构线（`TreeLine`） |
| 间距 / 圆角 / 分隔线 | 5/7/12/16 混搭圆角、一次性间距、各页各画分隔 | Spacing `2/4/6/8/12/20`（XXS…XL）；Radius Control 6 / Row 8 / Card 10 / Pill，图标按钮 `size/2.6`；控件高 Compact 24 / Regular 28 / Row 31；分隔线一律 1px `BorderSubtle`，分组列表里从 `rowInset`（14）起、`attached` 行不画；卡 = `Panel` 底 + 1px `BorderSubtle`，无阴影；页面不写字面量 |
| 交互 | 每行常驻三个按钮、"…" 文本按钮、各页高亮色不同 | 行内操作 = 悬停才现的菜单入口 + 右键菜单，**占位固定，悬停不位移**（有变异测试）；原生控件优先（Switch/Segmented/Pulldown/SearchField）；破坏性确认默认焦点不在 Confirm；**浮层只有一层**——点击条目 = 直达面板并收起浮层，不在浮层里再开详情层/返回键；主动作只有点击，其余进右键菜单，行尾至多一个快捷图标；浮层复用侧栏同一张卡片 |
| 动效 | 装饰性动画、卡顿的重排 | 悬停/按下/开态颜色 120 ms；重排/拖动占位 150–190 ms；浮层进出场 140 ms（Enter Y+6 / Exit Y−4）；侧栏滑动 200 ms（拖动时 1 ms 直落）；Reduce Motion 直接落位；不为装饰加动画 |
| 文案 | 自圆其说、泄漏内部信息 | 见下节 |

## 文案规则

- **说人话**：不出现 operation id / slug / 协议版本 / 状态枚举 / RPC 原文；错误 = 发生了什么 + 下一步（`Couldn't load sessions` + `Try Refresh.`）。
- **不自圆其说**：不写向读者解释设计意图的句子（"One conversation surface over…"、"Native Terminal" toast）；空态 = 一句标题 + 可选一句下一步。标题不带句号，说明句带句号。
- **大小写**：按钮/菜单项/对话框标题 Title Case（冠词与 ≤4 字母介词小写：`Merge into Current…`、`Draft Subject with Pi`；Git 复合词沿用既有写法 `Cherry-pick`）；**空态标题与 tooltip sentence case**。守卫：`TestMenuItemsAreTitleCase`（扫 `.Item("…")` 字面量）、`TestEmptyStateTitlesAreSentenceCase`（专有名词 Git/SSH 除外）。
- **同动作一个名字**：在 Finder 里定位统一 `Reveal in Finder`（Files 树/标题栏 Actions、Git worktree、Settings 日志、Sessions 的 `Reveal Source File in Finder` 只在宾语上多一个限定）；复制路径统一 `Copy Path`。
- **HIG**：打开对话框/需要再输入的项加 "…"；已换成图标按钮/下拉的不再用 "…" 文本按钮；复数用 `pluralS`；名称引号用弯引号。
- **可恢复**：任何截断文本必有 Tooltip；计数先于名称存活。

## 改共享组件的流程（单一真相源）

1. **先扫再改**：改 token/共享组件前跑下面的扫描，记下命中数与文件。
2. **改一处**：在 `design_system.go` / `gorex_style.go` / `ds_components.go` 改，不在业务组件 inline 覆盖。
3. **同步清理**：清掉业务侧同类 inline override；孤儿 helper（如 2026-10-14 删掉的旧 `iconButton`）随手删并更新 README/L2。
4. **全项目扫同类**，再钉测试。

```sh
cd internal/nativeui
rg -n 'pageHeader\('                        --glob '!*_test.go' .   # 页内页头（仅遗留 /chat 允许）
rg -n 'ui\.(Primary)?Button\([^)]*\)\.FontSize' --glob '!*_test.go' .  # Button 内联字号（应走 compactButton）
rg -n 'ButtonBase\(c[^)]*\)[^\n]*Size\((2|3)[0-9], (2|3)[0-9]\)' --glob '!*_test.go' .  # 自制图标按钮
rg -n 'Accent\.Alpha\(|rowTint\(' --glob '!*_test.go' .              # 开关自涂高亮（应走 gorexToggleFace）
rg -n '"Refresh[^"]*"' --glob '!*_test.go' .                          # 每个上下文只应有一处
rg -n 'Radius\((5|7|9|11|12|13|14|16)\)|FontWeight\((6|7)[0-9]{2}\)' --glob '!*_test.go' .   # 字面量圆角/字重
rg -n 'emptyState\(c, "' --glob '!*_test.go' .                         # 空态标题大小写
rg -nP '\.Item\("[A-Z][\w-]+ (?!(?:a|an|the|and|or|in|into|to|of|on|as|from|with|for|at|by)\b)[a-z]' --glob '!*_test.go' .   # 菜单项 sentence case 漂移（应为 0）
rg -n 'Padding\(14, 9, 9\)' --glob '!*_test.go' .                      # 侧栏内缩只应在 sidebarInnerNav 出现一次
```

## 打磨循环（一边测，一边修，一边沉淀）

1. **基线**：改动前先出 headless 证据 `SHARDLANE_UI_EVIDENCE=/tmp/x/before GOTOOLCHAIN=go1.27.1 go test ./internal/nativeui -run 'TestUIEvidence$' -count=1`（深/浅各 12 页：Workspace、Settings 各分组、Sessions、详情、Files、Agent 面板、Hosts）；用户正在用的 App 只做被动观察（cua-driver 后台 AX/截图，边界见 `shardlane-ui-audit`）。
2. **收集问题**，分四类：重复（入口/标题/刷新/路径回声）、不一致（同组件两种脸/尺寸/文案）、AI 味（自圆其说/泄漏/装饰）、断路（死按钮/无 Tooltip 截断）。每条记 `文件:行`。
3. **修**：先判规则归属（五条不变量或 token），改唯一实现，不堆页面特例。需求有歧义（例如"把终端标题移进 Header"）→ 按保守解读做信息去重，并列出备选交给用户，不擅自删结构。
4. **钉**：每个修复一条回归测试 + **变异检查**（`cp` 备份 → 改回旧实现 → 测试必须红 → 从备份还原并复核）。像素断言用 `pixelAt`/`rgbDiff`（`sidebar_tree_test.go`），布局断言用 `Tester.Find` 矩形；路由动画用真实时钟，测试用 `settleFrames`。
5. **证据**：重出 after 图并排对比（`Read` 看 PNG），再看一次浅色与深色。
6. **门禁**（全部实跑）：
   ```sh
   GOTOOLCHAIN=go1.27.1 go test ./... -count=1
   GOTOOLCHAIN=go1.27.1 go vet ./... && GOTOOLCHAIN=auto go tool mygo vet .
   GOTOOLCHAIN=go1.27.1 go test -race -count=1 ./internal/gitworkbench ./internal/nativeui
   (cd frontend && bun run typecheck)          # 动过 WebView 前端时
   GOTOOLCHAIN=go1.27.1 go tool mygo build && plutil -lint build/darwin-arm64/Shardlane.app/Contents/Info.plist
   codesign --verify --deep --strict build/darwin-arm64/Shardlane.app
   gofmt -l <本次改动的 go 文件>   # 既有漂移（gorex_style.go、pane_header_buttons_test.go）不顺手重排
   git diff --check && git diff --cached --check
   ```
7. **部署与复核**：`bash .agents/skills/shardlane-install/scripts/install.sh`（优雅重启，用户已长期授权）；复核只做被动 AX/截图，逐项对照 before。
8. **回写**：规则与锚点更新到本文件「已落地」，结构变化更新 `internal/nativeui/CLAUDE.md`（L2）与新文件 L3 头。

## 已落地（2026-10-14 一轮）

- 标题栏接管所有内页标题/动作：`route_header.go`（Changes、Sessions、Session 详情、Files、Hosts、Settings、Inspector），统一为单行 Title 大字标题（先前是 crumb 胶囊 + 第二行上下文，太拘谨，2026-10-14 第二轮去掉）；`pageHeader` 退出生产页面；侧栏不再回声标题。
- 四个视图按钮（Changes/Sessions/Files/Hosts）从标题栏右上移到侧栏顶部等宽图标标签条（2026-10-14 第二轮）；标题栏工具区只剩 Agent 活动与置顶；侧栏页脚返回键统一叫 `Back`。
- Refresh 收口为每个上下文一个图标；Hosts 页内 Refresh 与侧栏重复的已删。
- 窗格卡头：标签已写出目录或单卡时不再出 cwd（`paneCardCaption`/`titleSpellsOutDir`）。
- 开关脸统一：Files 忽略开关与 Sessions 的 Select/Hidden 从蓝/accent 改为 `gorexToggleFace` 的中性芯片。
- 旧 `iconButton`（28/14，半径 6）并入 `gorexIconButton`：侧栏页脚设置齿轮走工具栏档，Workspace 区 "+" 走密集档。
- 空态标题统一 sentence case；Inspector 路由补回 `gorexContentCard`。
- Hosts 侧栏列表不再二次内缩（原比 Sessions/Settings 低 14、窄 9）：`TestInnerSidebarsShareOneInset`；Hosts 详情操作行三按钮同一档（`Edit File` 原为密集档）：`TestHostActionRowSharesOneButtonTier`。
- Agent 浮层（标题栏/托盘）重排（2026-10-14 第二轮）：顶部 Agents/Services 分段、浮层自身无标题；Agents 按状态分组（Needs attention › Review › Working › Idle，空组省略；分组头 = 状态点 + 名称 + 计数，Services 分区标题用同一构件 `quickPanelGroupHeader`），行复用侧栏 `agentCardRow`；点击行**直达面板并收起浮层**（待交互 → Chat；blocked/failed → Terminal；其余回到该 pane 上次所在面板），行尾 ✓ = 标记已审阅（不跳转），其余动作在右键菜单；去掉详情层、统计筛选卡与浮层标题。高度随内容增长至上限 480：预算常数取自真实布局（`quickPanelChromeHeight/GroupHeight/RowHeight/UsageHeight`），不是估的——估的 72/28/44 实测多出 57 px 空白，`TestQuickPanelHeightHoldsTheContent` 现在钉贴合（含用量行）。
- 菜单项一律 Title Case（Git 分支/标签/Stash/提交/文件夹菜单 20 项）、Finder 定位统一 `Reveal in Finder`、Git 确认框标题对齐菜单（`Undo Last Commit?`、`Delete Tag “x”?`）：`TestMenuItemsAreTitleCase`。

## 已知例外与待办（下一轮从这里接）

- **终端窗格卡头保留**（N19 #7：Terminal⇄Chat 开关在卡头；去掉会改 PTY 尺寸）；只做信息去重，不做结构搬家。
- Git 工作台自带一套图标按钮/下拉（`gd_surface.go` 的 `gdIconButton`/`gdPulldown`，28/半径 7、灰度 alpha 悬停）与 `git_review_split.go:202`、`page_history.go:436`（行内 "…"）、`gd_surface.go:965` 的 5/7 圆角小按钮：与 `gorexIconButton` 同类不同脸。换壳要先确认 Box 版按钮的键盘焦点/`FocusVisible` 语义。
- 两个 Commit 主按钮（`commit_surface.go:168`、`git_review_split.go:189`）内联字号，未入 `compactButton` 档。
- Git 下半区标签条（Commit/Changes/File Tree）选中脸是 `SurfaceHover`，Segmented 用 MyGo 原生脸。
- `/chat` 遗留页仍用 `pageHeader` 与自涂 `Content` 底（仅测试/遗留路由）；`Refresh Runtime` 菜单项保留（命令级入口，不是页面按钮）。

## 反模式速查

| 症状 | 规则 | 落点 |
|---|---|---|
| 页面里又画了一遍标题/面包屑/路径 | 不变量 1/3 | 删页内头；路径放标题悬停提示（`titleTip`）或页面内容里，不要再加第二行 |
| 侧栏和页面各有一个 Refresh | 不变量 3 | 保留项目头/列表头图标，删另一个 |
| 同一个开关在两页两种高亮 | 不变量 4 | 走 `gorexToggleFace`；行选中走 `rowTint` |
| 新页面自己写 28×28 按钮 | 不变量 5 | `gorexIconButton(... gorexIconBtn|gorexPaneBtn ...)` |
| 页面进出时标题位置跳动 | 不变量 1 | 所有内页同字号同一行，不要按路由改标题栏高度 |
| 浮层里点条目又进下一层详情 / 返回键 | 交互行 | 点击直达面板并收起；其余动作进右键菜单 |
| 内容驱动高度的预算常数凭估算填 | 打磨循环 4 | 用 `Tester.Find` 矩形测真实高度，写贴合测试（不越界、空白有上限），预算改常数而非猜 |
| 文案向读者解释自己 / 暴露内部名 | 文案规则 | 删句或改成动作 + 下一步 |
