# Acceptance evidence schema

`evidence.jsonl` 是一次验收运行的 append-only 控制平面账本。每行是一个独立 JSON object：

```json
{
  "schema": 1,
  "ts": "2026-09-02T00:00:00.000+00:00",
  "driver": "computer-use",
  "phase": "bind",
  "status": "pass",
  "message": "bind default: app alive and bind.ok observed",
  "metrics": {"pane_count": 1}
}
```

必填字段：

- `schema`: 当前值为 `1`；
- `ts`: UTC ISO-8601 时间；
- `driver`: `computer-use` 或 `native`；
- `phase`: 短小阶段名；
- `status`: `pass | fail | skip | info`；
- `message`: 单行、最多 240 字符的控制面描述。

`metrics` 只允许短的 `[A-Za-z][A-Za-z0-9_.-]*` key（最多 48 字符）和数字/短字符串。不得写终端输出、用户输入、Prompt、环境变量全集、密钥或整张截图。`acceptance-evidence.py summary --require-phase <phase>` 会校验 schema/driver/status/phase/message，并要求该阶段至少有一个 `pass` 事件；invalid 行或缺少必需阶段即非零退出。
