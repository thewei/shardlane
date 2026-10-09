# Shardlane MyGo 统一界面与交互优化方案

**日期**：2026-10-09  
**状态**：分阶段实施，UI-001～UI-005 已完成 headless；UI-006 搜索/筛选已实现；UI-007～UI-009 的核心 UI/导航链路已完成 headless，UI-010 的右栏宽度策略已实现；排序控件、Commit Dirty Guard、完整焦点/实机验收仍待完成  
**基线**：Go / MyGo Native UI，Herdr 为唯一终端与 Agent 运行时  
**架构权威**：[client-product-architecture.md](client-product-architecture.md)；本文仅规定交互细节与实施顺序，不重定义运行时归属。  
**输入**：用户提供的 6 张现状截图；`internal/nativeui` 当前真实代码；MyGo 0.10 Closure Audit；2026-10-06 UI 发现台账。

## 0. 产品结论

**Terminal First · Context Aware · One Shell**

Shardlane 的身份是「以 Terminal 为中心的 Agent 开发工作区」，不是拼合起来的多个管理后台。日常工作只需要识别三层：

1. **Where（左）**：导航、实例 / Project / Tab / Pane / Agent / 历史过滤，回答「我在哪里」。
2. **Do（中）**：唯一主工作表面，执行 Terminal / Chat / Diff Review / Commit / History 阅读 / Settings 操作，回答「我正在做什么」。
3. **About（右）**：与当前页面和对象严格匹配的工具、摘要和动作，回答「关于当前对象还能做什么」。

不要为了页面统一，把所有页面硬塞成相同三栏。**统一的是应用外壳、设计语言和切换规则，不是每个页面必须强制出现三栏。**

## 1. 现状审计和证据

