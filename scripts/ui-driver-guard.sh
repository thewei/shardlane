#!/usr/bin/env bash
# -----------------------------------------------------------------------------
# [INPUT]: SHARDLANE_UI_DRIVER, SHARDLANE_ALLOW_GLOBAL_INPUT, and
#          SHARDLANE_ALLOW_GLOBAL_CAPTURE environment variables; the caller's
#          shell functions for reporting failures.
# [OUTPUT]: require_native_ui_driver / require_computer_use_ui_driver /
#           require_global_capture guards
#           that make UI input/capture boundaries explicit before a smoke starts.
# [POS]: shared safety seam for scripts that can drive a macOS UI; it does not
#        inject events, launch apps, or own an acceptance assertion.
# [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
# -----------------------------------------------------------------------------

# Computer Use is the safe default. It is driven by the Agent through the
# `mcp__node_repl__js` MCP tool and @oai/sky; shell scripts must not pretend
# that they can invoke that channel themselves.
ui_driver_name() {
  case "${SHARDLANE_UI_DRIVER:-computer-use}" in
    computer-use|computer_use|cu) echo "computer-use" ;;
    native|cg-event|cg_event) echo "native" ;;
    *) echo "unknown" ;;
  esac
}

require_native_ui_driver() {
  local driver
  driver="$(ui_driver_name)"
  if [[ "$driver" != "native" || "${SHARDLANE_ALLOW_GLOBAL_INPUT:-0}" != "1" ]]; then
    cat >&2 <<'EOF'
FAIL safety: this path posts global macOS mouse/keyboard events.
Use the Computer Use MCP path first (mcp__node_repl__js + @oai/sky).
If a real-device pacing/trackpad measurement is required, opt in explicitly:
  SHARDLANE_UI_DRIVER=native SHARDLANE_ALLOW_GLOBAL_INPUT=1 <script> ...
The native path may move the user's pointer, steal focus, and consume keys.
EOF
    return 64
  fi
  export SHARDLANE_UI_DRIVER=native SHARDLANE_ALLOW_GLOBAL_INPUT=1
}

require_global_capture() {
  local driver
  driver="$(ui_driver_name)"
  if [[ "$driver" != "native" || "${SHARDLANE_ALLOW_GLOBAL_CAPTURE:-0}" != "1" ]]; then
    cat >&2 <<'EOF'
FAIL safety: this path records pixels from the shared macOS display.
Set SHARDLANE_ALLOW_GLOBAL_CAPTURE=1 only for an explicitly-authorized
real-device screenshot/video measurement; Computer Use state.screenshot is
App-targeted and does not need this flag.
EOF
    return 64
  fi
  export SHARDLANE_ALLOW_GLOBAL_CAPTURE=1
}

require_computer_use_ui_driver() {
  local driver
  driver="$(ui_driver_name)"
  if [[ "$driver" != "computer-use" ]]; then
    echo "FAIL safety: expected SHARDLANE_UI_DRIVER=computer-use, got $driver" >&2
    return 64
  fi
  export SHARDLANE_UI_DRIVER=computer-use
}
