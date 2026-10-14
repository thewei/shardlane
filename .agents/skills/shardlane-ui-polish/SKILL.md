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
| 5 | **同一组件同一尺寸**：图标按钮只有两档——工具栏 28/16（`gorexIconBtn/Glyph`）、密集 24/14（`gorexPaneBtn/Glyph`），全走 `gorexIconButton`（等分一行的标签条走同脸的 `gorexIconTab`；Git 工作台的 `gdIconButton`/`gdPulldown` 是它与 `gorexPulldown` 的薄壳，只多一个 `.Focusable()` Tab 停靠点）；文字按钮只有 regular（默认）与 `compactButton`/`compactPrimaryButton`（Micro 10），**业务代码不在 Button 上内联字号**；互斥视图一律 `ui.Segmented`（含 Git 下半区 Commit/Changes/File Tree）——段里要放图标/计数时用 `ui.SegmentedBase` + `ds_components.go` 的 `segmentedTrack/segmentFace`（Sessions Agent 标签、Git Split/Unified），脸只在那一处定义，选中芯片浅色=页面色（凸起）、深色=按下面，与官方一致（`segmented_face_test.go`）；"…" 文本按钮一律换图标按钮/`gorexPulldown`；空态 `emptyState`/`workspaceEmptyBanner`、加载 `loadingState`、提示 `inlineNotice`、分组 `settingsGroup/formRow`、内页列宽 `contentColumn(760 表单 / 1040 列表)` | 见左 | `copy_style_test.go`（空态标题）、`button_tier_test.go`（Button 无内联字号）、`TestGitIconButtonsWearTheSharedFace`、`TestGitCommitReviewViewsAreASegmentedControl` + 下方扫描 |

## 键盘模型（一类列表一套手感）

侧栏的树/列表与 Sessions 列表只有一个键盘实现——`list_nav.go` 的 `listNav`。新树/新列表**接入它，不要自己写 `Shortcut` 或焦点环**。

- **真焦点即光标**：当前行就是 MyGo 的键盘焦点，脸 = 选中同脸 `SidebarSelect`（统一画在 `sidebarTreeRow`/`sidebarSectionHeader` 里，经 `listNav.showsCursor`）；整棵树是一个 `FocusGroup(ui.Vertical)` = 一个 Tab 停靠点，Shift-Tab 回到记住的行。
- **光标不亮不等于没有**：进页时光标停在开场行但**不亮**（鼠标打开的列表和从前一个样，没有"看起来被选中其实没选"的行）；任一列表键用过它才亮，Tab 进入自带亮（MyGo `FocusVisible`），离开列表或重进页面复暗，点击不亮。会改东西的行键（Files 的 `a/A/r/d/y`）等光标亮了才响应——不对看不见的光标动手。
- **键位**（表驱动 `navBindings`）：`j/k`·`↓/↑` 移动且**不回绕**（方向键由行认领，绕开 MyGo 焦点组的回绕）；`h/←` 已展开则折叠、否则回父级；`l/→` 已折叠则展开、已展开则进首个子项、叶子则打开；`o` 打开；`gg`/`G`/`Home`/`End` 首尾；`Ctrl-d/u` 跳十行；Enter/Space 本就是行点击。树自己的动作键归各树（Files：`a/A` 新建文件/文件夹、`r` 重命名、`d` 删除、`y` 复制路径；Sessions：`d` 移入回收站，回收站里 `u` 放回、`d` 永久删除——都与右键菜单走同一份实现，永久删除照样先确认）。
- **接入**：容器用 `s.navList(c, &s.nav.<树>, autoFocus, box, rows)`；行/分组头 spec 带 `Nav: &s.nav.<树>`；选择与折叠仍由各树处理——构建器返回的 `clicked` 已兼收鼠标点击与键盘激活。进页聚焦：Files/Hosts 开（落在打开的文件/选中项，否则首行），Git 引用树关（窗口级 j/k 归 Diff）。
- **虚拟化 `ui.List`（Changes）不走这里**：`listNav` 要求每帧画出全部行。Sessions 不是虚拟化（`ui.Scroll` + 项目卡），**走它**——项目卡头注册为 `Section`、会话行自注册，行内预览当作该行的折叠（`l` 只开不关、`h` 收起再 `h` 回项目头、`o` 切换）；选择模式与回收站里行是叶子（`o/l` 勾选）；删除/放回让行离开前用 `stay` 把光标落到邻行（先下后上，不落项目头）。
- **对话框坑**：MyGo 只把焦点还给"仍然存在的打开者"，带 AutoFocus 输入框的对话框会让它失效；`navTrackDialogs` 在对话框开/关时记住并还原光标行——新对话框只要计入 `dialogOpen/confirmOpen/mergeDialogOpen`。
- **守卫**：`list_nav_test.go` 与 `history_nav_test.go`（Sessions）（移动与不回绕、gg/G/Ctrl-d、h/l/o、单 Tab 停靠点、进页聚焦含列表晚到、对话框后焦点、Files 行键、Hosts 选中跟随与机器行、光标脸的亮/暗四种时机），22 种变异检查全红。headless 里用 `Tester.Key` 发行键——`TypeKey` 会把文字投进刚打开的对话框，只用于输入框。
- **验证边界**：App 在后台时 MyGo 的 `Focused()` 需要窗口焦点，被动观察验证不了按键；键盘只靠 headless 测试，真机只做外观被动核对。

