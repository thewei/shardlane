# Computer Use MCP 运行手册

本参考只在 UI 验收选择 Computer Use 驱动时读取。

## Agent 连接声明

Computer Use 由宿主提供；仓库不注册第二个 MCP server，也不读取 API key。
在 Codex 桌面端先从 Plugins 安装/启用 Computer Use，打开 Computer Use
server/skill 开关，并按系统提示授予 Screen Recording 与 Accessibility；
某些版本会由已启用的 `node_repl` 服务承载它，而不是展示一个同名 MCP 条目。
其他 Agent 的工具清单/系统提示应声明这组契约：

```yaml
computer_use:
  transport: mcp__node_repl__js
  runtime: persistent node_repl
  package: "@oai/sky"
  target: mac
  probe: scripts/acceptance-capabilities.py mcp-snippet
```

这段 YAML 是跨 Agent 的能力声明，不是可以直接粘贴到所有宿主的通用
`.mcp.json`；宿主必须真的提供 `mcp__node_repl__js`。Codex 的自定义 MCP
server 才放在 `~/.codex/config.toml` 的 `[mcp_servers.<name>]` 表中；不要
把 bundled Computer Use 猜成一个可由 `codex mcp add` 启动的 `@oai/sky`
进程。没有该工具就报告 `computer-use:mcp=unknown`，停在后端断言，不以
shell/CGEvent 冒充。

`codex mcp list` 或 Codex TUI 的 `/mcp` 只能帮助诊断宿主，内部
`node_repl`/Computer Use server 名称和状态不是本仓库契约；只要下面的
`mcp__node_repl__js` 探针通过，就不要手改宿主内部命令路径。

运行 `scripts/acceptance-capabilities.py mcp-snippet`，将输出 JavaScript
通过宿主 MCP 执行。它只导入 `@oai/sky` 并检查导出，不产生任何 UI 动作；
结果应为 `target: "mac"` 且 `missing: []`。将结果中的导出名通过
`scripts/acceptance-capabilities.py check --mcp-export <name>`（可重复）合并
到本地清单，得到可供 Agent 选择驱动的完整报告。

## MCP 调用

Computer Use 由 Codex 的 `computer-use` MCP 服务提供，Agent 通过 `mcp__node_repl__js` 执行 JavaScript；不要从 shell 模拟 `@oai/sky`，也不要把 `osascript`/CGEvent 当作等价实现：

```js
globalThis.sky ??= (await import("@oai/sky")).sky;
let state = await sky.get_app_state({app: appPath, disableDiff: true});
nodeRepl.write(JSON.stringify({app: state.app, text: state.text}));
```

动作模板：

```js
await sky.click({app: appPath, element_index: index});
state = await sky.get_app_state({app: appPath});
nodeRepl.write(state.text);
```

`element_index` 只对最近一次 AX 状态有效。动作后状态发生变化时，重新定位索引；`perform_secondary_action` 的 action 名称必须来自 AX 输出，不能猜。`paste` 会暂存并恢复用户剪贴板，仍要避免粘贴敏感数据。需要截图时只读取 `state.screenshot.url`，不要把屏幕内容写入仓库。

## 隔离边界

准备脚本的 `manifest.env` 是控制平面事实：

| 字段 | 用途 |
| --- | --- |
| `APP_PATH` / `APP_PID` | Sky 的 App 目标和进程存活检查 |
| `HERDR_SOCKET_PATH` | Herdr CLI 查询/断言 |
| `TMUX_SOCKET_PATH` / `TMUX_TARGET` | tmux 侧真值 |
| `LAG_LOG` | bind、连接和性能边界 |
| `EVIDENCE` | 事件账本 |
| `ACCEPTANCE_TIMEOUT_SEC` | PASS/FAIL marker 等待上限 |
| `PASS_MARKER` / `FAIL_MARKER` | 通知准备脚本结束 |

所有 CLI 断言都显式携带 manifest 中的 socket；不要调用默认 Herdr/tmux socket。准备脚本只清理自己的临时前缀目录和 PID。

完成一次动作后，Agent 在准备脚本之外的普通终端中记录控制面结果（示例
只写短消息和数值，不把 pane 内容复制进账本）：

```sh
tmux -S "$TMUX_SOCKET_PATH" capture-pane -t "$TMUX_TARGET" -p | grep -q "$MARKER"
scripts/acceptance-evidence.py record "$EVIDENCE" \
  --driver computer-use --phase input --status pass --message 'marker observed'
touch "$PASS_MARKER"
```

若后端断言失败，记录 `--status fail` 后创建 `$FAIL_MARKER`；不要同时创建
两个 marker。准备脚本会回收它自己启动的进程，并在失败或 `--keep` 时保留
runtime 供排查。

Sky 的公开接口没有返回“虚拟光标/虚拟显示器”状态的字段。把它视为 App 定向的隔离输入通道，而不是可证明的 headless 显示器：若 AX 目标不明确、动作需要全局前台或目标窗口无法区分，停止当前验收并报告阻塞原因。

## 风险确认

Computer Use 的普通 App 读取、点击、输入和滚动仍属于 UI 动作；若下一步会删除数据、上传/发送敏感数据、修改账号/系统设置或代表用户向第三方提交内容，按 Computer Use skill 在动作发生前确认。测试 marker、临时项目和隔离 runtime 不应触碰用户数据。
