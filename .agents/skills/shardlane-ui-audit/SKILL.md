---
name: shardlane-ui-audit
description: Use when auditing Shardlane MyGo (next/) real-app UX/UI with a Computer Use driver — finding interface inconsistencies, ambiguities, and broken affordances; verifying findings against code/Herdr truth; fixing and re-verifying on device; or continuing the F-numbered findings ledger in docs/ui-computer-use-findings-2026-10-06.md. Covers the device-driving quirks (coordinates, chords, popups, own-pane safety), the fix conventions (single-sourced actions, tooltips, plurals, HIG ellipsis, user-language copy), and the concurrent-batch protocol. Not for backend-only acceptance (use ui-acceptance-testing) or mobile (use shardlane-mobile-development).
---

# Shardlane UI 审计与修复（CUA 实机驱动）

一条「实机巡检 → 代码验证 → 修复 → 重建 → 实机复核 → 台账回写」的完整循环。
它只产出用户可见行为的结论；运行时真值仍以 Herdr CLI / 日志 / 代码为准，截图永不作为唯一证据。

## 先读

1. `AGENTS.md` + `CLAUDE.md`；
2. `docs/ui-computer-use-findings-2026-10-06.md` 的**最后两轮**——台账是滚动契约，开工前必须对账既有编号与已修项，新编号从最大号续；
3. `next/CLAUDE.md`（模块地图与所有权）；
4. 需要驱动细节时 `.agents/skills/ui-acceptance-testing/SKILL.md`。

## 硬边界

- 只测 `next/` MyGo 客户端；Rust `crates/` 是冻结的参考实现，不修不测。
- 终端 pane 常常**就是审计会话自己的 stdin**：永不点击 pane 头部的 Close Pane、永不向终端 surface 发裸文本/裸 Return。导航只走侧栏、标题栏、菜单栏、修饰键 chord。
- 破坏性 UI（Stage/Unstage/Commit/Discard/Delete/Confirm 的 Confirm 侧）在用户真实仓库上一律不点；用代码读 + 既有测试钉住，或 scratch 仓库。
- 不 kill 用户实例；重启走「替换二进制 → graceful quit → open」。

## 准备（一次性）

```sh
git status --porcelain            # 识别并行批次的工作；有 UU/DU 先停下读「并发协议」
cd next && GOTOOLCHAIN=go1.27.1 go build ./...   # 树必须先可编译
md5 -q /Users/wilson/Applications/Shardlane.app/Contents/MacOS/Shardlane \
  next/build/darwin-arm64/Shardlane.app/Contents/MacOS/Shardlane   # 运行实例是否=当前代码
```

CUA 绑定：`cua.getApp("Shardlane")`（bundle `com.whstudio.shardlane.next`）。首次绑定返回完整 AX 树。

## 实机驱动坑清单（2026-10-06 实测，违反即浪费一轮）

- **坐标空间**：`click([x,y])` 收**原始像素**；截图标注的 displayed 坐标 ×1.22 才是真实坐标。搞错的表现：光标落在别处、点击无效果。
- **窗口底边 ~50px 是点击禁区**（session selector、Settings 齿轮在此）：向内偏移 y-20 再点。
- **纯文本/裸键不落**：`typeText` 对 MyGo 输入框不可靠；用 `setValue(elementIndex, …)`。修饰键 chord（⌘K、⌘1、⇧⌘N）可靠。
- **弹层菜单/对话框是独立 NSWindow**：Go 菜单、右键菜单、会话切换器弹层在窗口级截图和 AX 里**不可见**。验证它们只能：AX 抓 menu bar 的 menu item（Go 菜单可行）、或代码审计。不要重复浪费截图。
- **AX 树在 diff 视图下洪泛**（整文件内容逐行进 AX）：Changes/diff 页面优先用截图定位，只在需要元素索引时抓 AX，且用 diff 模式。
- **元素索引即用即取**：任何动作后重新抓 AX 再取 index，跨调用复用必错。
- Esc 关 ⌘K/⌘F 可靠；对话框家族 Esc/Cancel 默认聚焦（F69 契约）可依赖。
- 窗口标题会随路由变化（F104），AX window 行即路由真值。

## 巡检清单（每表面过一遍）

按序走查，每表面记录候选问题再继续：

1. Workspace：Terminal/Changes 双表面切换、面包屑、pane 头按钮、⌘B 侧栏、右面板三标签（Changes/Files/Services）；
2. History：列表 → 详情（键盘 Down 可开）→ By Project；注意标题/描述重复、截断无 tooltip、空卡片；
3. Status Center（badge）/ New Task（⇧⌘N）/ Chat / Settings 五个 section / Providers / Runtime / Diagnostics；
4. ⌘K 与 ⌘⇧P（同一弹层）：空查询、过滤、结果行 AX 角色；
5. Go 菜单逐项（AX 点 menu bar）；
6. 终端查找条 ⌘F：空查询计数、流式窗格搜索；
7. 复数/文案批（grep 全量扫，见下）。

