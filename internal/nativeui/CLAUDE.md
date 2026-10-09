# nativeui/
> L2 | 父级: /CLAUDE.md

成员清单
icons.go: 矢量图标字典，inline SVG 定义，提供常用及跨平台窗格/窗口控制图标（iconMinus/iconSquare）
proc_brands.go: 进程与工具品牌标识解析器，将前台进程与 Agent CLI 映射为矢量图标与友好名称
gorex_style.go: Gorex 风格视觉设计系统，提供渐变背景/卡片配色/shortDir/samePath 等跨平台路径处理
terminal.go: 终端画布与 Gorex 窗格卡片主渲染器，管理活动终端连接并投影 Agent/进程品牌视觉头部
breadcrumb_switcher.go: 标题栏面包屑与快速切换弹层所有者，展示项目/Tab/Pane层级并渲染品牌图标
titlebar.go: 窗口标题栏组件，红绿灯与工具区对齐、视图切换（History 内页先返回根）与 Windows 平台窗口控件支持
router.go: 单一 Router 的页面归属、精确 History 路径分类与窗口标题路由语义
navigation_semantics_test.go: UI-009 顶栏/侧栏同级返回、重复点击、深链接与浏览历史回归
panel_motion.go: 左右侧栏动画与 UI-010 中心至少 560 DIP 的响应式右栏规则
panel_motion_test.go: Reduce Motion、窄窗口右栏自动隐藏/恢复、显式开栏宽度门禁验证
right_panel.go: Workspace/History 各自独立的右栏状态，狭窄窗口显式打开的腾挪/提示行为；Git Diff/Commit 时 Changes 切换为 Review
right_panel_git.go: Git Diff/Commit 的右侧 Review Inspector，读取缓存仓库/文件/Commit 元数据，Terminal 不复用此内容
right_panel_git_test.go: Terminal Changes 与 Git Review 互斥、历史 Commit 只读状态的 headless 验证
command_center_ui.go: Command Center 路由 / Agent / Files 工具动作入口，Files 遵从与标题栏相同的宽度和上下文规则
project_tree.go: 侧边栏工作区与会话树渲染器，展示项目、Tab 及 Pane 层次与运行状态
workbench.go: 桌面工作台状态中枢，Agent 目录与运行时事件分发
page_history.go: History 左侧筛选与中心列表/详情布局，SearchField、结果计数和 Clear filters 回到同一个查询入口
history_state.go: HistoryService 查询/详情的生成代际隔离，筛选清空旧可选行及 Provider 元数据选择
history_test.go: History 页面真实 Catalog 场景和会话详情渲染回归
history_filter_test.go: History 筛选空态、结果计数、Provider 选项与旧行不可点击的 headless 验证
shell.go: 核心 Shell 宿主，协调 Router、Window、Titlebar 与各页面组件
page_settings.go: 五个 Settings section 统一 settingsPage Header/Scroll/最大内容宽度，单一设置项来源
page_providers.go: Provider Integration 设置页仅绘制内容，跟随 settingsPage 的公共头部和滚动容器
workspace.go: Terminal/Chat/Diff/Commit 唯一主表面及 Chat 的当前 Agent 身份门禁
chat_ui.go: Chat Timeline/Composer 和身份消失时的安全空态，不得泄漏旧 transcript
gd_surface.go: Git Diff、无仓库/无变更/无匹配状态，clean 时直达 All Commits 或 Terminal；工作树 Git toolbar 单一调用点
git_workbench_actions.go: Git 唯一主操作条（Stage/Unstage、Diff 布局、Refresh、Commit、More…），按中央可用宽度折叠，repo snapshot 绑定不变
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
