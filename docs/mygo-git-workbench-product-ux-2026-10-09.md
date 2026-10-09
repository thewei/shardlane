# Shardlane Git Workbench · Fork / lazygit 交互统一与功能规划

日期：2026-10-09  
范围：MyGo Native UI + 唯一 `internal/gitworkbench.Runner`；**Terminal First**，不集成第二套 lazygit 进程或 Git 应用。  
阶段：第五至七轮已完成基本 Git 工作流、Merge/Cherry-pick/冲突恢复、响应式 Toolbar 和 AI Prompt 预览；第八轮收敛重复按钮和分支菜单，支持 Git & AI 可编辑提交规范与一次性 Pi CLI 无工具消息生成。**第九轮将 Commit 主操作固定在弹窗底部、Pi 区域分级披露、支持文件全选/清空，分支 Popover 加本地搜索和受限滚动列表（headless 960/1200 DIP 验证）**。真正 macOS GUI/Provider 授权实测及高级三方冲突编辑器仍待做。  
依赖：[客户端架构](client-product-architecture.md)、[统一 UI/UX 计划](mygo-native-unified-ui-ux-plan-2026-10-09.md)。**本文不是架构真值，只规定 Git 产品设计与执行。**

## 1. 方向：一处 Git Workbench，三个责任区

- **左侧 Repo Navigator**：Local Changes / All Commits（继续保留已有单导航）；Local Changes 下 Staged / Unstaged 文件树；左侧顶部 Branch / Tags / Stashes / Worktrees 的紧凑锚定菜单。操作关联到**当前 Tab 的 repo root**，不与全局 Workspace、History 左侧栏串味。
- **中心 Diff Review**：继续使用 Godiff 模式的单/双栏 Diff、行号、文件卡、hunk/line stage、Viewed 和 Find；纯展示 read-only Commit Diff。中心是 Terminal 之外的唯一 Git 工作表面，选择 Commit 历史时不能悄悄 stage；普通用户操作仍能一步回到 Terminal。
- **右侧 Review Inspector（本轮已接入）**：左侧已有变更树时，Workspace 的 Changes 选项在 Git Diff/Commit 模式显示为 Review；右栏从重复文件树切为当前仓库/上游/选中文件的只读事实及少量关联操作，选中历史提交时显示 Commit Hash/Author/Date/Revert。Terminal 模式恢复原 Changes 工具。由现有 gitService 快照直接投影，渲染不执行 Git IO。
- **临时编辑/确认层**：Commit 使用原生 modal，复用现有 `WorkspaceSurfaceCommit` 唯一草稿和 stale-state preflight；危险操作使用原生确认对话框；只读信息用 Popover / Context Menu。弹窗不得新建 Router/Runtime、不得自动执行 Git 子命令、不得把真实 Git 输出接入日志。

UI 规范：和 Workspace / History / Settings 复用 `Spacing / Typography / Radius / DesignTokens`；主体背景低对比，强调色只用于主动作与有效状态；`Git Actions` 为次要菜单，不能在 960 DIP 宽的主栏里塞入全部高级按钮。禁用与悬停应解释原因，超长分支和路径需完整 Tooltip。目标尺寸测试：960×640 / 1280×800 / 1512×982，另需真实 Mac 截图和 VoiceOver。分组标题、字段、错误反馈与其它页面同一语义。

## 2. 成熟产品参考与取舍