| ID | 优先级 | 现象及证据 | 根因 / 影响 | 处置 |
|---|---|---|---|---|
| UX-01 | P0 | History + 右栏仍显示 Workspace 的 Services/端口，截图 6；`shell.go` 持续挂载 `rightPanelSlide`，原 `right_panel.go` 内容固定 Changes/Files/Services | 缺少按路由的 Inspector 归属 | **首批实现**：路由决定右栏内容；History 使用自身元数据 Inspector |
| UX-02 | P0 | History 和 Workspace 分享一个开关状态；当 History 打开面板，回 Terminal 会意外打开 Workspace 工具 | 可见性误共用 | **首批实现**：Workspace 与 History 各自记住展开状态，Settings 不继承 |
| UX-03 | P0 | History 行标题、描述、项目可重复（截图 5/6） | 标题和摘要同源，先前回退又显示 Project | **首批实现**：不重复摘要；截断文本有完整 Tooltip |
| UX-04 | P0 | 左侧点击 Agent 原本直接跳转 Workspace，用户无法快速查看 Agent 事实或选择 Chat/Terminal | Agent 是对象而非默认导航页 | **首批实现**：桌面端改为该 Agent 的详情浮层；无浮层能力时保留安全降级 |
| UX-05 | P1 | Status/Tray 浮层仅有标题、过滤器、列表，信息粗糙（`quick_panel.go`） | 统计不可操作、缺少精细的层级 | **首批实现**：统一详情窗口、统计筛选卡；更多视觉打磨仍待实机 |
| UX-06 | P1 | 普通 Shell 的 Agent Chat 是“无 Agent”大空白（截图 3） | Chat 入口缺当前 Pane 的 Agent 身份门禁；失去 Agent 时可能遗留旧 transcript | **已实现（headless）**：无 Agent 不切换路由，提示去左栏选择；Agent 消失时隐藏旧 transcript/composer；等待会话有紧凑提示和 Terminal 出口 |
| UX-07 | P1 | Git clean 状态占用中央大片空间，CTA 与空状态孤立（截图 4） | 独立居中大卡侵入 Terminal 主工作区视觉层级 | **已实现（headless）**：靠近顶部的轻量空状态条，View Commits / Open Terminal 可实际操作 |
| UX-08 | P1 | Settings 页面与 History、Git 的头部高度、卡片、字级不一致（截图 2） | 五个设置页分别拥有标题与填充；Terminal 甚至重复渲染同一设置 | **已实现（headless）**：单一 settingsPage Header / Scroll / 内容宽度；五个 section 不再各自定义标题；删除重复 Transparent background 控件 |
| UX-09 | P1 | History 过滤栏缺结果数、搜索反馈，Provider 筛选原先发生在最近 100 条结果之后（截图 5/6；领域查询代码） | 过滤 UI 与有界查询没有完整闭环 | **已实现（headless）**：原生 SearchField 即时筛选、结果数/限额提示、空结果一键清除、Provider/Project 过滤先于 SQL LIMIT、完整 Provider 选项查询；可配置排序与实机焦点仍待做 |
| UX-10 | P1 | 一级导航和工作区 Surface Switch 叠加，Settings/History 上的顶栏动作仍可能“再次点击 = 返回 Terminal” | 路由与局部 surface 切换语义混用；History 子页直接跳 Terminal、Sidebar Back 可能恢复到 Diff | **已修复核心导航（headless）**：子页先回 History 根，根页二次点击才切 Terminal；Sidebar Back 明确返回 Terminal；内页选中项不重复压入路由 |
| UX-11 | P2 | 右面板 240–500 DIP、左栏 252 DIP；窄窗时主 Terminal 可用宽度不足 | 缺响应式压缩优先级 | **已实现基础策略（headless）**：中央保留至少 560 DIP；空间不足右栏自动隐藏但保留用户 open/tool 状态；主动开栏时可收起左栏或提示宽度不足；实机过渡动画/跨屏未验 |
| UX-12 | P2 | 快捷操作的 hover、focus、disabled、错误消息、装载状态跨页面不完全一致 | 一次性组件/文案散落 | 待完成通用组件审计与可访问性矩阵 |
| UX-13 | P0 | Git 左侧文件过滤无匹配时发生 nil-pointer 崩溃（UI-007 恢复路径测试复现） | `gitworkbench.FilterTree` 合法返回 nil，但 `Flatten` 无空根节点防护；被 `sidebar.sectionRows` 直接调用 | **已修复（domain + headless）**：`Flatten(nil)` 返回空行；Diff 空结果展示 Clear file filter；分别由 `tree_empty_test.go` 与 `unified_page_test.go` 验证 |
| UX-14 | P1 已完成主要入口（headless），实机/全入口待验 | Commit 编辑期间切页面可能丢草稿 | `WorkspacePrimarySurface` 与 Router 分开 | **Git 第五轮**：Commit 变成 Modal；取消时 Keep Editing / Discard Draft；in-flight 禁退出；顶部视图/主要快捷键离开守卫。原生窗口和其它间接导航入口仍需实机验收 |

**证据等级**：UX-01 至 UX-05 已有源码与 headless 测试；其余截图现象可信，但根因或完整影响仍需在隔离的真实客户端逐项复测，不以截图代替运行时证据。

## 2. 页面地图：互斥与并存

| 场景 | 路由 / 主表面 | 左侧区域 | 中心区（唯一主内容） | 右侧区域 | 弹层 |
|---|---|---|---|---|---|
| 日常开发 | `/workspace` Terminal | 当前实例的 Project/Tab/Pane/Agent 树 | **原生 Terminal**（Herdr Pane） | Changes / Files / Services 工具；可关 | Agent / Command / 右键菜单 |
| Agent 对话 | `/workspace` Chat | 仍显示当前实例树和 Agent | 所选真实 Agent 会话；无数据明确说明 | 第一阶段仍沿用 Workspace 工具，后续评估与 Agent Inspector 互斥切换 | Agent Quick View |
| Git 变更 | `/workspace` Diff | Git 文件 / 导航 | **Diff Review**，不能与 Terminal 同时吃输入 | Changes / Files / Services（仅导航和辅助） | 分支/差异动作 |
| 提交 | `/workspace` Commit | 项目与 Git 状态 | **Commit**，事务独占主工作面 | Changes 导航与暂存事实 | 明确确认/错误 |
| 历史列表 | `/history` | History 类型、项目、搜索过滤 | 会话列表 | **History Context**（统计与使用提示） | 必要的非破坏性快捷动作 |
| 历史详情 | `/history/{id}` | History 导航与筛选 | **只读会话内容** | **精确 id 对应的元数据**、来源和 Copy Path | 后续可接官方 Continue 流程 |
| 项目归档视图 | `/history-projects` | History 导航 | Project 分组的历史列表 | **History Context**（总量与引导） | 同上 |
| Agent Inspector | `/inspector/{pane}` | 对象导航 | 详细 Agent 状态和会话 | 默认无全局工具栏 | Agent Quick View |
| Settings | `/settings/*` | 设置分节导航 | 设置 Form、帮助和诊断 | **无右栏**；占满可用中心区 | 确认/系统原生对话框 |
| 无效路径 | Not Found | 保持应用导航 | 解释和返回 Workspace | **无右栏** | 无 |