## 去 AI 味配方（本项目真实做过的，按维度）

| 维度 | AI 味症状 | 做法 |
|---|---|---|
| 画布层次 | 渐变、浮卡、粗边框、大阴影、紫橄榄配色 | 平面中性画布 + 整高 macOS vibrancy 侧栏（无材质回退不透明）；Apple 中性灰 + 系统蓝强调；深度只靠表面对比，**只有终端 pane 是卡片**，内页是平面全幅 `gorexContentCard`；Chat WebView 调色板对齐原生 |
| 字体 | 字号/字重满地飘（650/700/.Bold()、8~16 乱字号） | 只用 `Typography()`：Title 20 / Heading 16 / Section 13 / Body 12 / BodySmall 11 / Caption 10.5 / Micro 10；字重只用 400/500/600/700，**正文最多 semibold，bold 只给页标题和数字**；重点 = 深浅色 + 字号 + 字重的小步进（参考 Pulse）；窗格头标题 12 semibold + 路径 11 regular 弱化色 |
| 图标 | Lucide 粗描边、机器人头像、文本字符冒充图标（↻ ← ↑↓ −） | 全部细线 1.5（iOS 风格）；彩色圆角方块 `iconTile` 只标目的地/类别（设置导航、Agent 列表、Sessions 行），状态一律 `StatusTone`；刷新/返回/展开用 SVG |
| 布局 | 互斥项并排按钮、整页拉伸、窄窗挤成单字列 | 互斥项 → Segmented tabs（Git 侧栏 Local Changes/All Commits、Git 下半区 Commit/Changes/File Tree、Theme/Density、Sessions Agent 标签条；只有一个来源不显示）；设置页 = macOS 分组列表（原生 Switch、右对齐紧凑控件、行内分隔线）；内页居中最大宽度，宽窗长边距；窄窗 `Wrap().Basis()` 让操作换行到消息下方；侧栏统一对齐网格 + 折叠结构线（`TreeLine`） |
| 间距 / 圆角 / 分隔线 | 5/7/12/16 混搭圆角、一次性间距、各页各画分隔 | Spacing `2/4/6/8/12/20`（XXS…XL）；Radius Control 6 / Row 8 / Card 10 / Pill，图标按钮 `size/2.6`；控件高 Compact 24 / Regular 28 / Row 31；分隔线一律 1px `BorderSubtle`，分组列表里从 `rowInset`（14）起、`attached` 行不画；卡 = `Panel` 底 + 1px `BorderSubtle`，无阴影；页面不写字面量 |
| 交互 | 每行常驻三个按钮、"…" 文本按钮、各页高亮色不同、编辑要另开一张卡 | **编辑就地发生**（值变输入框，主按钮换成 Cancel/Save，Return 保存，不另开卡）；行内操作 = 悬停才现的菜单入口 + 右键菜单，**占位固定，悬停不位移**（有变异测试）；原生控件优先（Switch/Segmented/Pulldown/SearchField）；破坏性确认默认焦点不在 Confirm；**浮层只有一层**——点击条目 = 直达面板并收起浮层，不在浮层里再开详情层/返回键；主动作只有点击，其余进右键菜单，行尾至多一个快捷图标；浮层复用侧栏同一张卡片 |
| 动效 | 装饰性动画、卡顿的重排 | 悬停/按下/开态颜色 120 ms；重排/拖动占位 150–190 ms；浮层进出场 140 ms（Enter Y+6 / Exit Y−4）；侧栏滑动 200 ms（拖动时 1 ms 直落）；Reduce Motion 直接落位；不为装饰加动画 |
| 文案 | 自圆其说、泄漏内部信息 | 见下节 |

## 文案规则

