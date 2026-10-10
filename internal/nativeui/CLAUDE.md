# nativeui/
> L2 | 父级: /CLAUDE.md

成员清单
icons.go: 矢量图标字典，inline SVG 定义，提供常用及跨平台窗格/窗口控制图标（iconMinus/iconSquare）
proc_brands.go: 进程与工具品牌标识解析器，将前台进程与 Agent CLI 映射为矢量图标与友好名称
gorex_style.go: Gorex 风格视觉设计系统，提供渐变背景/卡片配色/shortDir/samePath 等跨平台路径处理
terminal.go: 终端画布与 Gorex 窗格卡片渲染器，Pane 标题原生拖动交换、悬停目标占位提示及确认后位置动画，保持 Herdr 终端连接权威
breadcrumb_switcher.go: 标题栏面包屑与快速切换弹层所有者，Pane 顺序与 Herdr 实际布局对齐并展示品牌图标
titlebar.go: 窗口标题栏组件，红绿灯与工具区对齐、视图切换（History 内页先返回根）与 Windows 平台窗口控件支持
router.go: 单一 Router 的页面归属、精确 History 路径分类与窗口标题路由语义
navigation_semantics_test.go: UI-009 顶栏/侧栏同级返回、重复点击、深链接与浏览历史回归
panel_motion.go: 只动画左侧导航栏；无右栏布局宿主
panel_motion_test.go: Reduce Motion 下左导航收起/恢复，以及各窗口宽度无右栏回归
right_panel.go: 旧会话设置兼容字段及 Files 缓存、根目录 identity fence、URL Preview 管理；**不绘制右栏**
right_panel_files.go: /files 左侧目录缓存/树及点击加载预览的适配器
page_files.go: Header Files 独立页面：左树/中预览、当前 Tab CWD 归属和空态
quick_panel_services.go: Agent 浮层里的 Services 内容：当前 Tab Scripts 与观测端口
right_panel_git_test.go: Diff/历史 Commit 保留左导航/中预览而不绘制旧 Review Inspector
command_center_ui.go: Command Center Files 操作直达独立 /files 页面，不受旧右栏宽度限制
project_tree.go: 侧栏 Workspace/Tab/Pane 原生拖拽入口及运行状态树，尊重 Herdr 权威顺序而非按 Agent 状态重排
reorder.go: 作用域隔离的拖放语义与目标插入索引、Herdr mutation 分发和 Pane layout 顺序投影
reorder_test.go: Drag/Drop 真正鼠标事件、插入索引、跨 Session/父级边界和投影排序回归
components.go: 共用导航树行与视觉组件，拖动时提供原生浮动预览、插入占位、动画及命中目标
workbench.go: 桌面工作台状态中枢，Agent 目录与运行时事件分发
page_history.go: History 左侧筛选与中心列表/详情布局，SearchField、结果计数和 Clear filters 回到同一个查询入口
history_state.go: HistoryService 查询/详情的生成代际隔离，筛选清空旧可选行及 Provider 元数据选择
history_test.go: History 页面真实 Catalog 场景和会话详情渲染回归
history_filter_test.go: History 筛选空态、结果计数、Provider 选项与旧行不可点击的 headless 验证
detail_fact.go: Agent 详情复用的只读字段样式，从退役 Inspector 解耦
shell.go: 核心 Shell 宿主，协调 Router/Window/Titlebar；首次 Herdr 加载独占中心展示并淡入工作区，Git sidebar 与双模式分栏展示态仅本地持有
page_settings.go: 五个 Settings section 统一 settingsPage Header/Scroll/最大内容宽度，单一设置项来源
page_providers.go: Provider Integration 设置页仅绘制内容，跟随 settingsPage 的公共头部和滚动容器
workspace.go: Terminal/Chat/Diff/Commit 唯一主表面，Changes 主内容采用上下可调 Git Review，Chat 保持 Agent 身份门禁
chat_ui.go: Chat Timeline/Composer 和身份消失时的安全空态，不得泄漏旧 transcript
gd_surface.go: 下方 Git Diff、无仓库/无变更/无匹配状态，clean 时直达 All Commits 或 Terminal；历史模式只读，工作树工具栏单一调用点
git_workbench_actions.go: Git 视图操作条（Stage/Unstage All、始终可见 Split/Unified、⋯ 更多菜单）；仓库 Refresh 唯一入口在 Git 侧栏，按 Diff 实际宽度折叠批量操作
git_sidebar_tree.go: 左侧 Git 引用导航入口、分组操作弹窗和仓库身份展示，Worktrees/Branches/Remotes/Tags/Stashes 使用同一树组件
git_ref_tree.go: Git 引用按斜线构树、目录键稳定、筛选时独立折叠/再次展开、右键菜单，不执行 Git IO
git_review_split.go: Changes 双模式主工作台，Local Changes 文件树+Diff+底部 Commit 分离按钮（AI Commit 经 Pi 只填 Subject）；All Commits 为图谱上层+只读文件树/Diff 下层
git_review_split_test.go: Git 双模式分栏、960 DIP 几何、拖拽、历史只读与底部草稿统一 Commit Modal 测试
git_tree_interaction_test.go: 所有 Git 引用组反复折叠/展开、筛选后目录折叠与恢复回归
git_file_tree_fold_test.go: Local Changes/All Commits 中重复点击相同文件目录的折叠与再展开回归
git_directory_actions.go: Staged/Unstaged 独立目录子文件解析，路径边界准确，不以目录路径批量 Git add
git_quick_ai.go: 底部 AI Commit Pi 生成器；异步 root/snapshot/草稿门禁、仅安全元数据、只填 Subject，拒绝自动提交
git_workbench_polish_test.go: Split/Unified 真实点击、目录选中与边界、唯一 Refresh、⋯ 菜单和异步 AI 草稿门禁回归
git_commit_graph.go: 历史提交真实父 SHA 的 merge lanes 纯渲染投影；无运行时 Git IO
git_commit_graph_test.go: 提交父节点拓扑及合并分支回归
git_layout_render_test.go: 仅显式启用时输出 Local Changes/All Commits 实际 MyGo 渲染截图
changes_panel.go: Changed File Tree 目录每次直接点击均切换折叠，不依赖 List.Changed；文件行保留多选及 Diff 导航
bootstrap_page.go: 启动时全屏居中 Loading 或可重试的错误页面，尚未初次完成 Herdr projection 时不挂载侧栏或顶部菜单
bootstrap_page_test.go: 初次加载/故障时隐藏交互、中心位置与就绪后切换的 headless 回归
runtime.go: 首次 Herdr 列举、异步 Projection、初次 terminal coordination 完成前的 bootstrap gate；移除第二次阻塞枚举
panel_motion.go: Git/Terminal 左栏统一 252 DIP 宽度和折叠动画，不再依路由改变宽度
repo_bar.go: 仅保留 Git 树分组弹窗、变更文件点击和菜单辅助函数，不再绘制顶部压缩 chips
branch_menu.go: 分支管理的可搜索弹窗及安全切换动作，入口由左侧 Git 树分组菜单持有
git_service_snapshot.go: 仓库绑定快照/提交历史及本地远程跟踪引用的分离异步获取与 generation root 隔离
commit_surface.go: 原生 Commit Dialog 唯一草稿，滚动正文 + 固定底部 Commit/Cancel、Select all/Clear selection
commit_ai_ui.go: Commit 内折叠的 Pi 生成、敏感路径受限 Prompt 原文预览与复制、手动候选消息编辑；默认只展示必要选项
commit_modal_layout_test.go: 960×640/1200×800 长文件与 Pi 展开后固定 CTA 几何、文件选择批量操作回归
branch_menu_filter_test.go: 搜索过滤、当前分支优先、创建入口始终可见、跨仓库过滤状态清理
commit_pi.go: Pi 一次性候选生成 UI 协调、后台取消/超时、repo/snapshot/paths identity fence，拒绝自动 git commit
page_settings_git.go: Git & AI 配置页：默认 Conventional Commit 规范、显式保存/恢复、Pi CLI 可执行路径
commit_pi_test.go, settings_git_test.go: Pi 后台时序/过期消息隔离、Git Settings 持久化及 MyGo 控件回归
commit_message_import.go: AI 建议文本纯校验与 Subject/Body 导入，已有手写消息时必须二次确认，不触发 Git 提交
commit_message_import_test.go: 消息长度/代码围栏/空主题校验、覆盖确认及本地状态销毁回归
workspace_header.go: Commit Modal 打开/取消/草稿导航防护，仍使用既有 WorkspaceSurfaceCommit 主状态
git_operations.go: 背景 mutation 编排，FF/no-FF Merge、Revert/Cherry-pick/Undo、Git Continue/Abort，经确认并在失败后刷新
git_sequencer_ui.go: Git Diff 顶栏冲突提示、操作准确的 Continue/Abort/Refresh；右侧 Inspector 共用状态和指引
git_sequencer_ui_test.go: Merge/Revert/Cherry-pick 的 headless 冲突状态、按钮安全门禁和仓库切换隔离
dialogs.go: Git 危险动作确认绑定打开时 repository root；Merge 策略 Modal 失效时关闭；Confirm/Cancel 保持安全默认
branch_menu.go: 本地分支 Popover 可搜索/独立滚动/当前分支优先，Switch/More 层级；Merge FF/no-FF 策略和 dirty Branch Switch 的真实确认分发
git_workbench_actions_test.go: Git 本地操作条、Modal、Draft Guard、Merge/Revert Confirm 的 headless 回归
ds_components.go: 通用卡片、Field、工作区 Chat/Diff 轻量空状态组件
unified_page_test.go: UI-007/UI-008 Settings/Git/Chat 跨页面基础回归与尺寸矩阵（headless）
window_actions.go: 原生窗口级行为与标题同步适配器
pane_header_visuals_test.go: 窗格头部视觉解析、Agent 图标与名称解析、以及跨平台路径格式化的单元测试

[PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
