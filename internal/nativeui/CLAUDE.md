# nativeui/
> L2 | 父级: /CLAUDE.md

成员清单
icons.go: 矢量图标字典，inline SVG 定义，提供常用及跨平台窗格/窗口控制图标
proc_brands.go: 进程与工具/服务品牌标识解析器，将前台进程、Agent CLI 与各类微服务/数据库/工具映射为矢量图标与友好名称
brands.go: Agent 品牌位图资产与视觉标识解析器，将 live Agent 映射为全彩品牌 Logo 与 agentMarkForPane 窗格标识
agent_visual.go: Agent 品牌矢量回退图标字典、窗格 Agent 绑定与状态指示点（statusDot）
brands_test.go: Agent 品牌位图资产检索、Antigravity 及其它 Provider 标识与 agentProvider 回退回归
gorex_style.go: 扁平视觉系统：卡片/配色、图标按钮度量（gorexIconBtn·Glyph、gorexPaneBtn·Glyph 为唯一尺寸来源）、gorexPulldown 图标+下拉箭头（标题栏 Actions）与 shortDir/samePath 路径处理；无渐变、无投影
terminal.go: 终端画布与 Gorex 窗格卡片渲染器，Pane 标题双击最大化/恢复、最大化 Tab 的窗格 Logo 数量角标（Tab 内面板数，无重叠堆叠画）、窗格标题取 Agent 名（Shardlane 生成的短名 <kind>-<6hex> 不当标题，回落到 Agent 品牌名）、标题原生拖动放置（跨窗格/跨 Tab 移动、拆分与交换）与 split 分隔条拖拽调宽高（解析为 Herdr pane.resize，双击回到 50/50；布局权威在 Herdr）
breadcrumb_switcher.go: 标题栏面包屑与快速切换弹层所有者，Pane 顺序与 Herdr 实际布局对齐并展示品牌图标
titlebar.go: 内容列标题栏（面包屑与工具区）、整高侧栏上的红绿灯带与侧栏切换钮（sidebarBand/sidebarToggle）、视图切换（History 内页先返回根）与 Windows 平台窗口控件支持
router.go: 单一 Router 的页面归属、精确 History 路径分类与窗口标题路由语义
navigation_semantics_test.go: UI-009 顶栏/侧栏同级返回、重复点击、深链接与浏览历史回归
panel_motion.go: 整高侧栏（macOS vibrancy 材质、无 vibrancy 时不透明回退）的收起/展开动画；无右栏布局宿主
panel_motion_test.go: Reduce Motion 下左导航收起/恢复，以及各窗口宽度无右栏回归
right_panel.go: 旧会话设置兼容字段及 Files 缓存、根目录 identity fence、URL Preview 管理；**不绘制右栏**
right_panel_files.go: /files 左侧目录缓存/树（共用侧栏网格行与项目/分支头部）及点击加载预览的适配器
page_files.go: Header Files 独立页面：左树/中预览、当前 Tab CWD 归属和空态
quick_panel.go: 托盘与顶栏 Agent 快捷浮层宿主，提供 Agent 概览与 Services 双 Tab 及详情卡片
quick_panel_services.go: Agent 浮层里的 Services 内容：当前 Tab Scripts 与观测端口
quick_panel_test.go: QuickPanel 尺寸伸缩、卡片详情点击与空态行为测试
right_panel_git_test.go: Diff/历史 Commit 保留左导航/中预览而不绘制旧 Review Inspector
command_center_ui.go: Command Center Files 操作直达独立 /files 页面，不受旧右栏宽度限制
project_tree.go: 侧栏 Workspace/Tab/Pane 原生拖拽入口及运行状态树，尊重 Herdr 权威顺序而非按 Agent 状态重排；行内无 "…"，项目行悬停 "+"（New Tab），其余动作经右键菜单
reorder.go: 作用域隔离的拖放语义与目标插入索引、跨 Tab/Project 移动与拆分位置计算、Herdr mutation 分发和 Pane layout 顺序投影
reorder_test.go: Drag/Drop 真正鼠标事件、插入索引、跨 Tab/Project 移动校验、窗格卡片放置动作推导和投影排序回归
components.go: 共用导航树行与视觉组件（treeRow 悬停图标动作 Action）、sidebarRowHeight 密度令牌，拖动时提供原生浮动预览、同级插入占位条、容器高亮、动画及命中目标
workbench.go: 桌面工作台状态中枢，Agent 目录与运行时事件分发
page_history.go: Sessions 中心列表布局（参照 magpie 会话历史）：Agent 标签条（All + 各 Provider，带会话数）、搜索/Select/Hidden/Refresh 工具簇（窄窗口整体换行）、项目分组卡片（默认只展开最近一个项目，头部在卡内）与行（标题/事实行/Token 列/日期↔悬停菜单固定占位，点行内展开预览）、有界结果计数文案、扫描未结束时的 Updating 提示（分批渐进加载）
history_state.go: HistoryService 列表查询的生成代际隔离，筛选清空旧可选行/预览/多选；Provider 元数据选择与各 Provider 总数（可选 HistoryProviderCountView）、扫描分批上报（可选 HistoryScanProgressView：每批约 100 个来源在窗口线程刷新一次可见列表，数千个单会话文件夹不再把页面按在 Loading 态）；分组展开、多选、行内预览与详情页帧戳（只读 Chat 绑定的空闲释放）状态
history_select.go: Sessions 多选：行/项目头勾选标记、底部批量 Hide/Restore 栏；只改本地隐藏列表，不碰 Provider 会话文件
history_preview.go: 行内会话预览：点行懒读原始会话末尾若干条（Tail 窗口，代际防过期），只读展示，带 Open/Continue/Hide/Restore；不缓存任何会话内容
history_detail.go: /history/{id} 详情页：头部（标题、Provider · 项目 · 模型、操作菜单）+ 与 Chat 共用的只读视图（WebView 页或原生时间线回退，最新消息优先、向上懒加载）；页面停止构建后释放只读绑定
history_continue.go: Sessions 的 Continue / Continue in <Agent>：只读规划（PlanHistoryContinuation）→ 已在运行则定位窗格，否则经共享确认框后由既有 Herdr launch transaction 新建 Tab 续接（同 Agent 原生 resume 或跨 Agent 带最新消息简报），成功选中新窗格；失败按类给一行文案（shell 没就绪、Agent 没启动、可能已启动等分开说，不猜原因），已创建的窗格直接展示；引擎经 continuationRuntime 缝可被测试替换
text_clip.go: Inspector 摘要与工具输出预览共用的按字节/行数 Unicode 安全截断（clipNativeText/clipPreview）
history_test.go: History 页面真实 Catalog 场景回归
history_select_test.go: 多选批量 Hide/Restore、项目头勾选与恢复、筛选清空选择、setSessionsHidden 边界、最近项目默认单开回归
history_preview_test.go: 行内预览懒读、代际防过期、阻塞读取、Hide/Restore 与 Open 跳转回归
history_continue_test.go: Continue 先确认再动、同 Agent 续接/跨 Agent 交接（简报取自原文最新窗口）、已在运行只定位窗格、无项目目录的说明、失败分类与已创建窗格的定位、行菜单/预览入口与禁用态回归（假体，不启动真实 Agent）
history_detail_test.go: 详情共享 Chat 只读绑定、最新消息优先与向上分页、切换重绑/离开释放、不可读来源恢复态回归
text_clip_test.go: 截断的 Unicode 边界、行/字符上限与截断标记回归
history_filter_test.go: History 筛选空态、结果计数、Provider 选项与旧行不可点击的 headless 验证
history_layout_test.go: Sessions 页 Agent 标签（含数量与隐藏扣减）、行 Token 列与悬停占位、有界计数文案、窄窗口工具条换行回归
sidebar.go: 唯一左栏宿主——Workspace/Agent/Pin 树、Git 引用导航、内页（Settings/History/Files）自有导航；内页返回控件统一固定在侧栏页脚（取代 Workspace 切换页脚；会话详情返回 Sessions，其余返回 Terminal）
sidebar_test.go: 内页侧栏导航与页脚返回的位置/文案、Settings 导航、Pin 区块回归
detail_fact.go: Agent 详情复用的只读字段样式，从退役 Inspector 解耦
shell.go: 核心 Shell 宿主，协调 Router/Window/Titlebar；首次 Herdr 加载独占中心展示并淡入工作区，Git sidebar 与双模式分栏展示态仅本地持有
page_settings.go: 五个 Settings 分组页（General/Terminal/Agents/Git/Diagnostics）统一 settingsPage Header/Scroll/最大内容宽度；设置项只经 settingsGroup/formRow 分组行表达，支持外观主题/透明度/密度与终端配置；Diagnostics 兼容分组承载 Rich chat view 开关（实现细节，不放在 General）
page_providers.go: Agents 设置页——单一分组列表，每个 Agent 一行（品牌 tile + 状态胶囊 + 单一动作）；直连 Status hook 为主，Herdr integration 作为 attached 子行降级为 fallback（hook 健康时仅次要展示、不提供动作），跟随 settingsPage 公共头部与滚动容器
providers_ui.go: Providers 统一状态机（经 herdr.RunCLI 取 integration 状态；失败只显示人话——integrationErrorText，细节进日志；一次审计同时覆盖两面、进入/刷新/动作后审计、hook 主导 + fallback 修复动作门控、在途防重、一次性 toast、两面状态→语义色映射）
workspace.go: Terminal/Chat/Diff/Commit 唯一主表面，Changes 主内容采用上下可调 Git Review，Chat 保持 Agent 身份门禁
chat_ui.go: Chat Timeline/Composer 和身份消失时的安全空态，不得泄漏旧 transcript；bindChatWindow（Tail 窗口）供 Workspace 历史绑定与 Sessions 详情只读绑定共用，releaseHistoryChat 保证只读绑定不外泄给窗格/Inspector
gd_surface.go: 下方 Git Diff、无仓库/无变更/无匹配状态，clean 时直达 All Commits 或 Terminal；历史模式只读，工作树工具栏单一调用点；大 diff 为一行占位（Large diff · Show/Retry），点 Show 才按需读取；卡片头按 gdCardWidth（窗口−侧栏−已打开的文件导航−边距，与 gitFilePaneLimits/gitFilePaneOpen 同源）分档，窄卡让出复制/打开按钮与 Viewed 文案、文件夹退到第二行，保证文件名完整
gd_header_test.go: 卡片头响应式回归——900 宽（用户打开导航、双栏全开）文件名不被压成省略桩、宽窗口保留全部控件、gdCardWidth 随侧栏/导航开合与拖拽变化
gd_perf_test.go: Diff 大文件基线（行模型构建/帧耗时 benchmark）与惰性计算结构性回归——词级 diff 按需、度量只算一次、快照刷新保留缓存、就近优先加载
gd_large_test.go: 大 diff 占位→Show→按需读取→渲染 端到端回归（真实仓库）；失败重试、过大无动作、刷新保留/失效语义、不预读颜色
git_workbench_actions.go: Git 视图操作条（Stage/Unstage All、Fetch/Pull/Push 图标、始终可见 Split/Unified、Git actions 下拉）；仓库 Refresh 唯一入口在 Git 侧栏，按 Diff 实际宽度折叠批量操作；首位是文件列表开关（gitFilePaneToggle）
git_sidebar_tree.go: 左侧 Git 引用导航，Worktrees/Branches/Remotes/Tags/Stashes 共用 sidebarTreeRow；分区悬停 "+" 与右键菜单承载创建动作，无 "···"
git_ref_tree.go: Git 引用按斜线构树、目录键稳定、筛选时独立折叠/再次展开、行经 sidebarTreeRow（网格+导向线）、右键菜单，不执行 Git IO
git_review_split.go: Changes 双模式主工作台，Local Changes 文件树+Diff+底部 Commit 分离按钮（经 Pi 只填 Subject）；All Commits 为图谱上层+只读文件树/Diff 下层；文件导航宽度及上限唯一来源 gitFilePaneLimits；中心区（窗口−侧栏）<750 时导航自动收起，开合偏好按宽/窄两档各自记忆，工具栏 gitFilePaneToggle 取回（Local Changes 操作条与 All Commits 的 Changes 页签共用）
git_review_split_test.go: Git 双模式分栏、960 DIP 几何、拖拽、历史只读与底部草稿统一 Commit Modal 测试；文件导航窄窗口自动收起/按中心区判断/宽窄两档偏好各自记忆/提交评审同规则
git_tree_interaction_test.go: 所有 Git 引用组反复折叠/展开、筛选后目录折叠与恢复回归
git_file_tree_fold_test.go: Local Changes/All Commits 中重复点击相同文件目录的折叠与再展开回归
git_directory_actions.go: Staged/Unstaged 独立目录子文件解析，路径边界准确，不以目录路径批量 Git add
git_quick_ai.go: 底部 Pi 提交生成器；异步 root/snapshot/草稿门禁、仅安全元数据、只填 Subject，拒绝自动提交
git_workbench_polish_test.go: Split/Unified 真实点击、目录选中与边界、唯一 Refresh、Git actions 菜单和异步草稿门禁回归
git_commit_graph.go: 历史提交真实父 SHA 的 merge lanes 纯渲染投影；无运行时 Git IO
git_commit_graph_test.go: 提交父节点拓扑及合并分支回归
git_layout_render_test.go: 仅显式启用时输出 Local Changes/All Commits 实际 MyGo 渲染截图
changes_panel.go: Changed File Tree 行落在共用侧栏网格（chevron 槽·图标·标题·悬停图标动作·+N -N·状态字母）、导向线、行高走密度令牌；目录每次直接点击均切换折叠，不依赖 List.Changed；文件行保留多选及 Diff 导航
bootstrap_page.go: 启动时全屏居中 Loading 或可重试的错误页面，尚未初次完成 Herdr projection 时不挂载侧栏或顶部菜单
bootstrap_page_test.go: 初次加载/故障时隐藏交互、中心位置与就绪后切换的 headless 回归
runtime.go: 首次 Herdr 列举、异步 Projection、初次 terminal coordination 完成前的 bootstrap gate；移除第二次阻塞枚举
panel_motion.go: Git/Terminal 左栏统一 252 DIP 宽度和折叠动画，不再依路由改变宽度
repo_bar.go: 仅保留变更文件点击与 toggleMenu popover 辅助函数；仓库菜单见 git_sidebar_tree 与 sidebar_switchers
branch_menu.go: 当前分支优先的搜索过滤与受保护的切换/创建入口，弹层由 sidebar_switchers 持有
sidebar_tree.go: 侧栏层级唯一对齐网格（chevron 槽·图标·标题、首层对齐分区标题）、导向线、统一行构建器 sidebarTreeRow 与行高令牌 treeRowHeight；Git 引用与 Files 树共用
sidebar_switchers.go: Git/Files 侧栏共用头部：项目名与分支名各自是可搜索的 pull-down 快速切换，外加刷新图标
sidebar_tree_test.go: 网格对齐、导向线连续、分区无 "···"、悬停 "+"、创建对话框、项目/分支快速切换、Workspace 行无 "…" 与标题栏 Actions 回归
changes_tree_test.go: Changes 文件树共用网格（逐层缩进一格）与行内图标动作命名回归
layout_width_test.go: 宽窗口内容列居中封顶（Settings 760、Sessions 1040、提交表单 760）回归
git_service_snapshot.go: 仓库绑定快照/提交历史及本地远程跟踪引用的分离异步获取与 generation root 隔离
commit_surface.go: 原生 Commit Dialog 唯一草稿，滚动正文 + 固定底部 Commit/Cancel、Select all/Clear selection
commit_ai_ui.go: Commit 内折叠的 Pi 生成、敏感路径受限 Prompt 原文预览与复制、手动候选消息编辑；默认只展示必要选项
commit_modal_layout_test.go: 960×640/1200×800 长文件与 Pi 展开后固定 CTA 几何、文件选择批量操作回归
branch_menu_filter_test.go: 搜索过滤与当前分支优先
commit_pi.go: Pi 一次性候选生成 UI 协调、后台取消/超时、repo/snapshot/paths identity fence，拒绝自动 git commit
page_settings_git.go: Git 配置页：可编辑的只含风格的 Conventional Commit 规则（安全约束固定在 agent.BuildPiCommitPrompt，不在页面展示）、显式保存/恢复、Pi CLI 可执行路径
design_system.go: 排版/间距/圆角/控件高度/TileTone 令牌，字号字重只经此处取值，禁止字面量
theme.go: 中性调色板与 DesignTokens；Sidebar 仅在窗口无 vibrancy 材质时作为不透明回退
ui_evidence_test.go: 显式启用的视觉取证（SHARDLANE_UI_EVIDENCE=<dir> 渲染主要视图的深浅色 PNG，不访问真实会话）
commit_pi_test.go, settings_git_test.go: Pi 后台时序/过期消息隔离、Git Settings 持久化及 MyGo 控件回归
commit_message_import.go: 生成候选文本纯校验与 Subject/Body 导入，已有手写消息时必须二次确认，不触发 Git 提交
commit_message_import_test.go: 消息长度/代码围栏/空主题校验、覆盖确认及本地状态销毁回归
workspace_header.go: Commit Modal 打开/取消/草稿导航防护，仍使用既有 WorkspaceSurfaceCommit 主状态
git_operations.go: 背景 mutation 编排，FF/no-FF Merge、Revert/Cherry-pick/Undo、Git Continue/Abort，经确认并在失败后刷新
git_sequencer_ui.go: Git Diff 顶栏冲突提示、操作准确的 Continue/Abort/Refresh；右侧 Inspector 共用状态和指引
git_sequencer_ui_test.go: Merge/Revert/Cherry-pick 的 headless 冲突状态、按钮安全门禁和仓库切换隔离
dialogs.go: Git 危险动作确认绑定打开时 repository root；Merge 策略 Modal 失效时关闭；Confirm/Cancel 保持安全默认
branch_menu.go: 本地分支 Popover 可搜索/独立滚动/当前分支优先，Switch/More 层级；Merge FF/no-FF 策略和 dirty Branch Switch 的真实确认分发
git_workbench_actions_test.go: Git 本地操作条、Modal、Draft Guard、Merge/Revert Confirm 的 headless 回归
ds_components.go: 通用卡片、settingsGroup/formRow 分组行（控件先于标签入树，保证焦点/朗读顺序）、groupLabel、iconTile 彩色图标块、compactButton/compactPrimaryButton 紧凑按钮层级（密集工具栏/行内操作的唯一字号来源，业务代码不再对 Button 内联 FontSize）、工作区 Chat/Diff 轻量空状态组件（窄列时操作换行到消息下方，消息不被挤成单字列）
unified_page_test.go: UI-007/UI-008 Settings/Git/Chat 跨页面基础回归与尺寸矩阵（headless；含窄窗口 clean 状态可读性）
pane_header_visuals_test.go: 窗格头部视觉解析、Agent 图标与名称解析、路径格式化及最大化 Tab 窗格计数单元测试
chat_hooks.go: Shell 钩子层：挂载原生 apphook 进程内 IPC 服务与 spool 兼容层，直接接收并更新 Agent 状态与会话绑定，驱动 blocked 交互检测
chat_hooks_test.go: 钩子事件解析、提示词检测、投影等价去重与原生 apphook 状态/会话更新回归

[PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
