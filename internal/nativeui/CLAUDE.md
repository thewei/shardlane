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
shell.go: 核心 Shell 宿主，协调 Router、Window、Titlebar 与各页面组件；主布局只有左栏/中央内容
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