- **说人话**：不出现 operation id / slug / 协议版本 / 状态枚举 / RPC 原文；错误 = 发生了什么 + 下一步（`Couldn't load sessions` + `Try Refresh.`）。
- **不自圆其说**：不写向读者解释设计意图的句子（"One conversation surface over…"、"Native Terminal" toast）；空态 = 一句标题 + 可选一句下一步。标题不带句号，说明句带句号。
- **大小写**：按钮/菜单项/对话框标题 Title Case（冠词与 ≤4 字母介词小写：`Merge into Current…`、`Draft Subject with Pi`；Git 复合词沿用既有写法 `Cherry-pick`）；**空态标题与 tooltip sentence case**。守卫：`TestMenuItemsAreTitleCase`（扫 `.Item("…")` 字面量）、`TestButtonLabelsAreTitleCase`（扫 `ui.Button/PrimaryButton/compactButton/compactPrimaryButton` 的字面量，第二轮审计抓出 11 个 sentence case 漏网：`Select all`/`Clear selection`/`Retry connection`/`Copy hash`/`Clear filters`/`Earlier messages`/`Review prompt`/`Paste suggestion…`/`Hide draft`/`Clear diff search`/`Clear file filter`，另有 `Review conflict: <path>`）、`TestEmptyStateTitlesAreSentenceCase`（专有名词 Git/SSH 除外）。**边界**：Chat WebView（`frontend/src/App.tsx`）是独立表面，自成 sentence case 的声口（`Earlier messages`/`Send answer`/`Chat about this`），不在这条规则内；改它是 WebView 整体的声口决定，不要单点改。
- **确认框标题是问句**（2026-10-14 第二轮审计）：全仓其余 19 个字面确认标题都是 `Close Tab?` / `Delete “x”?` / `Discard Changes in 3 files?`，唯独自己加的回收站两个写成 `Delete Forever` / `Empty Trash`——后者还和触发它的工具栏按钮 `Empty Trash` 重名，对话框标题读起来像又一个按钮。已改成 `…?`；守卫 `TestConfirmTitlesAreQuestions`（AST 扫 `openConfirm` 的标题实参：字面量、`+` 链末尾必须是字面量 `?`、Sprintf 格式；变量标题不可判定则跳过）。
- **不写填充词**（2026-10-14 第二轮审计）：`Please` / `successfully` / `Oops` 不出现——说发生了什么 + 下一步。全仓只有一处漏网：同一事件（对话框打开后仓库变了）三种写法，其中 `Repository changed. Please reopen the Git action before confirming.` 是唯一带 Please 的，已并到 `Repository changed. Reopen the dialog to continue.`；守卫 `TestCopyHasNoFillerWords`（AST 扫全部生产字符串字面量，4600+ 个）。
- **同动作一个名字**：在 Finder 里定位统一 `Reveal in Finder`（Files 树/标题栏 Actions、Git worktree、Settings 日志、Sessions 的 `Reveal Source File in Finder` 只在宾语上多一个限定）；复制路径统一 `Copy Path`；标记已读统一 `Mark Reviewed`（图标 tooltip 与右键菜单项曾各写一种）；编辑器统一 `Open in Editor`（标题栏 Actions 曾写 `Open Editor`，文件卡片头 tooltip 曾写 `Open file in editor`，且已删除文件要同判置灰）；设置一律叫 `Settings`（`Git Preferences…` 是唯一例外，已改 `Git Settings…`）；菜单项写"动词 + 去处"（`Open Inspector`，不是 `Details`）。机械扫描逮不到"词不同"的同动作，换一类新菜单项就要人工对一遍已有叫法。守卫 `TestSameActionIsSpelledOnce`（扫菜单项/图标按钮/Tooltip 的动作名字面量，大小写不同＝两种拼写；尾部 `…` 不算拼写：菜单项开对话框带省略号、图标按钮不带）。 同一事实同一写法（第二轮审计）：提交时间曾有三种（元信息行 `Jan 2, 2006 15:04`、行 tooltip ISO、详情 RFC1123），现在都走 `gdCommitTime`，`TestCommitTimeIsSpelledOnce` 拦下调用处自己 `.Time.Format(…)`。数量同理：文件数只经 `pluralFiles`（"1 file"/"N files"），行数经 `gdLines`，其余名词经 `pluralS`；`"%d files"`、`itoa(n) + " files"` 把数字和复数名词焊死，最常见的"只改了一个文件"就读成 "1 files"（编辑区、列表头、文件夹 Stage 提示都曾如此），`file(s)` 是回避而不是句子——`TestFileCountIsPluralisedByTheHelper` 扫字符串字面量，`TestChangesCountAgreesWithItsNumber` 渲染一个文件的状态。
- **HIG**：打开对话框/需要再输入的项加 "…"；已换成图标按钮/下拉的不再用 "…" 文本按钮；复数用 `pluralS`；名称引号用弯引号。
- **可恢复**：任何截断文本必有 Tooltip；计数先于名称存活。

