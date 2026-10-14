# nativeui/
> L2 | 父级: /CLAUDE.md

成员清单
icons.go: 矢量图标字典，inline SVG 定义，提供常用及跨平台窗格/窗口控制图标
proc_brands.go: 进程与工具/服务品牌标识解析器，将前台进程、Agent CLI 与各类微服务/数据库/工具映射为矢量图标与友好名称
brands.go: Agent 品牌位图资产与视觉标识解析器，将 live Agent 映射为全彩品牌 Logo 与 agentMarkForPane 窗格标识
agent_visual.go: Agent 品牌矢量回退图标字典、窗格 Agent 绑定与状态指示点（statusDot）
brands_test.go: Agent 品牌位图资产检索、Antigravity 及其它 Provider 标识与 agentProvider 回退回归
gorex_style.go: 扁平视觉系统：卡片/配色、图标按钮度量（gorexIconBtn·Glyph、gorexPaneBtn·Glyph 为唯一尺寸来源）、gorexToggleFace 按钮的按下/悬停/「开」态唯一来源（图标开关与文字开关同一枚中性芯片；列表/树行的选中仍是 SidebarSelect）、gorexIconTab 等分一行的图标标签（侧栏视图标签条，与 gorexIconButton 共用 gorexIconFace 的脸）、gorexPulldown 图标+下拉箭头（标题栏 Actions）与 shortDir/samePath 路径处理；无渐变、无投影
terminal.go: 终端画布与 Gorex 窗格卡片渲染器，Pane 标题双击最大化/恢复、最大化 Tab 的窗格 Logo 数量角标（Tab 内面板数，无重叠堆叠画）、窗格标题取 Agent 名（Shardlane 生成的短名 <kind>-<6hex> 不当标题，回落到 Agent 品牌名）、标题原生拖动放置（跨窗格/跨 Tab 移动、拆分与交换）与 split 分隔条拖拽调宽高（解析为 Herdr pane.resize，双击回到 50/50；布局权威在 Herdr）；窗格头的目录文案不复述已有信息——标签已写出目录（titleSpellsOutDir，F168 去重对 shortDir 截断路径失效的修复）或单卡时标题栏第二行已是该窗格目录（paneCardCaption）都不再出第三份
run_command_button.go: 窗格卡片头的项目命令运行控件：嗅探命令（package.json/Makefile/justfile + 手写 Script 合并）经 root 键缓存读盘外构建；Run 直跑最后使用命令、菜单全量列出（last-used 置顶）；忙碌（pane.process_info 扫描事实）非 Agent 窗格变 Stop（pane.send_text 写 ETX=Ctrl+C）；事务走 ShellReady 探测 + pane.send_text 字面写入（agent.send_keys 只认 Agent 目标，2026-10-13 隔离探针验证；headless 按 mutate 规则丢弃），无新 Herdr 方法
run_command_button_test.go: 运行控件状态机回归（命令菜单内容、last-used 置顶、忙碌换 Stop、Agent 繁忙隐藏、无命令隐藏、headless 点击安全丢弃）与 computePaneBusy 纯函数、合并形状测试
breadcrumb_switcher.go: 标题栏面包屑与快速切换弹层所有者，Pane 顺序与 Herdr 实际布局对齐并展示品牌图标
titlebar.go: 内容列标题栏（身份区：Terminal/Chat 为面包屑 + cwd 双行，Changes 与各内页为 route_header.go 的单行 Title 大字标题；标题栏是页面标题与页面动作的唯一归属）、整高侧栏上的红绿灯带与侧栏切换钮（sidebarBand/sidebarToggle）、showViewChanges/History/Files/Hosts 视图切换入口（History 内页先返回根）与 Windows 平台窗口控件支持；工具区只剩 Agent 活动与置顶，四个视图按钮已移到侧栏顶部（sidebar.go sidebarViewTabs）
route_header.go: Terminal/Chat 以外各页的标题栏身份（routeHeader/routeHeaderSpec：Changes[Diff/Commit 表面，Actions=表面菜单]、Sessions、Session 详情[灰色父级 Sessions › 会话标题，可返回，同行事实，会话操作菜单]、Files[选中文件时 Copy Path/Reveal in Finder，根路径只在标题悬停提示]、Hosts[SSH 配置路径只在悬停提示；选中主机时 Actions=该主机菜单]、Settings[灰色 Settings › 分组，由 settingsSections 命名]、Inspector）与 routeHeaderBlock（单行 Title 大字加粗标题，无 crumb 胶囊无第二行，每页同字号同位置）；headerDetailRow 只服务 Workspace 面包屑的第二行
unified_header_test.go: 标题栏单一归属回归——内页标题只在标题栏且各路由（含 Changes）落在同一位置、标题为 Title 大字单行（不复述根路径/配置路径成第二行）、内页/侧栏不自画页头或标题回声、每个内页至多一个 Refresh 且为图标、Sessions/Settings/Hosts 侧栏列表同一起点（x/y）、Session 详情标题栏可返回列表、各路由 routeHeader 身份
toggle_face_test.go: 开关「开」态回归——Sessions 工具栏的文字开关（Select/Trash）与标题栏图标开关同为中性芯片，不再是蓝色选中色（像素采样，深浅两套外观）
copy_style_test.go: 文案大小写回归——emptyState/workspaceEmptyBanner 的字面标题一律句首大写（专有名词除外）；菜单项（.Item("…") 字面量）一律 Title Case（冠词/短介词小写），拦下两个方向的大小写漂移
router.go: 单一 Router 的页面归属、精确 History 路径分类与窗口标题路由语义
navigation_semantics_test.go: UI-009 顶栏/侧栏同级返回、重复点击、深链接与浏览历史回归
panel_motion.go: 整高侧栏（macOS vibrancy 材质、无 vibrancy 时不透明回退）的收起/展开动画与尾缘拖拽调宽（sidebarLimits 钳制，宽度会话持久化）；无右栏布局宿主
panel_motion_test.go: Reduce Motion 下左导航收起/恢复，以及各窗口宽度无右栏回归
right_panel.go: 旧会话设置兼容字段及 Files 缓存、根目录 identity fence、URL Preview 管理；**不绘制右栏**
right_panel_files.go: /files 左侧目录缓存/树（共用侧栏网格行与项目/分支头部；工具条用共用 gorexIconButton，开关态与全 App 同一枚中性芯片，无私有 accent 底；整棵树是 listNav 焦点组，进页即聚焦打开的文件/首行，光标行上 Vim 键见 list_nav.go，a/A/r/d/y 经 files_ops.go 的 filesRowKeys 直通新建/重命名/删除/复制路径）及点击加载预览的适配器
page_files.go: Header Files 独立页面：左树/中预览（查看条 + 分级预览体）、当前 Tab CWD 归属和空态；页面只画内容，标题/根路径/文件动作在标题栏（route_header.go）
files_native_preview.go: Files 预览分级计划与原生层——选中后离 UI lane 探测 size×flavor×偏好：文本/代码/Markdown 源码进只读 editor.View（Buffer 离 UI lane 构建，8 MiB 上限、截断胶囊、散文自动换行），Markdown 渲染与网页图片交 WebView 页，解码得了的图片原生位图，其余给真实失败态；查看条（名称/大小/截断/Rendered⇄Source 分段）横跨所有层级；重命名、偏好切换、Source 切换都重新计划
code_theme.go: codeEditorTheme——把 Git 评审的语法色/代码色/行号色（gd_palette 单一来源）穿到框架 editor 上，同一份代码在 Files 与 Git 评审里同色
files_preview_test.go: 预览分级回归（文本/代码原生、Markdown 渲染⇄源码与超限、网页图片与「Rich file preview」关闭、截断/二进制/空文件/CJK、重命名重新计划、桥只放行页面拥有的文件、语法配色单一来源）
quick_panel.go: 托盘与顶栏 Agent 快捷浮层宿主：顶部 Agents/Services 分段（浮层无标题），Agents 列表按状态分组，一层到底——点击行直达面板并收起浮层；高度随内容增长（预算常数取自真实布局）
quick_panel_services.go: Agent 浮层里的 Services 内容：当前 Tab Scripts 与观测端口（分区标题与 Agents 分组同一构件）
quick_panel_test.go: QuickPanel 高度预算（纯函数 + 真实布局贴合含用量行）、状态分组、点击直达（Terminal/Chat/记忆面板/从 Diff 返回）、右键菜单与行尾标记已审阅回归
agent_quick_panel.go: 浮层的状态分组（quickPanelGroups）、分组标题（quickPanelGroupHeader）、条目行（点击直达、行尾标记已审阅、右键菜单）与直达动作 openQuickPanelEntry/openAgentTerminal；另保留历史遗留的统计 tile（无调用方）
agent_quick_panel_test.go: 状态分组的优先级顺序、空组省略与组内保序回归
agent_card.go: 侧栏与 Agent 浮层共用的紧凑 Agent 卡片 agentCardRow（标记/标题/供应商/状态点，可选行尾槽）
right_panel_git_test.go: Diff/历史 Commit 保留左导航/中预览而不绘制旧 Review Inspector
command_center_ui.go: Command Center Files 操作直达独立 /files 页面，不受旧右栏宽度限制
project_tree.go: 侧栏 Workspace/Tab/Pane 原生拖拽入口及运行状态树，尊重 Herdr 权威顺序而非按 Agent 状态重排；行内无 "…"，项目行悬停 "+"（New Tab），其余动作经右键菜单
reorder.go: 作用域隔离的拖放语义与目标插入索引、跨 Tab/Project 移动与拆分位置计算、Herdr mutation 分发和 Pane layout 顺序投影
reorder_test.go: Drag/Drop 真正鼠标事件、插入索引、跨 Tab/Project 移动校验、窗格卡片放置动作推导和投影排序回归
components.go: 共用导航树行与视觉组件（treeRow 悬停图标动作 Action）、sidebarRowHeight 密度令牌，拖动时提供原生浮动预览、同级插入占位条、容器高亮、动画及命中目标
workbench.go: 桌面工作台状态中枢，Agent 目录与运行时事件分发
page_history.go: Sessions 中心列表布局（参照 magpie 会话历史）：Agent 标签条（All + 各 Provider，带会话数；回收站里按回收站内容计数）、搜索/Select/Trash（+Empty Trash）/Refresh 工具簇（窄窗口整体换行；Select 与 Trash 只在挂了回收站时出现，Trash 有内容或视图打开才显示）、项目分组卡片（默认只展开最近一个项目，头部在卡内）与行（标题/事实行/Token 列/日期↔悬停菜单固定占位，点行内展开预览；回收站视图复用同一列表，行键是条目 id、选择常开）、行菜单 Move to Trash（无回收站则无此项）、listNav 键盘（进页聚焦开场行；j/k 跨项目头、h/l 折叠项目与行内预览、o 切换预览；光标亮时 d 移入回收站，回收站里 u 放回、d 永久删除——先确认；行离开后光标让位邻行）、有界结果计数文案、扫描未结束时的 Updating 提示（分批渐进加载）
history_state.go: HistoryService 列表查询的生成代际隔离，筛选清空旧可选行/预览/多选；Provider 元数据选择与各 Provider 总数（可选 HistoryProviderCountView）、扫描分批上报（可选 HistoryScanProgressView：每批约 100 个来源在窗口线程刷新一次可见列表，数千个单会话文件夹不再把页面按在 Loading 态）；分组展开、多选、行内预览与详情页帧戳（只读 Chat 绑定的空闲释放）状态；回收站条目缓存与视图开关（trashItems/showTrash，trashGen 代际防过期）
history_select.go: Sessions 多选：行/项目头勾选标记、底部批量栏（会话视图：Move to Trash；回收站：Put Back / Delete Forever…，有勾选才出现）；选择只是 UI 状态，文件搬动全在 history_trash.go
history_preview.go: 行内会话预览：点行懒读原始会话末尾若干条（Tail 窗口，代际防过期），只读展示，带 Open/Continue/Move to Trash；不缓存任何会话内容
history_trash.go: Sessions 删除 = 移入回收站（取代旧的 Hide；经 SessionTrashView 接缝只调 internal/sessiontrash，不新增 Herdr 方法）：移入（后台线程搬文件→摘除 Catalog 行→关预览/详情→刷新列表与回收站，Undo 提示条）、放回（先搬回再同步 Scan，不覆盖既有文件，冲突有专门提示）、永久删除/清空（二次确认，对话框 kind history-delete-forever / history-empty-trash）、回收站视图的行（条目 id 作行键，标题等从被搬走的文件按需读回）/计数/筛选/菜单，以及保护：Herdr 投影里运行中 Agent 的会话身份（文件路径或原生 id，且 Provider 一致）命中、或文件 60 秒内刚写过的会话不搬；日志只记操作名/数量/稳定 id，不记标题与正文
history_detail.go: /history/{id} 详情页：与 Chat 共用的只读视图（WebView 页或原生时间线回退，最新消息优先、向上懒加载）；页面停止构建后释放只读绑定；标题、Provider · 项目 · 模型与操作菜单在标题栏（route_header.go），页面不自带头部
history_continue.go: Sessions 的 Continue / Continue in <Agent>：只读规划（PlanHistoryContinuation）→ 已在运行则定位窗格，否则经共享确认框后由既有 Herdr launch transaction 新建 Tab 续接（同 Agent 原生 resume 或跨 Agent 带最新消息简报），成功选中新窗格；失败按类给一行文案（shell 没就绪、Agent 没启动、可能已启动等分开说，不猜原因），已创建的窗格直接展示；引擎经 continuationRuntime 缝可被测试替换
text_clip.go: Inspector 摘要与工具输出预览共用的按字节/行数 Unicode 安全截断（clipNativeText/clipPreview）
history_test.go: History 页面真实 Catalog 场景回归
history_select_test.go: 多选模式机制（Select 进出、行点击勾选而非展开、项目头勾选不折叠、筛选/回收站切换清空选择）、最近项目默认单开回归；删除/放回的真实文件语义在 history_trash_test.go
history_trash_test.go: Sessions 回收站回归（真实 Provider 夹具文件 + 真实 HistoryService/sessiontrash.Store）：行菜单 Move to Trash 搬文件/摘行/Undo 提示、Undo 放回、回收站列表读回标题与放回、Agent 标签与搜索、永久删除/清空先确认（取消不动）、批量移入、刚写入/运行中会话拒绝、放回冲突不覆盖、详情页与预览里的移入、无回收站不出现删除入口；含内存替身 fakeTrash 与已变异验证过的守卫
history_nav_test.go: Sessions 列表键盘回归——进页聚焦且不亮、j/k 跨项目头与边界、h/l 折叠项目与预览（l 只开不关）、o 切换、选择模式里行是叶子、d 移入回收站（不亮不响应、光标让位邻行、最后一行上移）、会话在用拒绝、回收站里 Ctrl-d 不是删除/d 先确认并把键盘还给光标/u 放回、搜索框里字母仍是文字
history_preview_test.go: 行内预览懒读、代际防过期、阻塞读取与 Open 跳转回归
history_continue_test.go: Continue 先确认再动、同 Agent 续接/跨 Agent 交接（简报取自原文最新窗口）、已在运行只定位窗格、无项目目录的说明、失败分类与已创建窗格的定位、行菜单/预览入口与禁用态回归（假体，不启动真实 Agent）
history_detail_test.go: 详情共享 Chat 只读绑定、最新消息优先与向上分页、切换重绑/离开释放、不可读来源恢复态回归
text_clip_test.go: 截断的 Unicode 边界、行/字符上限与截断标记回归
history_filter_test.go: History 筛选空态、结果计数、Provider 选项与旧行不可点击的 headless 验证
history_layout_test.go: Sessions 页 Agent 标签（含数量）、行 Token 列与悬停占位、有界计数文案、Trash 开关出现规则与窄窗口工具条换行回归
sidebar.go: 唯一左栏宿主——栏顶四个等宽图标视图标签（sidebarViewTabs：Changes/Sessions/Files/Hosts，每条路由同位置，点亮页再点回 Terminal）、Workspace/Agent/Pin 树、Git 引用导航、内页（Settings/History/Files）自有导航；内页返回控件统一固定在侧栏页脚、一律叫 Back（取代 Workspace 切换页脚；tooltip 写去向：会话详情→Sessions，其余→Terminal）
sidebar_test.go: 内页侧栏导航与页脚 Back 的位置/文案、栏顶四个视图标签的位置/等宽/同位置/点击语义、Settings 导航、Pin 区块回归
detail_fact.go: Agent 详情复用的只读字段样式，从退役 Inspector 解耦
shell.go: 核心 Shell 宿主，协调 Router/Window/Titlebar；首次 Herdr 加载独占中心展示并淡入工作区，Git sidebar 与双模式分栏展示态仅本地持有；WithSessionTrash 挂 Sessions 回收站，pendingToastAction 在 View 里发带按钮（Undo）的提示条
page_settings.go: 五个 Settings 分组页（General/Terminal/Agents/Git/Diagnostics）统一 settingsPage Scroll/最大内容宽度（无页内标题：标题栏按 settingsSections 表命名分组，侧栏列表也不再回声 "Settings"）；设置项只经 settingsGroup/formRow 分组行表达，支持外观主题/透明度/密度与终端配置；Diagnostics 兼容分组承载 Rich chat view 开关（实现细节，不放在 General）
page_providers.go: Agents 设置页——单一分组列表，每个 Agent 一行（品牌 tile + 状态胶囊 + 单一动作）；直连 Status hook 为主，Herdr integration 作为 attached 子行降级为 fallback（hook 健康时仅次要展示、不提供动作），跟随 settingsPage 滚动容器；Refresh 为工具条图标按钮（与全 App 刷新同一形态）
page_hosts.go, hosts_nav.go, hosts_groups.go, hosts_form.go, hosts_editor.go: /hosts 内页（详情中心 + 侧栏 SSH Config/Herdr Machines 列表）；页面无自带页头，列表头的图标刷新是该页唯一的 Refresh；侧栏按配置自带分组（SSH Config Editor 式注释标记 + Include 文件，只读，`hostGroups`）可折叠，无可分组内容时平铺，行/分组头走共享 sidebarTreeRow/sidebarSectionHeader（悬停 Connect 图标、右键与标题栏 Actions 同一份 hostEntryMenu）；详情 = 一行标题（Heading + file:line）+ Connect + 铅笔图标 + 单个 settingsGroup 指令列表，结构化编辑就地发生（Cancel/Save 顶替、Return 保存），生效配置是折叠行；hostsMainFile 用索引解析后的主文件判断 include
providers_ui.go: Providers 统一状态机（经 herdr.RunCLI 取 integration 状态；失败只显示人话——integrationErrorText，细节进日志；一次审计同时覆盖两面、进入/刷新/动作后审计、hook 主导 + fallback 修复动作门控、在途防重、一次性 toast、两面状态→语义色映射）
workspace.go: Terminal/Chat/Diff/Commit 唯一主表面，Changes 主内容采用上下可调 Git Review，Chat 保持 Agent 身份门禁
chat_ui.go: Chat Timeline/Composer 和身份消失时的安全空态，不得泄漏旧 transcript；bindChatWindow（Tail 窗口）供 Workspace 历史绑定与 Sessions 详情只读绑定共用，releaseHistoryChat 保证只读绑定不外泄给窗格/Inspector
gd_surface.go: 下方 Git Diff、无仓库/无变更/无匹配状态，clean 时直达 All Commits 或 Terminal；历史模式只读，工作树工具栏单一调用点；大 diff 为一行占位（Large diff · Show/Retry），点 Show 才按需读取；卡片头按 gdCardWidth（窗口−侧栏−已打开的文件导航−边距，与 gitFilePaneLimits/gitFilePaneOpen 同源）分档，窄卡让出复制/打开按钮与 Viewed 文案、文件夹退到第二行，保证文件名完整
gd_header_test.go: 卡片头响应式回归——900 宽（用户打开导航、双栏全开）文件名不被压成省略桩、宽窗口保留全部控件、gdCardWidth 随侧栏/导航开合与拖拽变化
gd_perf_test.go: Diff 大文件基线（行模型构建/帧耗时 benchmark）与惰性计算结构性回归——词级 diff 按需、度量只算一次、快照刷新保留缓存、就近优先加载
gd_large_test.go: 大 diff 占位→Show→按需读取→渲染 端到端回归（真实仓库）；失败重试、过大无动作、刷新保留/失效语义、不预读颜色
git_workbench_actions.go: Git 视图操作条（Stage/Unstage All、Fetch/Pull/Push 图标、始终可见 Split/Unified、Git actions 下拉）；仓库 Refresh 唯一入口在 Git 侧栏，按 Diff 实际宽度折叠批量操作；首位是文件列表开关（gitFilePaneToggle）
git_sidebar_tree.go: 左侧 Git 引用导航，Worktrees/Branches/Remotes/Tags/Stashes 共用 sidebarTreeRow 与 sidebarSectionHeader；分区悬停 "+" 与右键菜单承载创建动作，无 "···"
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
panel_motion.go: Git/Terminal 左栏统一宽度（默认 252 DIP，可拖拽 200–400）和折叠动画，不再依路由改变宽度
repo_bar.go: 仅保留变更文件点击与 toggleMenu popover 辅助函数；仓库菜单见 git_sidebar_tree 与 sidebar_switchers
branch_menu.go: 当前分支优先的搜索过滤与受保护的切换/创建入口，弹层由 sidebar_switchers 持有
sidebar_tree.go: 侧栏层级唯一对齐网格（chevron 槽·图标·标题、首层对齐分区标题）、导向线、统一行构建器 sidebarTreeRow（Selected 选中脸、键盘光标同脸 SidebarSelect、Action 悬停行内图标）、可折叠分组头 sidebarSectionHeader 与行高令牌 treeRowHeight；两个构建器返回 (行, clicked)（clicked 兼收鼠标点击与键盘激活），经 Spec.Nav 注册进 listNav；Git 引用、Files 树与 Hosts 列表共用
list_nav.go: 侧栏列表/树与 Sessions 列表唯一的键盘模型 listNav——真焦点即光标（FocusGroup 单 Tab 停靠点，方向键由它认领故不回绕），j/k·↓/↑ 移动、h/l·←/→ 折叠展开或进出层级、o 打开、gg/G·Home/End 首尾、Ctrl-d/u 跳十行，Enter/Space 本就是行点击；进页聚焦开场行但光标不亮——键盘用过它才亮（Tab 进入自带亮；离开列表/重进页面复暗；点击不亮），会改东西的行键（Files 的 a/A/r/d/y，Sessions 的 d/u）等光标亮了才响应；Git 引用树不抢进页焦点（窗口级 j/k 归 Diff）；对话框关闭后 navTrackDialogs 把焦点还给光标行（MyGo 对 AutoFocus 对话框的焦点恢复缺口，应用侧补）；Hosts 光标移动即选中；Sessions 的项目卡头与会话行自行注册（行内预览当作该行的折叠），stay 让删除/放回后光标落到邻行（优先下一行，其次上一行，不落在项目头）；虚拟化 ui.List（Changes）不走这里
list_nav_test.go: Files/Git 引用/Hosts 的真键盘回归（Sessions 见 history_nav_test.go）——移动与不回绕、gg/G/Ctrl-d/u、h/l/o 层级、单 Tab 停靠点与记忆、进页聚焦（含列表晚到）、对话框后焦点回到光标行、Files 行键 a/A/r/d/y、Hosts 选中跟随与机器行不属于末组、光标脸（键盘用过才亮、Tab 进入即亮、离开/重进复暗、点击不亮）
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
dialogs.go: Git 危险动作确认绑定打开时 repository root；Merge 策略 Modal 失效时关闭；Sessions 的 Delete Forever / Empty Trash 确认；Confirm/Cancel 保持安全默认
branch_menu.go: 本地分支 Popover 可搜索/独立滚动/当前分支优先，Switch/More 层级；Merge FF/no-FF 策略和 dirty Branch Switch 的真实确认分发
git_workbench_actions_test.go: Git 本地操作条、Modal、Draft Guard、Merge/Revert Confirm 的 headless 回归
ds_components.go: 通用卡片、pageTopInset（标题栏到页面首行的唯一顶部留白令牌）、settingsGroup/formRow 分组行（控件先于标签入树，保证焦点/朗读顺序）、groupLabel、iconTile 彩色图标块、compactButton/compactPrimaryButton 紧凑按钮层级（密集工具栏/行内操作的唯一字号来源，业务代码不再对 Button 内联 FontSize）、工作区 Chat/Diff 轻量空状态组件（窄列时操作换行到消息下方，消息不被挤成单字列）
unified_page_test.go: UI-007/UI-008 Settings/Git/Chat 跨页面基础回归与尺寸矩阵（headless；含窄窗口 clean 状态可读性）
pane_header_visuals_test.go: 窗格头部视觉解析、Agent 图标与名称解析、路径格式化、标签已写出目录/单卡不复述目录的去重及最大化 Tab 窗格计数单元测试
chat_hooks.go: Shell 钩子层：挂载原生 apphook 进程内 IPC 服务与 spool 兼容层，直接接收并更新 Agent 状态与会话绑定，驱动 blocked 交互检测
chat_hooks_test.go: 钩子事件解析、提示词检测、投影等价去重与原生 apphook 状态/会话更新回归

[PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