**互斥规则**：

- `ui.Router` 唯一拥有顶级页面；`WorkspacePrimarySurface` 唯一拥有 Workspace 的 Terminal / Chat / Diff / Commit。不得引入第二套路由器或第二套 Terminal runtime。
- 同一个 Workspace 中心一次只有一个 Surface；隐藏的 Terminal 不接收键鼠/文件拖放，仍遵循可见性与 Herdr Attach 规则。
- 右侧栏属于**当前页面的上下文**，不是全局固定“Changes/Files/Services”。右侧展开/收起绝不修改中心 Surface。
- 页面切换保留当前 Workspace 工具选项、展开状态；History 另记其开关；Settings/Inspector/无效路径不显示旧上下文，不清除其保存状态。
- 项目、Tab、Pane 改变后，只对受影响的 Workspace 工具重新解析上下文，History 不允许泄漏上一会话的摘要。
- Popover 是临时 overlay，既不是 Router 页面，也不是 Herdr 生命周期对象；最多显示一个同类快速面板。

## 3. History 完整交互设计

**左：导航和筛选**。All Conversations / By Project + Providers 动态来源 + 原生 SearchField。现已接入即时搜索、正在查询/结果数/限额提示、无匹配时 Clear filters；搜索、Provider 筛选和列表刷新仅在消费它们的 `/history` 列表页显示，分组与详情页不再出现无效控件。Provider 选项由无归档 Catalog 的 distinct Agent 返回，不再只从前 100 条生成。SQL 先应用 Provider/Project 条件再 LIMIT，页面结果按 Updated 降序。可配置排序、搜索防抖及实机输入法/焦点检查仍待完成，不重复项目名作为 preview。

**中：列表/详情的责任**。列表突出最有判别力的一行标题，第二行仅展示独立摘要；底行显示 Project、Updated、Message Count。按 Enter 进入详情；详情 Header 显示完整标题（Tooltip）+ 项目/Provider；Transcript 一直由中间区域所有。

**右：Inspector 严格跟随左 / 中的语境**：

- 列表态：当前筛选结果量、使用方法，提示选择会话；不能虚构所选会话。
- 详情态：优先使用 route `{id}` 对应的 Detail Meta；请求未完成时可用当前列表同 id 的轻量元数据；不同 id 严禁错显。
- 展示顺序：Title + Provider → Project → Messages → Created/Updated → 真实 Branch/Model（存在才显示）→ Source Path + Copy。
- 不展示其它 Project 的 Services，不解析整个 transcript 来拼一个摘要；无确切 continuation API 时不出现会造成误解的 Continue 按钮。
- 新历史被选中后右栏同步；Loading / Error / Deleted / No Matches 各有明确状态；旧请求使用现有 generation guard 丢弃。

## 4. Agent / 状态托盘弹层规范

**共享一个 MyGo 原生 Quick Panel**，拥有两个展示模式：

1. **Activity Overview**（标题栏 Agent、macOS Tray）：品牌小标 + “Agent Activity” + 状态摘要；Attention / Review / Working 三个可点击汇总，下面仍用统一的 All / Attention / Review / Working / Idle 筛选；每张 Agent 卡只包含品牌/状态、项目、简短事实。
2. **Agent Quick Detail**（单击左栏 Agent）：同一个弹层切成精确 Agent 模式，显示 Title、Provider、Operational Status、Project、Runtime、Unread / Review / Usage（有真实事实才显示），并提供 **Open Terminal / Open Chat / Inspect Agent**，有待复核时显示 Mark Reviewed。Back 回到全体列表。