## 改共享组件的流程（单一真相源）

1. **先扫再改**：改 token/共享组件前跑下面的扫描，记下命中数与文件。
2. **改一处**：在 `design_system.go` / `gorex_style.go` / `ds_components.go` 改，不在业务组件 inline 覆盖。
3. **同步清理**：清掉业务侧同类 inline override；孤儿 helper（如 2026-10-14 删掉的旧 `iconButton`）随手删并更新 README/L2。
4. **全项目扫同类**，再钉测试。

```sh
cd internal/nativeui
rg -n 'ui\.(Primary)?Button\([^)]*\)\.FontSize' --glob '!*_test.go' .  # Button 内联字号（只许 ds_components.go 的两档；TestButtonSizeComesFromTheTiers 守卫）
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
4. **钉**：每个修复一条回归测试 + **变异检查**（`cp` 备份 → 改回旧实现 → 测试必须红 → 从备份还原并复核）。像素断言用 `pixelAt`/`rgbDiff`（`sidebar_tree_test.go`），布局断言用 `Tester.Find` 矩形（按钮文字的矩形是字，不是按钮；比对齐线而不是高度）；路由动画用真实时钟，测试用 `settleFrames`。**`Tester.HasText` 是子串匹配**：页面会打印路径，`t.TempDir()` 路径带测试名（`…NoConnect` 的测试就"出现"了 Connect）——否定断言一律用精确 `Find`（`onScreen`），种子目录用 `os.MkdirTemp`；`applyOnUI` 在 headless 下默认丢弃，需要落地就设 `uiApplyOverride` 队列。**顺序/可见性断言以渲染为准，不以注册表为准**：⌘P 面板一个区按字母序列出，`buildCommandCenterActions()` 的顺序不是界面顺序（曾据注册表钉过一条"面板与 Go 菜单同序"的测试，渲染一张图才发现与界面不符）——写顺序断言前先渲一张图看。
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
- 并排的同类图标按钮要同一种语法（第二轮审计）：Files 树工具条曾是裸 `+`（新建文件）配文件夹轮廓（新建文件夹）——动作符号配对象符号；改成成对的 `iconFilePlus`/`iconFolderPlus`（在 `iconFile`/`iconFolder` 路径上加 +，`icons.go`）。新增图标沿用 24 栅格 + `iconStroke` 一个描边，不另起一套。
- 图标按钮尺寸只来自三档（第二轮审计）：工具栏 `gorexIconBtn/Glyph` 28/16、密集 `gorexPaneBtn/Glyph` 24/14（卡头、查找条、胶囊 Dismiss）、悬停行内 `gorexRowBtn/Glyph` 20/12（树行、列表行、Changes 行、Sessions 行出现的那一枚操作）。禁止两种写法：构造后链尾 `.Size(…)`（Dismiss 曾是 22）和构造处的数字字面量（`20, 12` 曾在四处各写一遍）。行内悬停操作原有 18/20/22 三种尺寸、4/5/7.7 三种圆角、三套悬停透明度，现在是同一个 `gorexIconButton(…, gorexRowBtn, gorexRowGlyph)`；`TestIconButtonSizeComesFromTheTiers` 扫描全包，Changes/Sessions 的行测试各钉一次尺寸。
- 圆角只来自 token（第二轮审计）：控件 `Radius().Control`、行/卡片 `Radius().Row/Card`、胶囊 `Radius().Pill`（`Radius(14)`/`Radius(16)` 是 Pill 的手写近似）；侧栏里所有可选中的行（树行、Changes 行、提交列表）共用 `treeRowRadius`（提交列表曾是 5，其余 4）。字面量圆角只剩三类合理用法：圆点/圆形（`size/2`）、小标签（ref 徽标 3）、终端拖放提示；新增字面量先问是不是这三类。
- 对话框标题只有一个（第二轮审计）：自建对话框（Merge、Commit、文本输入）的标题用 `modalTitle`（`Heading` 16 semibold），`Title` 20 只属于标题栏的页标题——Merge/Commit 曾直接拿页标题字号；对话框内层间距取 `Spacing().L`（与 `ui.Modal` 面板默认的 12 一致），页脚按钮间距取 `Spacing().M`（与官方 `AlertDialog` 的 8 一致）；标题 Title Case（`Commit Changes`）。守卫：`modal_title_test.go`。
- 单行输入框的高度只来自 `Controls()` 档（第二轮审计）：同一个 Commit 主题输入框在行内编辑区是 28、在 Commit 弹窗里是 30，旁边的描述输入框又是 26——三个数字都是手敲的。现在显式高度一律 `Controls().RegularHeight`，`TestFieldHeightComesFromTheTiers` 拦数字字面量（多行 TextArea 按内容定高，不在此列）。
- 勾选框只有一张脸（第二轮审计）：Sessions 多选的标记曾是线描的 task/square 图标，提交文件列表里的官方 `ui.Checkbox` 是实心强调色盒子加白色对勾。标记本身不是控件（点击归行/表头按钮，官方控件会再添一个 Tab 停靠点、还会吞点击），所以 `selectionMark` 画官方的盒子和对勾，`TestSelectionMarkWearsTheCheckboxFace` 与官方逐像素比（深浅×选中/未选中，四个变异均 KILLED）。同一模式见分段控件：官方只给 Base，脸自己搭，就拿官方像素当真值。
- 一个角色一个控件（第二轮审计）：搜索/筛选列表的输入框只用官方 `ui.SearchField`（放大镜 + 边框 + 清除钮 + Esc 清空）。曾同时存在三种脸——官方的、Git 面手搓的无边框灰底版（连清除钮都自绘）、无放大镜的裸 `TextInput`——改后一律官方；`search_field_test.go` 用 AST 扫描拦下「`TextInput` 链上带 Search/Filter 提示」与「手绘 Clear 钮」。注意官方控件的占位字固定为 `Search`（`.Placeholder()` 对它无效），具体名字放 `.Label(…)`；先扫「同一角色有几种实现」再动手，别只看当前文件。
- 分隔线只用发丝线 token（第二轮审计）：`theme.Border` 是控件描边（输入框、Segmented 轨道），不透明、比 `BorderSubtle` 重数倍，不能拿来画分隔线或面板边缘——Git 工作台四处曾这样做，与侧栏边线并排一粗一细。`hairline_test.go` 拦下 `.Background(t.Border)` / `.BorderColor(t.Border)`。
- 窗格卡头：标签已写出目录或单卡时不再出 cwd（`paneCardCaption`/`titleSpellsOutDir`）。
- 开关脸统一：Files 忽略开关与 Sessions 的 Select/Trash 从蓝/accent 改为 `gorexToggleFace` 的中性芯片。
- 旧 `iconButton`（28/14，半径 6）并入 `gorexIconButton`：侧栏页脚设置齿轮走工具栏档，Workspace 区 "+" 走密集档。
- 空态标题统一 sentence case；Inspector 路由补回 `gorexContentCard`。
- Hosts 侧栏列表不再二次内缩（原比 Sessions/Settings 低 14、窄 9）：`TestInnerSidebarsShareOneInset`。
- Agent 浮层（标题栏/托盘）重排（2026-10-14 第二轮）：顶部 Agents/Services 分段、浮层自身无标题；Agents 按状态分组（Needs attention › Review › Working › Idle，空组省略；分组头 = 状态点 + 名称 + 计数，Services 分区标题用同一构件 `quickPanelGroupHeader`），行复用侧栏 `agentCardRow`；点击行**直达面板并收起浮层**（待交互 → Chat；blocked/failed → Terminal；其余回到该 pane 上次所在面板），行尾 ✓ = 标记已审阅（不跳转），其余动作在右键菜单；去掉详情层、统计筛选卡与浮层标题。高度随内容增长至上限 480：预算常数取自真实布局（`quickPanelChromeHeight/GroupHeight/RowHeight/UsageHeight`），不是估的——估的 72/28/44 实测多出 57 px 空白，`TestQuickPanelHeightHoldsTheContent` 现在钉贴合（含用量行）。
- Hosts 重做（2026-10-14 第二轮）：**侧栏**按配置自带分组可折叠——`#SCE_GROUP:<id>:::<名>` / `#SCEGroup <id>`（SSH Config Editor 的注释标记，只读解析 `sshconfig.Index.Groups`，不写回、不建第二注册表）→ 声明组 › Ungrouped › 各 Include 文件，没有可分组内容时保持平铺；分组头与行都走共享的 `sidebarSectionHeader`/`sidebarTreeRow`（Git 分区头已委托同一构件，`Selected` 用 `SidebarSelect` 脸，`Action` = 悬停才现的行内主动作图标，占位固定）。**详情**不再是"标题 + 三个按钮 + 三张卡"：一行标题（Heading 16 semibold + 灰色 `file:line`，非主机则前缀 `Wildcard pattern ·`/`Match rule ·` 交代为何没有 Connect）+ 主按钮 Connect + 铅笔图标；指令只有一个 `settingsGroup`，**编辑就地发生**（值变输入框、Cancel/Save 顶替 Connect/铅笔、Return 保存、一句话说明其余内容原样保留），生效配置是折叠行（`ssh -G`）。长值走 `valueText`（单行省略 + 全文 tooltip，所有事实行共用）。次要动作（Add Herdr Machine…/Edit File）进标题栏 Actions，与行右键菜单是同一份 `hostEntryMenu`。守卫：`hosts_test.go`（分组折叠与计数、平铺、符号链接配置不被当 include、选中脸、悬停连接、就地编辑与放弃确认、Return 保存落盘、Actions 随选中出现）、`hosts_groups_test.go`、`sshconfig` 的标记解析测试。
- 键盘（2026-10-14 第二轮）：Files/Git 引用/Hosts 侧栏与 Sessions 列表共用 `listNav`（见「键盘模型」）；光标脸与选中同为 `SidebarSelect`，Hosts 光标移动即选中，Files 光标行上 `a/A/r/d/y`、Sessions 光标行上 `d/u` 直通右键菜单同一批动作。
- Sessions 删除（2026-10-14 第二轮）：**Hide 没有意义**——Hide/Restore 与 Hidden 开关退役，删除 = 移入应用内回收站（`internal/sessiontrash`，Provider 文件只往返于自己的目录；清单不含会话内容，回收站行按需从被搬走的文件读回）。**可撤销的动作不弹确认**：移入回收站只给带 `Undo` 的提示条（`pendingToastAction`）；**不可撤销的才确认**：`Delete Forever…`/`Empty Trash`（共享确认框）。Trash 开关只在回收站有内容（或视图已打开）时出现，与"无可操作的控件不显示"同一规则；回收站视图复用同一分组列表，行键是条目 id、选择常开、有勾选才出批量条。保护：运行中 Agent 的会话身份命中或 60 秒内刚写过的会话不搬，并说清原因。旧 `hidden_sessions` 设置键读时忽略、下次保存丢弃（被隐藏过的会话重新出现）。
- 菜单项一律 Title Case（Git 分支/标签/Stash/提交/文件夹菜单 20 项）、Finder 定位统一 `Reveal in Finder`、Git 确认框标题对齐菜单（`Undo Last Commit?`、`Delete Tag “x”?`）：`TestMenuItemsAreTitleCase`。

