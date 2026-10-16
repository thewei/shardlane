# agentstatus/
> L2 | 父级: /CLAUDE.md

成员清单
protocol.go: OSC 7501 Program Status Protocol（spec rev 0.4）报告体解析：`key=value` 对以 `:` 分隔、未知键忽略、同键后者胜；对级语法错只跳过该对，状态缺失/未知、id 违反路径语法、base64 不可解或解码含控制字符则整份丢弃；msg/title 为可选填充 base64 UTF-8（禁 C0/DEL/C1）；progress 0-100 之外视为缺省；提供 State/Kind 词汇与 StatusWord 到 Shardlane 状态词表（working/blocked/idle/done/error）的映射
records.go: 终端侧记录模型：每 id 一记录、报告整体替换、`clear` 无 id 清空全部/有 id 清子树、达 256 条按最近更新时间 LRU 逐出（含 root，规范原文无豁免）；进程退出与 OSC 133 A 丢 working/blocked（done/error/idle 存活）；Display 按紧急度排名（blocked>working>error>done>idle，root 同分优先、同级取新），app 从最近祖先继承
store.go: 每 pane 合并仲裁层（sync.Mutex，apphook goroutine 与终端 goroutine 并发喂入）：7501 报告永远生效并接管 pane；hook 事件仅当记录集无瞬态状态（working/blocked）或距最后 7501 报告超过 2 分钟（`oscAuthority`，防崩溃残留瞬态永久压制 hook）时生效；`released` 视同进程退出清瞬态；ApplyProgramReport/ApplyHookStatus/Display/PaneClosed 是全部公开面
protocol_test.go: 解析回归——spec 原例（terraform blocked、working:progress=50、clear）、对级规则（未知键/同键后者胜/字符集外跳对）、整份丢弃（未知状态、id 语法、坏 base64、控制字符、超限）、字段规则（progress/kind/app 的态相关与缺省语义、无填充 base64）
records_test.go: 记录回归——整体替换、clear 子树与清空、瞬态丢弃与 Reset、Display 排名（blocked 子记录压过 working root）与 app 继承、LRU 逐出（陈旧 root 被逐、现行 root 存活）
store_test.go: 仲裁回归——瞬态期内 hook 让位、存活态后 hook-only 程序接管、协议再次发言夺回、released 清瞬态、无效报告体不动状态、2 分钟新鲜度窗口翻转、PaneClosed 重置

[PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