相同 Agent 再点关闭，点击不同 Agent 就在同一窗口切换上下文；托盘/标题栏从概览进入，不能带着上一次的单 Agent 筛选泄漏。失去焦点/Escape 关闭；浮层内容仅来自现有 `StatusCenterSnapshot` / `AgentCardModel`，不在渲染期扫描磁盘或做 Herdr RPC。对于 Agent 已消失，展示“Agent unavailable”，且**绝不展示过期操作**。

**下一批验证点**：宽高变化时锚点翻转、左栏靠窗口底部的锚点、跨显示器、点击 Agent 后返回主窗的焦点、Review 成功反馈、Actions 动态能力门禁、VoiceOver 和键盘操作。

## 5. 统一视觉系统

目标不是改造成 Liquid Glass 或复制 Linear / VS Code 的外观，而是保留当前 **深色、低对比、精细层级、终端导向**的视觉方向，统一具体的尺寸与使用语义。只用 `theme.go`、`design_system.go`、`components.go`、`page_common.go` 的设计值，消除页面硬编码。

| Token 角色 | 建议值 / 约束 | 用途 |
|---|---|---|
| 字体层级 | Title 18 / Section 13 / Body 11.5 / Secondary 10.5 / Caption 10 DIP，跟随当前 Typography | 两个页面相同语义不能换另一套尺寸 |
| 空间步进 | 2 / 4 / 6 / 8 / 12 / 20 DIP | 行内、组内、区块、页边距 |
| 圆角 | Control 6 / Row 8 / Card 10 DIP | 禁止同类 Card 出现任意 5/7/12/16 混搭 |
| 深度 | 单一卡片底色 + 极轻边框；无大量独立 shadow 层 | 主 Terminal 始终是视觉焦点 |
| 色彩 | 蓝色表达可操作、绿色真实 working/success、橙红真实 attention/error | 不用颜色单独表达可访问性语义 |
| 最小交互尺寸 | 常规按钮 28 DIP，密集图标按钮不低于现有 Controls token | 不用缩到 9 DIP 的文本充当交互按钮 |
| 文本省略 | 标题/路径单行省略必须可悬停查看完整内容 | 信息不可被静默截断 |
| 状态反馈 | Loading / Empty / Error / Retry / Success 全覆盖 | 不允许死按钮和不可解释的空白 |
| 动效 | 复用现有 200 ms panel slide；Reduce Motion 直接落位 | 面板不会在路由改变时出现旧内容闪帧 |

**设置页**：与 History 共用 page header 间距；标题只保留一个；Form 统一 Label、Description、Control 对齐；页面过高时可滚动，宽窗使用舒适的内容宽度而非拉伸所有 Segmented 到最右边。

**空状态**：工作区 Git clean / 无 Agent Chat 不能复制整屏管理后台式空卡；内容应解释当前选择是什么、为何空、下一步动作，且保留一键回 Terminal。

## 6. 对照参考与取舍

