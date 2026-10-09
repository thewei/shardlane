# nativeui/
> L2 | 父级: /CLAUDE.md

成员清单
icons.go: 矢量图标字典，inline SVG 定义，提供常用及跨平台窗格/窗口控制图标（iconMinus/iconSquare）
proc_brands.go: 进程与工具品牌标识解析器，将前台进程与 Agent CLI 映射为矢量图标与友好名称
gorex_style.go: Gorex 风格视觉设计系统，提供渐变背景/卡片配色/shortDir/samePath 等跨平台路径处理
terminal.go: 终端画布与 Gorex 窗格卡片主渲染器，管理活动终端连接并投影 Agent/进程品牌视觉头部
breadcrumb_switcher.go: 标题栏面包屑与快速切换弹层所有者，展示项目/Tab/Pane层级并渲染品牌图标
titlebar.go: 窗口标题栏组件，红绿灯与工具区对齐、视图切换与 Windows 平台窗口控件支持
project_tree.go: 侧边栏工作区与会话树渲染器，展示项目、Tab 及 Pane 层次与运行状态
workbench.go: 桌面工作台状态中枢，Agent 目录与运行时事件分发
shell.go: 核心 Shell 宿主，协调 Router、Window、Titlebar 与各页面组件
window_actions.go: 原生窗口级行为与标题同步适配器
pane_header_visuals_test.go: 窗格头部视觉解析、Agent 图标与名称解析、以及跨平台路径格式化的单元测试

[PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