| 来源 | 经过验证的成熟行为 | Shardlane 采用 | 不直接照搬 |
|---|---|---|---|
| [Fork 官网](https://fork.dev/) | Working Directory + Commit List + Side-by-side Diff；stage/unstage、amend、Branch/Tag/Stash、Merge/Rebase、Revert、Cherry-pick、Interactive Rebase、冲突助手 | 左文件树/中 Diff；原生轻量弹层；分支动作；历史 Commit 行菜单与 Revert | 不额外引入第二 Git 窗口/侧栏或独立进程 |
| [Fork Mac Release Notes](https://fork.dev/releasenotes) | 2026-09-04 发布 Cursor AI commit messages，另有 Commit 详情弹层、导航快捷键、cherry-pick/revert 冲突预览和 Git bisect | 将真正的 AI 生成消息明确列入下一阶段，重视批准前预览与独立 Agent 权限 | 不假设当前 MyGo/Herdr 已具备 Fork/Cursor 的独立生成接口或冲突预测引擎 |
| [lazygit 用户指南](https://lazygit.dev/docs/guide/) | Files / Branches / Commits / Stash 视图的键盘闭环；hunk/line stage；撤销快捷路径 | 把常用动作贴近选中对象；所有常用操作有鼠标入口及后续键盘快捷键 | 不覆盖 macOS 的 Cmd 热键，也不机械复制 Vim 键位 |
| [lazygit Features](https://lazygit.dev/features/) | commit graph、交互 rebase、cherry-pick、reflog 驱动 Undo/Redo | 写入分阶段成熟功能清单 | 不将任意 Git Reflog 操作包装成“总能撤销” |

## 3. 功能能力矩阵（代码真值）

| 功能 | 当前提供 | 入口与安全措施 | 后续工作 |
|---|---|---|---|
| Workspace changes + Diff | 已有 | 左侧 Stage/Unstage、中心 Godiff | hunk/line 快捷键真实设备验收 |
| Right Review Inspector | **本轮新增（headless）** | 左 Stage/Unstaged Tree 对应右侧仓库/选中文件/历史 Commit 元信息；Terminal 保留 Changes | 实机检查窗口过渡和键盘焦点 |
| Stage All / Unstage All | **第八轮完成操作层级统一** | 中心 Toolbar 常用批量暂存、Diff 布局、Refresh、Commit，低频 Fetch/Pull/Push/Stash/Undo/历史/Git Settings 收入 More…；窄窗 Stage/Unstage 也进入 More…；底部只显示汇总，不再重复 Commit | 实机测宽屏/窄窗/菜单键盘操作 |
| Commit / Amend | **第九轮固定操作栏** | 原生 Modal 主体区独立滚动，Cancel / Commit 和编辑草稿的 Keep/Discard 固定在底部；显示已选数、Select all / Clear selection；不改变 Commit Preflight 事务，长路径完整 Tooltip；Pi 生成中禁止提交 | headless 960×640 / 1200×800 主操作区位置固定；真实设备焦点/IME 待验 |
| AI Commit Message | **第九轮减少默认表单密度** | Commit 内 AI 辅助为折叠区：展开只显示 Generate with Pi / Include excerpt / Review prompt / Paste suggestion；Prompt 文本和手动输入框均需再次主动展开，Pi 生成返回后才自动显示候选消息；运行中禁止 Commit；根目录、快照、选择集合及 excerpt/规则偏好变动后的旧建议拒绝 | Pi Auth/模型沿用本机安装，需要用户 Pi 可用；真实 Provider 操作验证待做 |
| Branch Switch/Create/Delete | **第九轮加搜索与可滚动列表** | Branch Popover：当前分支优先，输入即过滤（大小写不敏感）、无匹配提示、240 DIP 高列表单独滚动；Create 始终显示在底部；每行仅 Switch + More…，Merge/Delete 在 More；切仓库清理旧过滤 | Remote 和高级分组仍待做 |
| Merge | **已支持两种显式策略（第六轮）** | 分支 Merge… → 策略 Modal（FF-only 默认 / Create merge commit）→ Confirm → clean check；非 FF 允许产生冲突但不会自动提交解决结果 | 非 Git 原生命令的 3-way 冲突可视编辑器仍待做 |
| Commit History / Review | 已有 | 左栏 All Commits；点击展示只读 Commit Diff | 增加 graph、日期/作者/分支过滤、全量搜索 |
| Revert | **新增冲突恢复闭环（第六轮）** | Commit 行右键 Revert → 完整 Hash + Confirm → clean check → revert；出现冲突则明确提示数量，解决并 Stage 后 Continue 或确认 Abort | 3-way visual conflict editor 后续 |
| Undo Last Commit | 已有 soft reset，已加强安全门禁 | Git Actions → 确认；只允许 clean、非 root、HEAD 未被**本地已知** remote-tracking ref 包含；改动保留在 index。该门禁不能证明远端尚未发布，只是保守的本地保护 | 已发布提交优先使用 Revert；未来 reflog history 需独立数据模型 |
| Fetch/Pull FF Only/Push | 已有 | 顶部 icon + Git Actions；Git 原生网络调用限定时限；不自动拉取 | remote/upstream 选择与 preflight |
| Stash Push/Pop/Drop | 已有 | 侧边 Stash 菜单 / Git Actions；Drop 必须确认 | Stash Apply（保留 stash）、更完整索引信息 |
| Tags / Worktrees | 已有 | 侧边弹层创建、删除和打开 | worktree 删除与现存 branch 占用预检 |
| Cherry-pick | **第六轮已实现基础闭环** | History Commit 右键 / Review Inspector → 完整 Hash → Confirm → clean check；merge commit 缺 mainline 会拒绝；冲突后 Continue / Abort | 多个提交批量 Cherry-pick / conflict preview 后续 |
| Rebase / 复杂合并 | **未实现** | Rebase 不暴露伪 Continue/Abort；遇到现有 Rebase 会提示在 Terminal 处理 | 高级三方冲突视图 / Interactive Rebase 作为单独阶段 |
| Conflict file navigator | **七轮已实现** | Git 真实 unmerged index 条目转换为去重、排序、上限 24 条的列表，右侧 Review Inspector 为可显示在 Diff 的文件提供唯一的 Review conflict 入口；总冲突数不截断 | 仅做 Git Native Diff 定位，不是自定义三方合并编辑器 |
| Reflog / interactive rebase / bisect | **未实现** | 专项迭代 | Reflog 恢复策略、rebase TODO 编辑、bisect walkthrough |
| Diff image/blame/file history | **未实现** | 后续研究 | 分层插件或内建原生视图 |

## 4. Git 页面与弹层行为地图

```text
Header: Terminal | Chat | Changes | History       Agent Activity     Inspector
Sidebar Git: [branch / tags / stash / worktrees]
             [Local Changes | All Commits]
             [Unstaged Files]
             [Staged Files] / [Commit List]
Center:      [current branch] [Stage All] [Unstage All] [Commit…] [Git Actions]
             [Unified | Split]                                  [Fetch/Pull/Push]
             ─────────────────────────────────────────────────────────
             file/commit diff cards (search, stage hunk, view/copy)
Right:       [Review | Files | Services]
             Review (worktree): repo, branch, upstream, selected file
             Review (commit): subject, SHA, author/date, Revert
             (Terminal returns to normal Changes tool)
             
Modal:       Commit changes
               selected files; Subject / Body / Amend
               Preview AI Commit Prompt… (metadata default, exclude sensitive files)
               Optional include code excerpts → Copy Reviewed AI Prompt
               Paste suggested message → Use Suggested Message
               Existing Subject/Body → Confirm Replace or Keep Current
               Cancel Commit / Keep Editing / Discard Draft / Commit
             
Branch Popover:
             Switch | Merge… (FF-only default / Merge Commit → Confirm) | Delete
Git conflict banner:
             Operation / unresolved count / Continue / Abort… / Refresh
Commit Row Context Menu:
             Cherry-pick… / Revert commit… (Confirm) | Copy full hash
```

**互斥与闭环**：

1. Git 工具栏只在当前 Repo 的 Worktree Diff 显示；查看历史 Commit Diff 不渲染可误触的 Stage/Commit/Sync。
2. Commit 使用已有唯一草稿，不在 View 内同步执行 Git，也不在输入未确认时切回 Terminal 或 History。Modal 通过 Escape/背景关闭时走同一个取消保护；确认丢弃**仅清除本地 message 草稿**，不触碰 index/worktree。
3. Git 操作开始后 UI 不可重复发起，Runner 按 repository root single-flight；成功刷新 snapshot/branch；失败尤其 Revert 冲突也刷新，不保留过期 clean 显示。
4. 所有高风险修改前重验事实，无法安全推断时拒绝。Merge 只对存在的 local branch，工作树必须 clean，拒绝 detached HEAD、自身、分叉；Revert 必须是完整 Commit SHA；Undo 不能通过 Git GUI 重写已发布 HEAD。
5. Merge/Revert/Cherry-pick 发生冲突时，使用 Git 的每个 Worktree 独立的 `MERGE_HEAD`/`REVERT_HEAD`/`CHERRY_PICK_HEAD` 标记和 `ls-files -u -z` 统计实际未解决文件数；Diff 顶部标记及 Review Inspector 只消费后台缓存快照；Continue 前重新查询且拒绝未解决文件，Abort 按已选操作类型重新校验后执行，并需要确认。**不自动覆盖含冲突的文件、不伪造三方编辑器**；文件在用户编辑器或 Terminal 中解决并 Stage。
6. 如果 Git 正处于 Rebase 或其他不支持的状态，状态条只提示走 Terminal，不展示误导性的 Continue/Abort；如果检查 Git 状态失败，界面不声称工作树 clean。
7. Snapshot 变化后所有关联动作失效或重检；Merge 策略弹窗及所有 `git-*` 确认框持有打开时的 repository root，切换 Tab/Project 后必须拒绝在新仓库执行原来的动作。Dirty Branch Switch 已统一为 `git-switch-branch` 确认分发路径（原来的 `branch-switch` 只写了确认框却没有执行分发，是实测代码审计发现的闭环缺口）。不使用已关闭 Tab 的仓库，不将 History Provider 对话和 Git 提交混为一体。

## 5. 真正接入 AI 的产品契约（尚未实施）

本轮不引入假接口、不借用 Herdr 终端偷偷运行 AI CLI。第一步为用户主动复制**经过选择、长度受限的变更摘要**到已有的 Agent（已完成）。下一阶段用下面的显式能力门禁：

1. 设置中选择本机已安装且允许的 Provider，状态显示 Ready / Not Installed / Permission Needed；无 Provider 则按钮不可用且解释原因。
2. 用户点击 **Generate Commit Message** 后先显示将发出的文件清单及字节上限，明确提醒可能包含私密信息，可排除文件；默认不发送源码或完整私密文件。
3. 借用**现有 Agent / Herdr 正式语义 API**（前置条件必须通过真实能力验证），提交独立请求，由应用显示 token/错误/取消状态；不得偷写到当前用户 Agent session 中，更不能自动发送终端键盘输入。当前 `internal/herdr/launch_transport.go` 可见的 `agent.prompt {target,text}` 是向**已存在 Agent** 发送消息，不是隔离的一次性 Commit Message Completion，不能直接当作此能力。
4. 生成返回 Subject / Body 候选；用户可编辑、重新生成和**Accept Draft**；只有批准后才写入**当前** Commit 草稿，生成结果从不自动 stage/commit。
5. 生成中 git snapshot/root/selected paths 变了必须丢弃旧结果；有日志时只记录 provider、耗时、错误类别、长度，不记录 patch/prompt/output。
6. 如果 Herdr API 无安全的独立生成端点，则产品状态明确写 **Deferred / API gap**，不能把复制 Prompt 按钮称作“AI 已接入”。

## 6. 实施顺序 / 验收

| ID | 级别 | 状态 | 验收 |
|---|---|---|---|
| GIT-UX-01 | P0 | 已实现，headless | 工作区 Diff 主工具栏 Stage/Unstage/Commit/secondary Menu；Terminal 和只读 Commit Diff 不泄漏变更动作 |
| GIT-UX-02 | P0 | 已实现，headless | Commit Native Modal；Cancel、Escape/背景、Keep Editing、Discard；InFlight 禁离开；沿用 Preflight 事务 |
| GIT-UX-03 | P0 | 已实现，scratch Git | Fast-forward Merge，清洁与分歧/当前分支/非法分支拒绝；仅确认后执行 |
| GIT-UX-04 | P0 | 已实现，scratch Git | Revert 完整 Commit SHA + Confirm；工作树污染拒绝；异常刷新、提示手动解决 |
| GIT-UX-05 | P0 | 已实现，scratch Git | Undo 对 root、dirty、已发布 HEAD 拒绝，soft reset 恢复到 index；保留确认 |
| GIT-UX-06 | P1 | 七轮增强完成（pure/headless） | AI Prompt 默认元数据；敏感路径过滤；片段 opt-in；内容 9 KiB 上限；预览后复制；手动导入 AI 建议到 Subject/Body 需显式批准 |
| GIT-UX-07 | P1 | 待做 | 真正 AI 生成消息 Provider/权限/摘要同意/失败与 stale fence |
| GIT-UX-08 | P1 | **基础闭环完成（第六轮，scratch/headless）** | Merge 双策略显式确认；Revert/Cherry-pick 冲突识别、操作类型准确的 Continue / Abort + 未解决数量；视觉三方冲突编辑器待做 |
| GIT-UX-09 | P1 | 待做 | Commit graph、筛选、搜索、stash apply、交互 rebase |
| GIT-UX-10 | P0 验收 | 待真实设备 | 独立 HOME 与 HERDR_SOCKET_PATH；完整 Git scratch repo（含有 staged/unstaged、分叉 branches、Commit modal、undo/revert）；960/1280/1512 视觉与 VoiceOver/IME |
| GIT-UX-11 | P2 | 待做 | Git 命令历史记录/Reflog 可解释恢复；更广高级操作 |
| GIT-UX-12 | P1 | 右栏上下文已实现（headless） | Git Diff/Commit 对应 Review Inspector；Terminal 仍显示 Changes；元数据来自缓存，不重复左文件导航 |
| GIT-UX-13 | P1 | 已实现（第六轮，scratch/headless） | Git sequencer 的真实操作状态、每个 Worktree 的 unmerged 数、冲突引导、Abort 确认和 Continue 安全门禁，切 repo 清空旧状态；Rebase 未支持时显式禁用 |
| GIT-UX-14 | P0 | 已实现（headless） | Merge 策略框 / Git Confirm 绑定仓库根，切仓库拒绝旧动作；修复 dirty Branch Switch 确认后无 handler 的实际闭环缺口 |
| GIT-UX-15 | P1 | 第七轮已实现（headless） | Action Bar 计算中央可用宽度，窄窗 Stage/Unstage 下沉 Git Actions，960/1280/1512 DIP 宽度矩阵覆盖；真实设备菜单/IME 仍待验 |
| GIT-UX-16 | P1 | 第七轮已实现（pure/headless） | AI Preview excludes .env/key/credentials; default no source; reviewed clipboard, optional paste response and accept with replace confirmation; independent generation API gap documented |
| GIT-UX-17 | P1 | 第七轮已实现（scratch/headless） | SequencerStatus 保留精确总数和前 24 条唯一文件路径，Inspector 可定位 Diff，无法展示的文件按钮禁用，支持 Revert/Merge/Cherry-pick |
| GIT-UX-18 | P1 | 第八轮实现（headless） | Git 中心一个主操作条；底部无重复 Commit；分支行仅 Switch/More，长分支提示全名；不再单独显示 Fetch/Pull/Push 第二排；右侧 Inspector 只展示上下文 |
| GIT-UX-19 | P1 | 第八轮实现（fake Pi CLI + headless） | Settings → Git & AI 可修改默认 Conventional Commits 规范、Pi 路径；Pi 单次 stdin 生成，禁止 tools/session/extensions/context，取消和 stale fence，候选文本需用户接受后才进入现有 Commit 事务 |
| GIT-UX-20 | P1 | 第九轮实现（headless） | Commit 滚动正文与固定 CTA 分离；长文件列表 / Pi 展开前后按钮 y 位置稳定（960×640 / 1200×800），Select all / Clear selection 和完整路径 Tooltip；Pi 选项独立披露 |
| GIT-UX-21 | P1 | 第九轮实现（headless） | Branch 可搜索、当前分支优先、无匹配空态、创建入口常驻、240 DIP 独立滚动；切仓库清理过滤条件 |

**测试事实与限制**：scratch Git integration、Native UI headless、Go/MyGo 静态门禁能证明动作边界和控件有响应，但**不能证明真实 macOS 里弹窗锚点、IME、焦点、冲突场景视觉已全部通过**。本方案不允许对正在运行的用户仓库做实验性 Merge/Revert/Reset，也不会自动 git add/commit/push 工程代码。

## 7. 设计生图 Prompt（仅作风格研究）

> High fidelity native macOS dark Git workbench inside an existing terminal-first coding-agent application, called Shardlane. Preserve its charcoal and subtle plum gradient, macOS traffic-light titlebar, tiny precise typography, sober thin borders. Left rail: branch chip, compact Tags/Stashes/Worktrees menus, Local Changes/All Commits segmented switch, unstaged and staged file trees. Center: restrained single action bar with Stage All, Unstage All, primary Commit button, secondary Git Actions; side-by-side code diff with precise +/- gutters and syntax colors. Show an anchored Merge fast-forward confirmation and one centered native Commit dialog with selection checklist, subject, optional description, amend control and a clearly secondary Copy AI Commit Prompt, Cancel and Commit. Readonly history state should show no mutation toolbar. Keep the terminal as the application's primary product identity, no second Git application chrome, no neon glass, no giant panels or fake tabs. Responsive 16:10 screenshot, clear hierarchy and realistic density.