- 第一轮遗留四项收尾（2026-10-14 第二轮）：①遗留 `/chat` 整页并入 `gorexContentCard`、标题进标题栏（`routeHeader` 增 `/chat`），自画页头 `pageHeader` 随之删除。②Git 下半区 Commit/Changes/File Tree 换成原生 `ui.Segmented`（条高 36，与上方 All Commits 头同高）；`TestGitCommitReviewViewsAreASegmentedControl` 在同一棵树里放一个普通 Segmented，比对选中脸像素。③Commit 两个主按钮不再内联字号：快捷提交条走 `compactPrimaryButton`（Git 密集条、与同排 Cancel 同档），Commit 对话框页脚走默认档（见待办里的取舍）；`TestCommitPrimaryButtonsTakeTheirSizeFromASharedTier` 钉同排同高，`TestButtonSizeComesFromTheTiers` 扫全包。④Git 工作台 `gdIconButton`/`gdPulldown` 与 Commit options 下拉并入 `gorexIconButton`/`gorexPulldown` 同脸（半径 size/2.6、`k.hover/k.pressed`、悬停字形变深、120 ms）。**键盘语义先评估再换壳**：共享 Box 版按钮不是 Tab 停靠点、点击也不抢焦点，而 Git 评审靠键盘（文件头 Copy 按钮用 `FocusVisible` 显现），直接换会让整条工具栏脱离键盘——所以 `gdIconButton` 在共享脸上多一个 `.Focusable()`（带 Menu 的下拉本就可聚焦）。`TestGitIconButtonsWearTheSharedFace` 用悬停像素（边缘填色 + 角落留白）对照同尺寸共享按钮，`TestGitToolbarIconButtonsStayTabStops` 钉 Tab 必达 Fetch/Pull/Push/Git actions/Copy path。
- 代码着色只剩一个引擎（2026-10-14 第二轮）：Git 评审的分词从 Chroma 换成 MyGo `ui/editor` 的分词器（`internal/codehl.Lines`），`go.mod` 少 `chroma/v2` + `regexp2/v2`、`go.sum` 少 5 个模块；Files 代码视图本来就用这套分词器，两处经同一张表 `codehl.KindClasses`（token 种类 → 评审调色板类）上色，`TestReviewAndFilesPaintACharacterTheSameColor` 逐字符比对（含跨行块注释状态）。**不用 `editor.DiffView` 替换 `gd_*` 的结论**：DiffView 要先为每个文件建 Buffer，`FitContent` 无法跨文件虚拟化，而评审"一行一行"的虚拟化才是大 diff 的契约（`TestGdPerfBaseline`：58 k 行首帧约 32 ms）；`HunkActions` 的 Revert/Keep 是缓冲区语义，不是 Git 暂存区；暂存栅栏、Viewed、大 diff 占位等约 3600 行都得重写。**代价**：103,960 个跟踪文件里 Chroma 着色的 88,466 个有 2,537 个（约 2.9%，多为 `.hbs/.po/.mo/.feature/.txt/.env/.dart/gradle`）MyGo 不着色，反向多着色 1,140 个；运算符/标点与 Files 一样是普通色，用户自定义类型名不再单独品红（`ClassName` 类随引擎一起删）。
- 桌面动作交还 MyGo（2026-10-14 第二轮）：复制走 `mygo.Clipboard`，Finder 定位走 `mygo.Shell.ShowItemInFolder`，打开网页走 `mygo.Shell.OpenExternal`，`internal/platform` 只剩 `OpenFile`（`open -t` 要的是「文本编辑器」，框架的 `OpenPath` 是「该类型的默认应用」，不等价）。**真缺陷**：原来 26 处复制都 `exec pbcopy`，而 Finder/Dock 启动的 App 只有 `__CF_USER_TEXT_ENCODING=…:0x0`（MacRoman）、没有 `LANG`，pbcopy 把 `/Users/me/项目/测试.txt` 写成 `/Users/me/È°πÁõÆ/ÊµãËØï.txt`——中文路径、提交说明、会话恢复命令一律乱码（草稿 pasteboard `-pboard ruler` 复现）。守卫：`TestNoClipboardToolIsSpawned` 扫全部生产代码；`TestMain` 把桌面调用默认置空（框架调用要等主循环，测试里会挂住，也不该碰开发者的剪贴板）。实机验证（`cua-driver` 后台）：Git diff 里中文行（含 `—`）右键 Copy Line，剪贴板读回逐字节一致；验证前后备份/还原了用户剪贴板。
- 启动器同名同目的地（2026-10-14 第二轮审计，实机读 AX 菜单栏发现）：侧栏四个视图标签搬家后，**Changes 在 ⌘P 和 Go 菜单里根本没有入口**（只有侧栏图标能到）；Go 菜单的 Files 还是旧"右栏开关"的接缝（`ToggleRightPanelFromMenu`，选一次进、再选一次出，与同菜单的 Sessions/Hosts 不一致）；Settings 摆在自定义 Go 菜单里而不是应用菜单；自定义应用菜单丢了系统标准的 Hide/Hide Others/Show All（⌘H 无效）。现在：菜单栏搬进 `app_menu.go`（`Shell.MenuBar`，main.go 只挂上），应用菜单是系统惯例（About、Check for Updates…、**Settings… ⌘,**、Hide ⌘H/Hide Others/Show All、Quit），Go 菜单按侧栏标签顺序列 Workspace/Changes/Sessions/Files/Hosts；⌘P 补 `Changes` 与 `Workspace`（面板原先没有不靠选 Pane 的回 Terminal 路径；面板一个区内按字母序列出——空查询截图为证——所以面板只钉名字，顺序只钉 Go 菜单与侧栏标签）；Files/Changes 在菜单与面板里走「去往」形态 `goFiles/goChanges`——**点名已打开的页面保持不动，只有侧栏标签才是切换**。同轮实机再抓一个同类缺陷：**Go ▸ Workspace（⌘1）在 Changes/Chat 表面上是死的**——它只是 `router.Push(/workspace)`，而 Changes 与 Chat 本就是 `/workspace` 的表面，同路由的 Push 被去重，按了没有任何反应（实机 `invoke_menu` 后台复现：Changes 上选 Workspace，截图仍是 Changes）。现在走 `ShowWorkspaceFromMenu` = `showViewTerminal`：与 Back 同落点、幂等。守卫：`TestMenuBarFollowsTheMacConventions`、`TestLaunchersAgreeOnViewDestinations`（表驱动，侧栏标签数变了会逼你补）、`TestNamingTheCurrentViewKeepsIt`、`TestWorkspaceLandsTheTerminalFromAnySurface`（面板与菜单各从 Changes/Chat/Files 等表面按两遍）。**没有加 ⌘2–⌘5**：快捷键分配是产品决定，见下方待办。
- 窗口标题随**状态**而非随**跳转**（2026-10-14 第二轮审计，实机发现）：`restoreSession` 在窗口挂上之前就把可见路由设成了上次的内页，"路由变化时同步标题"的钩子永远不触发——恢复到 `/files` 的启动窗口一直叫 `Shardlane`。现在 `syncWindowTitle` 每帧由 `syncRouteVisibility` 调用，经 `windowTitleShown` 去重只在变化时碰窗口，窗口句柄收成 `setWindowTitle` 接缝（测试换成记录器）：`TestRestoredRouteTitlesTheNativeWindow`。