- [Apple HIG: Panels](https://developer.apple.com/design/human-interface-guidelines/panels)：Inspector 随用户当前选择变更，细节信息不能成为静态、无关的面板。**采用**。
- [Apple HIG: Split Views](https://developer.apple.com/design/human-interface-guidelines/split-views)：导航选择与详情区域需有持续、清晰的对应反馈。**采用**。
- [Apple HIG: Popovers](https://developer.apple.com/design/human-interface-guidelines/popovers)：浮层只放少量相关操作、一次一个、在锚点附近、失焦退出。**采用**。
- [VS Code: Custom Layout](https://code.visualstudio.com/docs/configure/custom-layout)：主次侧栏职责明确、可收起、保留视图位置和最近选择。**采用思路，不复制 IDE 的多列窗口实现**。
- [VS Code: Sidebar Guidelines](https://code.visualstudio.com/api/ux-guidelines/sidebars)：相关视图成组，别把所有动作与 View 都塞进侧边。**采用**。
- Godiff 作为 Git Diff/Changes 的行为参考已经记录于 `reference-godiff-shardlane-0.10-audit.md`；不复制其内部实现。

## 7. 概念效果图比较与准确生图提示词

本轮在会话中探索了两张深色 macOS 概念图：

- **方案 A**：Terminal 最强主视觉，History / Agent 浮层功能分区更细，适合提取深度、层级、边框和密度。**不足**：个别标签、内容、列布局是生成的示意，不符合实际 MyGo/Herdr 对象树，不能直接照抄。
- **方案 B**：Terminal / History / Agent 的结构关系更清楚，更贴近“左导航，中工作区，右上下文”的目标。**不足**：概念图额外画了顶部 tab 导航和可能不存在的 History 分类；不得引入第二路由或第二导航。

**最终选择**：采用 B 的上下文关系、A 的密度控制，坚持当前 Shardlane 原生应用的实际控件和截图事实；图是审美参照，不是交互契约或源码真值。

可复用高精度 Prompt：

> Refine an existing macOS-native developer application named Shardlane (Go + MyGo Native UI). Preserve the existing dark plum-to-charcoal color family, macOS traffic lights, subtle card borders, vertical left sidebar, and hierarchy Project → Tab → Pane. Design a terminal-first application, not a browser dashboard. Show three coordinated states in a single annotated design board: (1) Workspace with two stacked terminal panes as the visual anchor and a right contextual Changes/Files/Services inspector; (2) History with a left filter rail, a central conversation list or transcript, and a right inspector showing ONLY the selected history session's actual metadata, project, provider and timestamp; (3) one refined anchored Agent Activity popover reused by the title bar and tray, plus a focused single-Agent variant with only Open Terminal, Open Chat, Inspect actions. All states must share identical typography tokens, control height, gutter sizes, colors and selected/hover/focus treatment. No duplicate top tabs, no new global activity rail, no code/editor panel competing with the terminal, no fake services on History, no marketing graphics, no translucent neon glass, no gratuitous shadows. High fidelity native UI, clean textual hierarchy, realistic spacing, 16:10 macOS desktop aspect ratio, sharp legible microcopy.

## 8. 实施任务与验收

| Task | 顺序 | 修改归属 | 验收 |
|---|---|---|---|
| UI-001 Route Context Gate | P0 已实施（headless） | `context_panel.go` / `right_panel.go` / `panel_motion.go` / `titlebar.go` | History 只显示 History Context；Settings 无右栏；无旧工具闪现 |
| UI-002 Independent Visibility | P0 已实施（headless） | `right_panel.go` | History 展开不影响 Workspace 工具历史状态 |
| UI-003 History Information Density | P0 已实施（headless） | `page_history.go` | 同源摘要不重复，截断有 Tooltip；内容由详情 id 隔离 |
| UI-004 Agent Quick Detail | P0 已实施（headless） | `agent_quick_panel.go`、`quick_panel.go`、`sidebar.go` | 点击 Agent 只显示该 Agent；Back、点击动作、gone-state 安全 |
| UI-005 Quick Panel Polish | P0 已实施（headless） | `agent_quick_panel.go` | 三状态计数可点击、共享真实 Status snapshot |
| UI-006 History Search/Sort/Selection | P1 部分完成（headless）：即时搜索、结果数量反馈、Clear filters、查询前置 Provider/Project 过滤、完整 Provider 发现、仅列表页露出有效控件；排序控件/焦点实机待做 | `page_history.go`、`history_state.go`、`internal/history/service.go` 和 `catalog_read.go` | 筛选期间旧行不可点击、无结果可复原、较早 Provider 不因全局 LIMIT 漏掉；全量/实机门禁仍需通过 |
| UI-007 Workspace Empty & Chat Gate | P1 已实现（headless），实机待验 | `workspace.go`、`chat_ui.go`、`gd_surface.go`、`ds_components.go`、`titlebar.go`、`gitworkbench/tree.go` | 无 Agent 切换拒绝；Agent 消失遮蔽旧 Chat；Git clean 有 View Commits/Terminal，非匹配过滤可 Clear filter；`Flatten(nil)` 不崩溃；三种尺寸需实机视觉验收 |
| UI-008 Settings and Page Headers | P1 核心已实现（headless），交互精修待验 | `page_settings.go`、`page_providers.go`、`ds_components.go` | General/Terminal/Providers/Runtime/Diagnostics 使用唯一 pageHeader 与滚动内容区；消除重复透明背景项；旧表单数据更新路径不变 |
| UI-009 Topbar Semantic Audit | P1 导航主体已实现（headless），Commit Dirty Guard 待做 | `titlebar.go`、`router.go`、`sidebar.go`、`page_history.go`、`page_settings.go`、`navigation_semantics_test.go` | History 子页先回列表/根页二次进 Terminal；内页 Back 不依赖任意上一个路由；Settings/History 同行选中不压栈；⌘[/⌘] 原有 Router 历史保留 |
| UI-010 Responsive & Focus | P2 响应式右栏基础已实现（headless），实机/Focus 待做 | `panel_motion.go`、`right_panel.go`、`titlebar.go`、`shell.go`、`command_center_ui.go`、`panel_motion_test.go` | 中心 560 DIP 保底，面板随宽度自动显示/隐藏、展开偏好不丢；标题栏/快捷键/Command Center 共享宽度规则；主动开栏能腾出空间/不能则提示；跨屏、Reduce Motion、VoiceOver 需真实 GUI 验收 |
| UI-011 Real App Acceptance | P0 阻塞于真实设备驱动 | 独立临时 HOME/Herdr socket 的安装包 | 实际截图、焦点、Terminal 输入隔离、Popover blur、端口服务只出现在 Workspace |
| UI-012 Canonical Design Sweep | P2 待做 | Design Tokens / 组件 | 新增页面禁止私有色板/重复组件；`go test ./...` + MyGo build |
| UI-013 Git Workbench Enhancement | P1 主体已实现（headless/scratch repo），高级阶段待做 | [Git Workbench 专项规划](mygo-git-workbench-product-ux-2026-10-09.md) | Stage/Unstage/Commit 及窄窗 Git Actions，Modal Commit 草稿防丢；Branch FF/no-FF Merge / Revert / Cherry-pick / safe Undo；Git Sequencer 真实冲突/Continue/Abort 和冲突文件定位；AI Prompt 隐私预览与建议手动导入（非自动生成）；右侧 Review Inspector 上下文。真正隔离 AI API、三方编辑器、Rebase 和实机验收仍待做 |

### 定义“闭环”

任何可点击元素必须满足：**触发可解释 → 状态更新准确 → 成功/失败可感知 → 可逆/可返回 → 不改变无关区域 → 键盘可完成**。包含空结果、API 不可用、Agent 消失、History 列表刷新、误触重复打开、顶部关闭后恢复等状态，不只检查 happy path。

### 当前首批代码验证

- 定向单元 / Native UI headless 回归通过（新增 Context Panel、History 与 Quick Panel 行为测试）。
- 第二轮 UI-006：History 的旧筛选态不可误选；Catalog 在 SQL 限额前过滤，真实 Provider 来源不会因 100 条最近列表被隐藏；搜索结果可直接 Clear filters；分组/详情页隐藏不生效的列表搜索控件。定向 headless 测试及全量 Go/MyGo 静态门禁已通过，仍缺真实 GUI 验收。
- 第三轮 UI-007/UI-008：设置页 Header/滚动与内容列统一，删除重复透明背景 Field；Chat 普通 Shell 入口拒绝，无 Agent 时不抢占 History/Settings，Agent 消失时不泄漏已绑定文本或输入框；Git clean 使用 shared workspaceEmptyBanner，提供 View Commits 与 Open Terminal；Git 文件过滤无匹配时由 Clear file filter 恢复，修复 `Flatten(nil)` 导致的实际崩溃（UX-13）。回归位于 `workspace_chat_test.go`、`unified_page_test.go`、`internal/gitworkbench/tree_empty_test.go`。真实 UI 视觉和可访问性尚未完成。
- 第四轮 UI-009/UI-010 部分完成：统一 History 根/详情/By Project 的层级返回；Sidebar 明确 Back to Terminal 并真的切换 Surface；同页 Settings / History 列表项点击不压路由重复记录；`isHistoryRoute` 不接受假前缀；右栏采取 560 DIP 中央内容保底和隐藏偏好恢复策略。`navigation_semantics_test.go`、`panel_motion_test.go` 覆盖路由、独立状态、960/1280 宽度、History Inspector、Command Center Files 入口及 Reduce Motion。
- 第五轮 UI-013：参照 Fork/lazygit，在既有 MyGo 单 Git surface 内新增 Git Actions 主操作条、原生 Commit Modal 和草稿确认；FF-only Merge / Revert / 被本地 remote-tracking ref 覆盖的 HEAD Undo 拒绝由 scratch Git 测试覆盖；AI 尚无独立生成 API，只有明确的 selected diff Prompt 复制。
- 第六轮 UI-013：Branch Merge… 可选 FF-only / 创建 Merge Commit 两种策略（均需最终 Confirm）；历史行新增 Cherry-pick；Merge/Revert/Cherry-pick 冲突经每个 Worktree 的 Git marker 和 unmerged-index 检测，在 Git Diff 主区给出 Continue / Abort + Refresh，右侧 Inspector 显示同一状态；用户在编辑器解决文件后 Stage，Continue 重新校验，Abort 按指定操作类型确认。scratch Git + headless 覆盖不同操作的 Continue/Abort 和切换仓库状态隔离。进一步修复 Git 确认框与 Merge 策略框跨仓库误执行的可能性，以及 dirty Branch Switch 原本确认框缺少 dispatcher 的真实闭环问题。Rebase/复杂三方编辑与真实 Mac 视觉仍待做，见 `mygo-git-workbench-product-ux-2026-10-09.md`。
- 第七轮 UI-013：Git Action Bar 以中央可用宽度为依据，将 Stage All / Unstage All 在窄窗/右栏占位时收进 Git Actions；Git Sequencer 从索引提取最多 24 条去重排序的冲突文件路径，Review Inspector 可定位当前可审阅的文件；AI Commit Prompt 改为先预览再复制，默认仅变更摘要，排除敏感路径，代码片段须用户主动 opt-in，可粘贴 Agent 建议并经过 Subject/Body 内容校验与覆盖确认后导入草稿（绝不自动提交）；移除旧版 Prompt 生成器的隐私旁路。真实 macOS Modal 滚动、AX、焦点、键盘仍待验收。
- 第八轮 UI-013：Git 工具栏取消重复的分支标识、Fetch/Pull/Push 第二排、底部 Commit 与右侧 Inspector Commit；主栏保留 Stage/Unstage（宽窗）、Diff layout、Refresh、Commit 和 More…；分支行保留 Switch 和 More…；Settings 新增 Git & AI 自定义默认 Conventional Commit Prompt / Pi executable。Pi 通过一次性无工具、无会话、无项目上下文的 CLI Print 生成候选文本，提示由 stdin 而不是 argv 提供，带取消/90s 超时、repo+snapshot+path fence；用户审阅并接受后才写入 Commit Subject/Body，实际 git commit 仍由既有 Preflight 完成。真实 Pi Auth/模型可用性与 macOS 窗口的弹层/输入焦点待实机验证。
- 第九轮 UI-013：Commit Modal 使用固定底部 CTA + 独立滚动正文，文件选择显示计数并支持 Select all / Clear selection；Pi 区只在明确展开后展示，Prompt 原文与手动粘贴进一步单独披露，Pi 生成时阻止 Commit，选中文件或偏好变化拒绝过期建议；Branch Popover 新增输入过滤、当前分支优先、240 DIP 滚动区及始终可见的创建行，切仓库清除筛选。headless 960×640 / 1200×800 几何断言已覆盖固定 CTA；真实设备 UI 仍需手动核对。
- **尚未完成**：隔离的真实 macOS 应用窗口逐项操作、完整 A11y / 屏幕尺寸矩阵、History 输入法/焦点/排序控件和浮层跨屏锚点验证。概念图不算 UI 真实渲染测试。
- 不允许直接在用户运行的 Herdr 会话上测试 destructive Git / Agent 操作；需要 scratch repo 和独立 Herdr socket。
- 不以“已编译”替代“已经真正可用”，P1/P2 不能提前称 DONE。

## 9. 后续工作执行原则

每项任务只修改一个明确页面/交互责任域，新增或修改 Native UI 元素先复用既有 tokens / components / existing popover，最后补 regression test 和真实运行截图。架构冲突先更新 `client-product-architecture.md`，没有证据的页面行为不得写成已上线状态。保留所有用户暂存与未提交修改，不自动 `git add/commit/push`。

**最终目标**：无论用户停留在 Terminal、History、Diff、Settings 还是 Agent 弹层，都能用同一组标题栏和导航规则判断当前位置、当前焦点、可执行动作以及返回路径；右边永远在解释左边或中间当前对象，而不是展示一个孤立后台。