问题分类学（对着找）：**不一致**（同一动作两个名字/两个入口/两套行为）、**歧义**（同名不可分辨、状态不知道属于谁）、**断路**（按钮无处可去、计数被截断、tooltip 缺失、死路由、只读死胡同）、**内部泄漏**（operation id、slug、RPC 错误串直接给用户）、**惯例违例**（HIG "…"、复数、单复数动词、Enter 语义）。

## 验证方法（候选 → 定案）

```sh
# 文案/字符串定位
rg -n "疑似文案" next/internal/nativeui/*.go | grep -v _test
# 单复数批
rg -n 'Sprintf\("[^"]*%d [a-z]+' next/internal/nativeui/*.go | grep -v _test
# tooltip 缺失批：SingleLine() 截断文本行是否有 .Tooltip
rg -n "SingleLine\(\)" next/internal/nativeui/*.go | grep -v _test
# 对话框打开项的 "…" 惯例
rg -n 'openTextDialog|openConfirm' next/internal/nativeui/dialogs.go
```

- 每个 finding 必须有**代码证据**（文件:行）或**后端真值**（`herdr pane read`、`~/Library/Logs/Shardlane/shardlane.log`），二者至少其一；纯截图观察标「待验证」。
- 数据不可得 ≠ bug：先查接口是否透传（例：Services 端口行无进程名是 ObserveForRoot 只返回 []uint16，属 API 扩展项，不是渲染 bug）。
- 假问题同样记录「已排查排除」，防止下轮重复调查。

## 修复规范（重建前必须全部满足）

- **单一来源**：同一动作家族只能有一个实现（关闭 Tab 走 close-tab 确认通道；入口去重走别名路由，不复制页面）。
- **文案说人话**：不出现 operation id / verbatim / slug / RPC 错误原文；错误给「发生了什么 + 下一步」。
- **可恢复性**：任何截断文本必须有 Tooltip（全路径/全名）；计数优先于名称存活。
- **复数**：用 `pluralS(n)`；动词单复数（"1 needs attention"）单独处理。
- **HIG**：打开对话框的菜单项/按钮加 "…"；确认框 Confirm 永不默认聚焦（F69）。
- **新文件加 L3 头部**（[INPUT]/[OUTPUT]/[POS]/[PROTOCOL]），结构性变更回写 `next/CLAUDE.md`。
- **测试钉住**：每个行为修复带回归测试（改字符串的测试同步更新到新契约）；测试失败先分清「测试过期」还是「代码错了」。

## 重建 → 部署 → 实机复核

```sh
cd next
GOTOOLCHAIN=go1.27.1 go test ./...        # 全绿才继续；预先存在的失败先在干净 HEAD 复现定性
GOTOOLCHAIN=go1.27.1 go tool mygo build
cp build/darwin-arm64/Shardlane.app/Contents/MacOS/Shardlane \
   /Users/wilson/Applications/Shardlane.app/Contents/MacOS/Shardlane
osascript -e 'quit app "Shardlane"'       # 若 -128 悬挂（F133），kill -TERM 后 kill -9 回收
open /Users/wilson/Applications/Shardlane.app
```

重启安全：会话/pane/PTY 全在 Herdr 侧，审计会话自身无损。复核用「一个修复一条设备证据」收口（AX 抓取或截图），写进台账处置列。

## 台账纪律

- 编号续最大号，一行一 item，列固定：# / 严重度 / 界面 / 问题（现象+证据）/ 处置。
- 处置动词：✅ 已修（标注归属批次）、记录、挂起（写明阻塞）、已排查排除。
- 两个批次并行时（见下），台账合并对账，编号冲突显式注明。

## 并发协议（2026-10-06 血泪条）

1. 开工 `git status` 识别他人改动；进行中每改一个文件**先重读现场**（对方可能刚写入）。
2. 检测到对方正在写你手头的文件：立即停编辑，改为只记录，或换文件。
3. **永不对来历不明的冲突做 stash pop / merge**。本次事故：8/26 在 master 打的 `sidebar-wip-during-audit-commit` stash 被弹回到 10/6 的 rewrite/mygo，产生 11 个 UU/DU——stash 侧引用着已被删除并明令禁 reintroduce 的 workspace_management 机制。定性口诀：**查 `.git/MERGE_HEAD`（无 merge）+ `git stash list` 日期 + 冲突双方内容新旧**；处置口诀：Rust 参考侧永远 `checkout --ours`，DU 上被禁机制一律 `git rm` 接受删除，stash 条目保留作恢复阀、是否 drop 由用户定。

## 完成门禁

`go test ./...` 全绿（预先存在失败须已在干净 HEAD 定性并记录）→ `go tool mygo build` → 设备复核每项修复 → 台账回写 → L2/L3 回环检查 → `git diff --check` 干净（不含他人未完成批次）。