## 已知例外与待办（下一轮从这里接）

- **终端窗格卡头保留**（N19 #7：Terminal⇄Chat 开关在卡头；去掉会改 PTY 尺寸）；只做信息去重，不做结构搬家。
- 仍在自成一脸的小控件（同类不同脸，下一轮评估）：`gdPalette.hover/pill`（行脸）。`gdChangedPill` 的 Dismiss 已改为密集档 24/14，Changes 行与 Sessions 行的悬停操作已并入行内档 20/12（见上：图标按钮三档）。`gdLayoutControl` 与 `gdSearchInput` 已在第二轮审计并入共享脸（见上：分段控件一个 helper、搜索框一个官方控件）。
- Commit 对话框页脚的主按钮取**默认档**而非 `compactPrimaryButton`：同排 Cancel Commit 与丢弃草稿提示（Keep Editing/Discard Draft）都是默认档，只降主按钮会比兄弟矮，整排降档则对话框里 28 高的输入框旁出现 22 高的主动作。要降档需整排一起。
- `Refresh Runtime` 菜单项保留（命令级入口，不是页面按钮）。
- **待产品决定：命名词表**（F196–F198，影响面大，不单点改）：`Remote Machine` vs `Herdr Machine`；`Session` 既指 Herdr 运行时会话（切换器）又指 Agent 对话历史（Sessions 页）；侧栏条目叫 `Workspace` 还是 `Project`（分区头/创建入口 6 处 vs 菜单/对话框/⌘P/切换器/Files 约 20 处）。
- **待产品决定：没有菜单孪生的图标按钮 tooltip 大小写**（F192 ②）。现状两派：与菜单项同源的命令名是 Title Case（`Split Right`/`Close Pane`/`Show Sidebar`/`New File…`，`header_overflow.go` 的菜单项复用同一串）；Git 工具条与直写 `.Tooltip(…)` 是 sentence case（`Show file list`/`Commit options`/`Pane menu`）。同一动作的两种拼写已由 `TestSameActionIsSpelledOnce` 收口；剩下的是整类约定，在决定前不要单点改。
- **待产品决定：⌘2–⌘5 作 Changes/Sessions/Files/Hosts 的加速器**。侧栏收起时四个标签随之消失，今天的键盘路径只有 ⌘P；加速器只能挂在「去往」形态（`goChanges`/`goFiles` 与幂等的路由跳转）上，不能挂在侧栏的切换形态上（F13/F101/F103/F162 的历史教训）。另：没有 View 菜单（显示/隐藏侧栏、全屏只在应用内 ⌘B）、Window 菜单缺 Bring All to Front——都是显式没做，等决定。

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
| 新树/列表自己写 j/k、`Shortcut` 或焦点环 | 键盘模型 | 行走 `sidebarTreeRow`/`sidebarSectionHeader` + `Nav`，容器走 `navList` |
| 菜单/快捷键只是 `router.Push(路由)`，而目标是同一路由的另一个表面 | 不变量 2 | 同路由 Push 被去重＝按了没反应；落点要走与 Back 相同的 `showViewTerminal`/`showSurface` 这类显式落地函数，测试从每个表面各按两遍 |
| 新页面入口只加在一处（侧栏图标），⌘P 和菜单栏漏了 | 不变量 2 | 页面入口三处同名（侧栏标签 / ⌘P / Go 菜单；Go 菜单与标签同序，面板按字母序）；菜单项说"去往"（幂等），只有侧栏标签是切换；改动时跑 `TestLaunchersAgreeOnViewDestinations` |
| 镜像到系统的状态（窗口标题等）只挂在"变化"事件上 | 打磨循环 4 | 启动恢复直接写初始状态、没有变化事件；每帧幂等推导 + 去重，测试里走"恢复后首帧" |
| 文案向读者解释自己 / 暴露内部名 | 文案规则 | 删句或改成动作 + 下一步 |
| 把条目路径和"主文件路径"直接比较来判断是否 include | 打磨循环 4 | 索引会解析符号链接——软链的 `~/.ssh/config` 会让每个条目都像 include；比较用索引自己的主文件（`hostsMainFile`），并钉符号链接用例 |
| 复制/打开/定位自己 `exec` `pbcopy`/`open` | 所有权梯子（MyGo 优先） | `mygo.Clipboard` / `mygo.Shell`；子进程没有 UTF-8 locale，中文会乱码；`TestNoClipboardToolIsSpawned` 守着 |
