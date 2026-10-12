package agenthooks

/**
 * [INPUT]: 内嵌的通用 Agent hook 脚本（v6，v5 基础上补齐会话中继的可选字段）
 * [OUTPUT]: 对外提供 hookScriptTemplate、hookScriptMarker、hookScriptVersion
 * [POS]: agenthooks 的脚本供给源；install 原子写入 ~/.shardlane/hooks/ 下的 provider 副本
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

// hookScriptVersion is the version this build provisions; the marker line
// inside the template must match.
const hookScriptVersion = 6

// hookScriptMarker identifies files installed by Shardlane; foreign files
// at our install paths are never overwritten.
const hookScriptMarker = "installed by Shardlane"

// hookScriptTemplate is the universal direct-connect hook script shared by
// every provider copy. It reads stdin hook payloads, normalizes the event
// into a status, then (1) reports to the Shardlane apphook socket when
// HERDR_PANE_ID/TMUX_PANE identify a managed pane, (2) reports session
// identity to Herdr's pane.report_agent_session, and (3) emits an OSC 1337
// status escape for inline sniffing. It never logs content and fails open.
const hookScriptTemplate = `#!/bin/sh
# installed by Shardlane
# managed by Shardlane; reinstalling or updating the hook overwrites this file.
# SHARDLANE_HOOK_VERSION=6

set -eu

AGENT="${1:-unknown}"
EVENT_ARG="${2:-}"
PANE_ID="${HERDR_PANE_ID:-${TMUX_PANE:-}}"
if [ -z "$PANE_ID" ]; then
    exit 0
fi

HOOK_INPUT=""
if [ ! -t 0 ]; then
        HOOK_INPUT="$(cat 2>/dev/null || true)"
fi

python3 - <<'PY' "$AGENT" "$PANE_ID" "$HOOK_INPUT" "$EVENT_ARG"
import sys, os, json, socket, time

agent = sys.argv[1] if len(sys.argv) > 1 else "unknown"
pane_id = sys.argv[2] if len(sys.argv) > 2 else ""
raw_input = sys.argv[3] if len(sys.argv) > 3 else ""
event_arg = sys.argv[4] if len(sys.argv) > 4 else ""

hook_data = {}
if raw_input.strip():
    try:
        hook_data = json.loads(raw_input)
    except Exception:
        pass

event_name = (
    hook_data.get("hook_event_name")
    or hook_data.get("event")
    or hook_data.get("type")
    or event_arg
    or ""
)

# 常见 prompt 载荷字段（Claude UserPromptSubmit.prompt / agy PreInvocation 等）：
# 作为语义文本随报告下发，journal 侧才可能有正文。
prompt_text = (
    hook_data.get("prompt")
    or hook_data.get("user_query")
    or hook_data.get("message")
    or hook_data.get("text")
    or ""
)

session_id = (
    hook_data.get("session_id")
    or hook_data.get("conversationId")
    or hook_data.get("conversation_id")
)
if not session_id:
    # agy 1.2.7 的 PreInvocation 载荷实测不带会话 id（herdr 官方 hook
    # 因此静默失败）。从祖先进程参数恢复：链路是 agy -> hook.sh ->
    # python，直接父进程是 hook.sh 自身，必须向上走到 agy 那一级
    # 才能看到 --conversation <uuid>（2026-09-19 实测修复）。
    try:
        import re as _re
        import subprocess as _subprocess
        _pid = str(os.getppid())
        for _ in range(4):
            _args = _subprocess.run(
                ["ps", "-p", _pid, "-o", "args="],
                capture_output=True, text=True, timeout=2,
            ).stdout
            _match = _re.search(r"--conversation[= ]([0-9a-fA-F-]{8,})", _args)
            if _match:
                session_id = _match.group(1)
                break
            _pid = _subprocess.run(
                ["ps", "-p", _pid, "-o", "ppid="],
                capture_output=True, text=True, timeout=2,
            ).stdout.strip()
            if not _pid or _pid == "0":
                break
    except Exception:
        pass
status = "working"

if event_name in ("SessionStart", "session_start"):
    status = "idle"
elif event_name in ("PreInvocation", "pre_invocation"):
    status = "idle"
elif event_name in ("UserPromptSubmit", "user_prompt_submit", "PreToolUse", "pre_tool_use"):
    status = "working"
elif event_name in ("PermissionRequest", "permission_request", "AskUserQuestion", "ask_user_question"):
    status = "blocked"
elif event_name in ("PostToolUse", "post_tool_use"):
    status = "working"
elif event_name in ("Stop", "stop", "session_idle"):
    status = "idle"
elif event_name in ("SessionEnd", "session_end", "session_shutdown"):
    status = "released"

# 1. Report to Shardlane local Unix socket if available (managed panes only)
if not (os.environ.get("HERDR_PANE_ID") or os.environ.get("TMUX_PANE")):
    sys.exit(0)

sock_path = os.environ.get("SHARDLANE_SOCKET_PATH") or os.path.expanduser("~/.shardlane/run/agent-status.sock")
if os.path.exists(sock_path):
    try:
        payload = {
            "agent": agent,
            "status": status,
            "pane_id": pane_id,
            "tmux_pane": os.environ.get("TMUX_PANE"),
            "herdr_pane": os.environ.get("HERDR_PANE_ID"),
            # 缺失时发 null（不是空串）：空串曾绕过 ingest 的会话锚定
            # 校验，把全部会话写进同一个 journal 文件（审计 adv2-F1）。
            "session_id": session_id or None,
            "cwd": os.getcwd(),
            "event": event_name,
            "text": prompt_text,
        }
        with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as s:
            s.settimeout(0.5)
            s.connect(sock_path)
            s.sendall((json.dumps(payload) + "\n").encode("utf-8"))
    except Exception:
        pass

# 2. 会话身份接管（v3；v6 补齐字段）：载荷/父进程参数拿到会话 id 后，直接向
# Herdr socket 报 pane.report_agent_session（source=shardlane，客户端上报
# 语义）。Herdr 有了 typed session identity，Chat 入口链路即通；
# herdr 官方 hook 组只在存在时兜底，不再是前置依赖。v6 起可选字段与
# herdr 官方脚本同形状（transcript 路径 + SessionStart 来源），身份接管
# 完全等价，官方 hook 组可以安全地只做降级备用。
if session_id and os.environ.get("HERDR_ENV") == "1" and os.environ.get("HERDR_PANE_ID") and os.environ.get("HERDR_SOCKET_PATH"):
    try:
        _seq = time.time_ns()
        _params = {
            "pane_id": os.environ["HERDR_PANE_ID"],
            "source": "shardlane",
            "agent": agent,
            "seq": _seq,
            "agent_session_id": session_id,
        }
        _transcript = (
            hook_data.get("transcript_path")
            or hook_data.get("transcriptPath")
        )
        if isinstance(_transcript, str) and _transcript:
            _params["agent_session_path"] = _transcript
        if event_name in ("SessionStart", "session_start"):
            _start_source = hook_data.get("source")
            if isinstance(_start_source, str) and _start_source:
                _params["session_start_source"] = _start_source
        _req = json.dumps({
            "id": f"shardlane:{agent}:{_seq}",
            "method": "pane.report_agent_session",
            "params": _params,
        })
        with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as h:
            h.settimeout(0.5)
            h.connect(os.environ["HERDR_SOCKET_PATH"])
            h.sendall((_req + "\n").encode("utf-8"))
    except Exception:
        pass

# 3. Emit OSC 1337 escape sequence to /dev/tty for remote/tmux inline sniffing
try:
    with open("/dev/tty", "w") as tty:
        tty.write(f"\x1b]1337;AgentStatus={status};agent={agent};pane={pane_id}\x07")
        tty.flush()
except Exception:
    pass
PY
`
