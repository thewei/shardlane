//! Shardlane autonomous Agent Hook registry and idempotent installer/uninstaller.
//!
//! [INPUT]: AgentId from shardlane-history, home directory path.
//! [OUTPUT]: Safe, idempotent installation, uninstallation, and status audit
//! for Agent lifecycle hooks across Claude Code, Codex, OpenCode, Pi, Command Code,
//! Cursor, and Copilot.
//! [POS]: Tier 1 high-accuracy hook management for Shardlane, enabling agent status
//! sniffing across Herdr, tmux, and remote tmux (uuyc).

use shardlane_history::AgentId;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub const CURRENT_HOOK_VERSION: u32 = 1;

/// Installation status of an Agent Hook.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HookInstallStatus {
    /// Shardlane Hook is installed and up-to-date.
    Installed,
    /// Hook is installed but from an older version.
    Outdated,
    /// Hook is not installed.
    NotInstalled,
    /// An official Herdr integration is detected, but Shardlane autonomous Hook is not installed.
    HerdrManaged,
    /// Agent does not have a supported hook mechanism.
    Unsupported,
}

/// Outcome of an install or uninstall action.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HookActionOutcome {
    Installed,
    AlreadyCurrent,
    Updated,
    Uninstalled,
    NotInstalled,
    Unsupported,
}

#[derive(Debug, thiserror::Error)]
pub enum HookError {
    #[error("IO error: {0}")]
    Io(#[from] io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Agent {0:?} does not support hooks")]
    Unsupported(AgentId),
}

/// Metadata description for an Agent Hook.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AgentHookMeta {
    pub agent: AgentId,
    pub name: &'static str,
    pub status: HookInstallStatus,
    pub hook_path: Option<PathBuf>,
    pub config_path: Option<PathBuf>,
}

// =========================================================================
// Hook Script Templates
// =========================================================================

pub const SHELL_HOOK_TEMPLATE: &str = r#"#!/bin/sh
# installed by Shardlane
# managed by Shardlane; reinstalling or updating the hook overwrites this file.
# SHARDLANE_HOOK_VERSION=1

set -eu

AGENT="${1:-unknown}"
PANE_ID="${HERDR_PANE_ID:-${TMUX_PANE:-}}"
if [ -z "$PANE_ID" ]; then
    PANE_ID="$(tty 2>/dev/null || echo '')"
fi

HOOK_INPUT=""
if [ ! -t 0 ]; then
    HOOK_INPUT="$(cat 2>/dev/null || true)"
fi

python3 - <<'PY' "$AGENT" "$PANE_ID" "$HOOK_INPUT"
import sys, os, json, socket

agent = sys.argv[1] if len(sys.argv) > 1 else "unknown"
pane_id = sys.argv[2] if len(sys.argv) > 2 else ""
raw_input = sys.argv[3] if len(sys.argv) > 3 else ""

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
    or ""
)

session_id = hook_data.get("session_id") or ""
status = "working"

if event_name in ("SessionStart", "session_start"):
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

# 1. Report to Shardlane local Unix socket if available
sock_path = os.environ.get("SHARDLANE_SOCKET_PATH") or os.path.expanduser("~/.shardlane/run/agent-status.sock")
if os.path.exists(sock_path):
    try:
        payload = {
            "agent": agent,
            "status": status,
            "pane_id": pane_id,
            "tmux_pane": os.environ.get("TMUX_PANE"),
            "herdr_pane": os.environ.get("HERDR_PANE_ID"),
            "session_id": session_id,
            "cwd": os.getcwd(),
            "event": event_name,
        }
        with socket.socket(socket.AF_UNIX, socket.SOCK_STREAM) as s:
            s.settimeout(0.5)
            s.connect(sock_path)
            s.sendall((json.dumps(payload) + "\n").encode("utf-8"))
    except Exception:
        pass

# 2. Emit OSC 1337 escape sequence to /dev/tty for remote/tmux inline sniffing
try:
    with open("/dev/tty", "w") as tty:
        tty.write(f"\x1b]1337;AgentStatus={status};agent={agent};pane={pane_id}\x07")
        tty.flush()
except Exception:
    pass
PY
"#;

pub const OPENCODE_PLUGIN_TEMPLATE: &str = r#"// installed by Shardlane
// managed by Shardlane; reinstalling or updating the hook overwrites this file.
// SHARDLANE_HOOK_VERSION=1
import net from "node:net";
import fs from "node:fs";
import path from "node:path";
import os from "node:os";

const AGENT = "opencode";

function report(state, sessionID) {
  const paneId = process.env.HERDR_PANE_ID || process.env.TMUX_PANE || "";
  const sockPath = process.env.SHARDLANE_SOCKET_PATH || path.join(os.homedir(), ".shardlane", "run", "agent-status.sock");

  const payload = {
    agent: AGENT,
    status: state,
    pane_id: paneId,
    tmux_pane: process.env.TMUX_PANE || null,
    herdr_pane: process.env.HERDR_PANE_ID || null,
    session_id: sessionID || null,
    cwd: process.cwd(),
  };

  try {
    const client = net.createConnection(sockPath, () => {
      client.write(`${JSON.stringify(payload)}\n`);
      client.destroy();
    });
    client.on("error", () => {});
    client.setTimeout(500, () => client.destroy());
  } catch (e) {}

  try {
    fs.writeFileSync("/dev/tty", `\x1b]1337;AgentStatus=${state};agent=${AGENT};pane=${paneId}\x07`);
  } catch (e) {}
}

export const ShardlaneAgentStatePlugin = async () => {
  return {
    "chat.message": async ({ sessionID }) => {
      report("working", sessionID);
    },
    event: async ({ event }) => {
      const type = event?.type;
      const sessionID = event?.properties?.sessionID;
      switch (type) {
        case "session.created":
          report("idle", sessionID);
          break;
        case "tool.execute.before":
        case "tool.execute.after":
        case "session.compacted":
          report("working", sessionID);
          break;
        case "permission.asked":
        case "question.asked":
        case "session.error":
          report("blocked", sessionID);
          break;
        case "session.idle":
          report("idle", sessionID);
          break;
        case "session.deleted":
          report("released", sessionID);
          break;
        default:
          break;
      }
    },
  };
};
"#;

pub const PI_EXTENSION_TEMPLATE: &str = r#"// installed by Shardlane
// managed by Shardlane; reinstalling or updating the hook overwrites this file.
// SHARDLANE_HOOK_VERSION=1
// @ts-nocheck
import net from "node:net";
import fs from "node:fs";
import path from "node:path";
import os from "node:os";

const AGENT = "pi";

function report(state: string, sessionID?: string) {
  const paneId = process.env.HERDR_PANE_ID || process.env.TMUX_PANE || "";
  const sockPath = process.env.SHARDLANE_SOCKET_PATH || path.join(os.homedir(), ".shardlane", "run", "agent-status.sock");

  const payload = {
    agent: AGENT,
    status: state,
    pane_id: paneId,
    tmux_pane: process.env.TMUX_PANE || null,
    herdr_pane: process.env.HERDR_PANE_ID || null,
    session_id: sessionID || null,
    cwd: process.cwd(),
  };

  try {
    const client = net.createConnection(sockPath, () => {
      client.write(`${JSON.stringify(payload)}\n`);
      client.destroy();
    });
    client.on("error", () => {});
    client.setTimeout(500, () => client.destroy());
  } catch (e) {}

  try {
    fs.writeFileSync("/dev/tty", `\x1b]1337;AgentStatus=${state};agent=${AGENT};pane=${paneId}\x07`);
  } catch (e) {}
}

export default function (api: any) {
  if (api?.on) {
    api.on("session_start", () => report("idle"));
    api.on("prompt", () => report("working"));
    api.on("tool_call", () => report("working"));
    api.on("permission", () => report("blocked"));
    api.on("idle", () => report("idle"));
    api.on("session_shutdown", () => report("released"));
  }
}
"#;

pub const COMMAND_CODE_MOD_TEMPLATE: &str = r#"// installed by Shardlane
// managed by Shardlane; reinstalling or updating the hook overwrites this file.
// SHARDLANE_HOOK_VERSION=1
// @ts-nocheck
import net from "node:net";
import fs from "node:fs";
import path from "node:path";
import os from "node:os";

const AGENT = "commandcode";

function report(state: string, sessionID?: string) {
  const paneId = process.env.HERDR_PANE_ID || process.env.TMUX_PANE || "";
  const sockPath = process.env.SHARDLANE_SOCKET_PATH || path.join(os.homedir(), ".shardlane", "run", "agent-status.sock");

  const payload = {
    agent: AGENT,
    status: state,
    pane_id: paneId,
    tmux_pane: process.env.TMUX_PANE || null,
    herdr_pane: process.env.HERDR_PANE_ID || null,
    session_id: sessionID || null,
    cwd: process.cwd(),
  };

  try {
    const client = net.createConnection(sockPath, () => {
      client.write(`${JSON.stringify(payload)}\n`);
      client.destroy();
    });
    client.on("error", () => {});
    client.setTimeout(500, () => client.destroy());
  } catch (e) {}

  try {
    fs.writeFileSync("/dev/tty", `\x1b]1337;AgentStatus=${state};agent=${AGENT};pane=${paneId}\x07`);
  } catch (e) {}
}

export default function (cmd: any) {
  if (cmd?.on) {
    cmd.on("session_start", () => report("idle"));
    cmd.on("run_start", (p?: any) => report("working", p?.sessionId));
    cmd.on("run_end", () => report("idle"));
    cmd.on("interrupted", () => report("idle"));
    cmd.on("run_error", () => report("idle"));
    cmd.on("session_shutdown", () => report("released"));
  }
}
"#;

// =========================================================================
// Helpers for Atomic File & JSON Manipulation
// =========================================================================

fn write_executable_file(path: &Path, content: &str) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension(format!("tmp-{}", std::process::id()));
    fs::write(&tmp, content)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&tmp, fs::Permissions::from_mode(0o755));
    }

    if let Err(err) = fs::rename(&tmp, path) {
        let _ = fs::remove_file(&tmp);
        return Err(err);
    }
    Ok(())
}

fn check_version_in_content(content: &str) -> HookInstallStatus {
    if content.contains(&format!("SHARDLANE_HOOK_VERSION={CURRENT_HOOK_VERSION}")) {
        HookInstallStatus::Installed
    } else if content.contains("SHARDLANE_HOOK_VERSION=") {
        HookInstallStatus::Outdated
    } else {
        HookInstallStatus::Installed
    }
}

// =========================================================================
// Agent-specific Hook Handlers
// =========================================================================

pub struct AgentHookRegistry;

impl AgentHookRegistry {
    /// Supported agents in the Hook manager.
    pub const SUPPORTED_AGENTS: [AgentId; 7] = [
        AgentId::ClaudeCode,
        AgentId::Codex,
        AgentId::Opencode,
        AgentId::Pi,
        AgentId::CommandCode,
        AgentId::Cursor,
        AgentId::Copilot,
    ];

    /// Shared directory for shardlane hook scripts: `~/.shardlane/hooks/`
    pub fn shardlane_hooks_dir(home: &Path) -> PathBuf {
        home.join(".shardlane").join("hooks")
    }

    /// Read hook status for an agent given the home directory.
    pub fn read_status(agent: AgentId, home: &Path) -> HookInstallStatus {
        match agent {
            AgentId::ClaudeCode => Self::claude_status(home),
            AgentId::Codex => Self::codex_status(home),
            AgentId::Opencode => Self::opencode_status(home),
            AgentId::Pi => Self::pi_status(home),
            AgentId::CommandCode => Self::command_code_status(home),
            AgentId::Cursor => Self::cursor_status(home),
            AgentId::Copilot => Self::copilot_status(home),
            _ => HookInstallStatus::Unsupported,
        }
    }

    /// Audit all supported agent hooks.
    pub fn audit_all(home: &Path) -> Vec<AgentHookMeta> {
        Self::SUPPORTED_AGENTS
            .iter()
            .map(|&agent| {
                let status = Self::read_status(agent, home);
                let (hook_path, config_path) = Self::paths_for(agent, home);
                AgentHookMeta {
                    agent,
                    name: agent.display_name(),
                    status,
                    hook_path,
                    config_path,
                }
            })
            .collect()
    }

    /// Install or update the hook for an agent.
    pub fn install(agent: AgentId, home: &Path) -> Result<HookActionOutcome, HookError> {
        match agent {
            AgentId::ClaudeCode => Self::install_claude(home),
            AgentId::Codex => Self::install_codex(home),
            AgentId::Opencode => Self::install_opencode(home),
            AgentId::Pi => Self::install_pi(home),
            AgentId::CommandCode => Self::install_command_code(home),
            AgentId::Cursor => Self::install_cursor(home),
            AgentId::Copilot => Self::install_copilot(home),
            _ => Err(HookError::Unsupported(agent)),
        }
    }

    /// Uninstall the hook for an agent.
    pub fn uninstall(agent: AgentId, home: &Path) -> Result<HookActionOutcome, HookError> {
        match agent {
            AgentId::ClaudeCode => Self::uninstall_claude(home),
            AgentId::Codex => Self::uninstall_codex(home),
            AgentId::Opencode => Self::uninstall_opencode(home),
            AgentId::Pi => Self::uninstall_pi(home),
            AgentId::CommandCode => Self::uninstall_command_code(home),
            AgentId::Cursor => Self::uninstall_cursor(home),
            AgentId::Copilot => Self::uninstall_copilot(home),
            _ => Err(HookError::Unsupported(agent)),
        }
    }

    pub fn paths_for(agent: AgentId, home: &Path) -> (Option<PathBuf>, Option<PathBuf>) {
        match agent {
            AgentId::ClaudeCode => (
                Some(Self::shardlane_hooks_dir(home).join("shardlane-claude-hook.sh")),
                Some(home.join(".claude").join("settings.json")),
            ),
            AgentId::Codex => (
                Some(Self::shardlane_hooks_dir(home).join("shardlane-codex-hook.sh")),
                Some(home.join(".codex").join("hooks.json")),
            ),
            AgentId::Opencode => (
                Some(
                    home.join(".config")
                        .join("opencode")
                        .join("plugins")
                        .join("shardlane-agent-state.js"),
                ),
                None,
            ),
            AgentId::Pi => (
                Some(
                    home.join(".pi")
                        .join("agent")
                        .join("extensions")
                        .join("shardlane-agent-state.ts"),
                ),
                None,
            ),
            AgentId::CommandCode => (
                Some(
                    home.join(".commandcode")
                        .join("mods")
                        .join("shardlane-agent-state.ts"),
                ),
                None,
            ),
            AgentId::Cursor => (
                Some(home.join(".cursor").join("shardlane-agent-state.sh")),
                None,
            ),
            AgentId::Copilot => (
                Some(
                    home.join(".copilot")
                        .join("hooks")
                        .join("shardlane-agent-state.sh"),
                ),
                None,
            ),
            _ => (None, None),
        }
    }

    // ---------------------------------------------------------------------
    // Claude Code
    // ---------------------------------------------------------------------
    fn claude_hook_script(home: &Path) -> PathBuf {
        Self::shardlane_hooks_dir(home).join("shardlane-claude-hook.sh")
    }

    fn claude_settings_path(home: &Path) -> PathBuf {
        home.join(".claude").join("settings.json")
    }

    fn claude_status(home: &Path) -> HookInstallStatus {
        let script = Self::claude_hook_script(home);
        let settings = Self::claude_settings_path(home);

        if script.exists() && settings.exists() {
            if let Ok(content) = fs::read_to_string(&settings) {
                if content.contains("shardlane-claude-hook.sh") {
                    if let Ok(script_content) = fs::read_to_string(&script) {
                        return check_version_in_content(&script_content);
                    }
                    return HookInstallStatus::Installed;
                }
            }
        }

        // Check if Herdr official integration exists
        let herdr_script = home
            .join(".claude")
            .join("hooks")
            .join("herdr-agent-state.sh");
        if herdr_script.exists() {
            return HookInstallStatus::HerdrManaged;
        }

        HookInstallStatus::NotInstalled
    }

    fn install_claude(home: &Path) -> Result<HookActionOutcome, HookError> {
        let script = Self::claude_hook_script(home);
        write_executable_file(&script, SHELL_HOOK_TEMPLATE)?;

        let settings_path = Self::claude_settings_path(home);
        let mut root_json: serde_json::Value = if settings_path.exists() {
            let data = fs::read_to_string(&settings_path)?;
            serde_json::from_str(&data).unwrap_or_else(|_| serde_json::json!({}))
        } else {
            serde_json::json!({})
        };

        let hooks_obj = root_json
            .as_object_mut()
            .and_then(|obj| {
                if !obj.contains_key("hooks") || !obj["hooks"].is_object() {
                    obj.insert("hooks".to_string(), serde_json::json!({}));
                }
                obj.get_mut("hooks")?.as_object_mut()
            })
            .ok_or_else(|| io::Error::other("malformed settings.json"))?;

        let events = [
            "SessionStart",
            "UserPromptSubmit",
            "PreToolUse",
            "PermissionRequest",
            "PostToolUse",
            "Stop",
            "SessionEnd",
        ];

        let script_str = script.to_string_lossy();
        let cmd_str = format!("bash '{script_str}' claude");

        let mut modified = false;
        for event in events {
            let list = hooks_obj
                .entry(event.to_string())
                .or_insert_with(|| serde_json::json!([]));
            let arr = list
                .as_array_mut()
                .ok_or_else(|| io::Error::other(format!("hooks.{event} is not an array")))?;

            // 1. Clean up any invalid/legacy flattened item
            let had_legacy = arr.iter().any(|item| {
                item.get("command")
                    .and_then(|v| v.as_str())
                    .map(|c| c.contains("shardlane-claude-hook.sh"))
                    .unwrap_or(false)
            });
            if had_legacy {
                arr.retain(|item| {
                    !item
                        .get("command")
                        .and_then(|v| v.as_str())
                        .map(|c| c.contains("shardlane-claude-hook.sh"))
                        .unwrap_or(false)
                });
                modified = true;
            }

            // 2. Check if a valid group already contains shardlane-claude-hook.sh
            let already_has = arr.iter().any(|item| {
                item.get("hooks")
                    .and_then(|h| h.as_array())
                    .map(|sub_hooks| {
                        sub_hooks.iter().any(|sh| {
                            sh.get("command")
                                .and_then(|v| v.as_str())
                                .map(|c| c.contains("shardlane-claude-hook.sh"))
                                .unwrap_or(false)
                        })
                    })
                    .unwrap_or(false)
            });

            if !already_has {
                arr.push(serde_json::json!({
                    "matcher": "*",
                    "hooks": [
                        {
                            "type": "command",
                            "command": cmd_str,
                            "timeout": 10
                        }
                    ]
                }));
                modified = true;
            }
        }

        if modified || !settings_path.exists() {
            if let Some(parent) = settings_path.parent() {
                fs::create_dir_all(parent)?;
            }
            let serialized = serde_json::to_string_pretty(&root_json)?;
            let tmp = settings_path.with_extension(format!("tmp-{}", std::process::id()));
            fs::write(&tmp, serialized)?;
            fs::rename(&tmp, &settings_path)?;
            Ok(HookActionOutcome::Installed)
        } else {
            Ok(HookActionOutcome::AlreadyCurrent)
        }
    }

    fn uninstall_claude(home: &Path) -> Result<HookActionOutcome, HookError> {
        let script = Self::claude_hook_script(home);
        if script.exists() {
            let _ = fs::remove_file(&script);
        }

        let settings_path = Self::claude_settings_path(home);
        if !settings_path.exists() {
            return Ok(HookActionOutcome::NotInstalled);
        }

        let data = fs::read_to_string(&settings_path)?;
        let mut root_json: serde_json::Value = match serde_json::from_str(&data) {
            Ok(val) => val,
            Err(_) => return Ok(HookActionOutcome::NotInstalled),
        };

        let mut modified = false;
        if let Some(hooks_obj) = root_json.get_mut("hooks").and_then(|h| h.as_object_mut()) {
            for (_event, val) in hooks_obj.iter_mut() {
                if let Some(arr) = val.as_array_mut() {
                    let before_len = arr.len();
                    arr.retain_mut(|item| {
                        let direct_match = item
                            .get("command")
                            .and_then(|v| v.as_str())
                            .map(|c| c.contains("shardlane-claude-hook.sh"))
                            .unwrap_or(false);
                        if direct_match {
                            return false;
                        }

                        if let Some(sub_hooks) =
                            item.get_mut("hooks").and_then(|h| h.as_array_mut())
                        {
                            sub_hooks.retain(|sh| {
                                !sh.get("command")
                                    .and_then(|v| v.as_str())
                                    .map(|c| c.contains("shardlane-claude-hook.sh"))
                                    .unwrap_or(false)
                            });
                            if sub_hooks.is_empty() {
                                return false;
                            }
                        }
                        true
                    });
                    if arr.len() != before_len {
                        modified = true;
                    }
                }
            }
        }

        if modified {
            let serialized = serde_json::to_string_pretty(&root_json)?;
            let tmp = settings_path.with_extension(format!("tmp-{}", std::process::id()));
            fs::write(&tmp, serialized)?;
            fs::rename(&tmp, &settings_path)?;
            Ok(HookActionOutcome::Uninstalled)
        } else {
            Ok(HookActionOutcome::NotInstalled)
        }
    }

    // ---------------------------------------------------------------------
    // Codex
    // ---------------------------------------------------------------------
    fn codex_hook_script(home: &Path) -> PathBuf {
        Self::shardlane_hooks_dir(home).join("shardlane-codex-hook.sh")
    }

    fn codex_hooks_json_path(home: &Path) -> PathBuf {
        home.join(".codex").join("hooks.json")
    }

    fn codex_status(home: &Path) -> HookInstallStatus {
        let script = Self::codex_hook_script(home);
        let hooks_json = Self::codex_hooks_json_path(home);

        if script.exists() && hooks_json.exists() {
            if let Ok(content) = fs::read_to_string(&hooks_json) {
                if content.contains("shardlane-codex-hook.sh") {
                    if let Ok(script_content) = fs::read_to_string(&script) {
                        return check_version_in_content(&script_content);
                    }
                    return HookInstallStatus::Installed;
                }
            }
        }

        let herdr_script = home.join(".codex").join("herdr-agent-state.sh");
        if herdr_script.exists() {
            return HookInstallStatus::HerdrManaged;
        }

        HookInstallStatus::NotInstalled
    }

    fn install_codex(home: &Path) -> Result<HookActionOutcome, HookError> {
        let script = Self::codex_hook_script(home);
        write_executable_file(&script, SHELL_HOOK_TEMPLATE)?;

        let hooks_path = Self::codex_hooks_json_path(home);
        let mut root_json: serde_json::Value = if hooks_path.exists() {
            let data = fs::read_to_string(&hooks_path)?;
            serde_json::from_str(&data).unwrap_or_else(|_| serde_json::json!({ "hooks": {} }))
        } else {
            serde_json::json!({ "hooks": {} })
        };

        let hooks_obj = root_json
            .as_object_mut()
            .and_then(|obj| {
                if !obj.contains_key("hooks") || !obj["hooks"].is_object() {
                    obj.insert("hooks".to_string(), serde_json::json!({}));
                }
                obj.get_mut("hooks")?.as_object_mut()
            })
            .ok_or_else(|| io::Error::other("malformed hooks.json"))?;

        let events = [
            "SessionStart",
            "UserPromptSubmit",
            "PreToolUse",
            "PermissionRequest",
            "PostToolUse",
            "Stop",
            "SessionEnd",
        ];

        let script_str = script.to_string_lossy();
        let cmd_str = format!("bash '{script_str}' codex");

        let mut modified = false;
        for event in events {
            let list = hooks_obj
                .entry(event.to_string())
                .or_insert_with(|| serde_json::json!([]));
            let arr = list
                .as_array_mut()
                .ok_or_else(|| io::Error::other(format!("hooks.{event} is not an array")))?;

            let already_has = arr.iter().any(|item| {
                item.get("command")
                    .and_then(|v| v.as_str())
                    .map(|c| c.contains("shardlane-codex-hook.sh"))
                    .unwrap_or(false)
                    || item
                        .get("hooks")
                        .and_then(|h| h.as_array())
                        .map(|sub_hooks| {
                            sub_hooks.iter().any(|sh| {
                                sh.get("command")
                                    .and_then(|v| v.as_str())
                                    .map(|c| c.contains("shardlane-codex-hook.sh"))
                                    .unwrap_or(false)
                            })
                        })
                        .unwrap_or(false)
            });

            if !already_has {
                arr.push(serde_json::json!({
                    "type": "command",
                    "command": cmd_str,
                    "timeout": 10
                }));
                modified = true;
            }
        }

        if modified || !hooks_path.exists() {
            if let Some(parent) = hooks_path.parent() {
                fs::create_dir_all(parent)?;
            }
            let serialized = serde_json::to_string_pretty(&root_json)?;
            let tmp = hooks_path.with_extension(format!("tmp-{}", std::process::id()));
            fs::write(&tmp, serialized)?;
            fs::rename(&tmp, &hooks_path)?;
            Ok(HookActionOutcome::Installed)
        } else {
            Ok(HookActionOutcome::AlreadyCurrent)
        }
    }

    fn uninstall_codex(home: &Path) -> Result<HookActionOutcome, HookError> {
        let script = Self::codex_hook_script(home);
        if script.exists() {
            let _ = fs::remove_file(&script);
        }

        let hooks_path = Self::codex_hooks_json_path(home);
        if !hooks_path.exists() {
            return Ok(HookActionOutcome::NotInstalled);
        }

        let data = fs::read_to_string(&hooks_path)?;
        let mut root_json: serde_json::Value = match serde_json::from_str(&data) {
            Ok(val) => val,
            Err(_) => return Ok(HookActionOutcome::NotInstalled),
        };

        let mut modified = false;
        if let Some(hooks_obj) = root_json.get_mut("hooks").and_then(|h| h.as_object_mut()) {
            for (_event, val) in hooks_obj.iter_mut() {
                if let Some(arr) = val.as_array_mut() {
                    let before_len = arr.len();
                    arr.retain(|item| {
                        let direct_match = item
                            .get("command")
                            .and_then(|v| v.as_str())
                            .map(|c| c.contains("shardlane-codex-hook.sh"))
                            .unwrap_or(false);
                        let sub_match = item
                            .get("hooks")
                            .and_then(|h| h.as_array())
                            .map(|sub| {
                                sub.iter().any(|sh| {
                                    sh.get("command")
                                        .and_then(|v| v.as_str())
                                        .map(|c| c.contains("shardlane-codex-hook.sh"))
                                        .unwrap_or(false)
                                })
                            })
                            .unwrap_or(false);
                        !direct_match && !sub_match
                    });
                    if arr.len() != before_len {
                        modified = true;
                    }
                }
            }
        }

        if modified {
            let serialized = serde_json::to_string_pretty(&root_json)?;
            let tmp = hooks_path.with_extension(format!("tmp-{}", std::process::id()));
            fs::write(&tmp, serialized)?;
            fs::rename(&tmp, &hooks_path)?;
            Ok(HookActionOutcome::Uninstalled)
        } else {
            Ok(HookActionOutcome::NotInstalled)
        }
    }

    // ---------------------------------------------------------------------
    // OpenCode
    // ---------------------------------------------------------------------
    fn opencode_plugin_path(home: &Path) -> PathBuf {
        home.join(".config")
            .join("opencode")
            .join("plugins")
            .join("shardlane-agent-state.js")
    }

    fn opencode_status(home: &Path) -> HookInstallStatus {
        let dest = Self::opencode_plugin_path(home);
        if dest.exists() {
            if let Ok(content) = fs::read_to_string(&dest) {
                return check_version_in_content(&content);
            }
            return HookInstallStatus::Installed;
        }

        let herdr_dest = home
            .join(".config")
            .join("opencode")
            .join("plugins")
            .join("herdr-agent-state.js");
        if herdr_dest.exists() {
            return HookInstallStatus::HerdrManaged;
        }

        HookInstallStatus::NotInstalled
    }

    fn install_opencode(home: &Path) -> Result<HookActionOutcome, HookError> {
        let dest = Self::opencode_plugin_path(home);
        if let Ok(existing) = fs::read_to_string(&dest) {
            if existing == OPENCODE_PLUGIN_TEMPLATE {
                return Ok(HookActionOutcome::AlreadyCurrent);
            }
        }
        write_executable_file(&dest, OPENCODE_PLUGIN_TEMPLATE)?;
        Ok(HookActionOutcome::Installed)
    }

    fn uninstall_opencode(home: &Path) -> Result<HookActionOutcome, HookError> {
        let dest = Self::opencode_plugin_path(home);
        if dest.exists() {
            fs::remove_file(&dest)?;
            Ok(HookActionOutcome::Uninstalled)
        } else {
            Ok(HookActionOutcome::NotInstalled)
        }
    }

    // ---------------------------------------------------------------------
    // Pi
    // ---------------------------------------------------------------------
    fn pi_extension_path(home: &Path) -> PathBuf {
        home.join(".pi")
            .join("agent")
            .join("extensions")
            .join("shardlane-agent-state.ts")
    }

    fn pi_status(home: &Path) -> HookInstallStatus {
        let dest = Self::pi_extension_path(home);
        if dest.exists() {
            if let Ok(content) = fs::read_to_string(&dest) {
                return check_version_in_content(&content);
            }
            return HookInstallStatus::Installed;
        }

        let herdr_dest = home
            .join(".pi")
            .join("agent")
            .join("extensions")
            .join("herdr-agent-state.ts");
        if herdr_dest.exists() {
            return HookInstallStatus::HerdrManaged;
        }

        HookInstallStatus::NotInstalled
    }

    fn install_pi(home: &Path) -> Result<HookActionOutcome, HookError> {
        let dest = Self::pi_extension_path(home);
        if let Ok(existing) = fs::read_to_string(&dest) {
            if existing == PI_EXTENSION_TEMPLATE {
                return Ok(HookActionOutcome::AlreadyCurrent);
            }
        }
        write_executable_file(&dest, PI_EXTENSION_TEMPLATE)?;
        Ok(HookActionOutcome::Installed)
    }

    fn uninstall_pi(home: &Path) -> Result<HookActionOutcome, HookError> {
        let dest = Self::pi_extension_path(home);
        if dest.exists() {
            fs::remove_file(&dest)?;
            Ok(HookActionOutcome::Uninstalled)
        } else {
            Ok(HookActionOutcome::NotInstalled)
        }
    }

    // ---------------------------------------------------------------------
    // Command Code
    // ---------------------------------------------------------------------
    fn command_code_mod_path(home: &Path) -> PathBuf {
        home.join(".commandcode")
            .join("mods")
            .join("shardlane-agent-state.ts")
    }

    fn command_code_status(home: &Path) -> HookInstallStatus {
        let dest = Self::command_code_mod_path(home);
        if dest.exists() {
            if let Ok(content) = fs::read_to_string(&dest) {
                return check_version_in_content(&content);
            }
            return HookInstallStatus::Installed;
        }

        let herdr_dest = home
            .join(".commandcode")
            .join("mods")
            .join("shardlane-herdr-agent-state.ts");
        if herdr_dest.exists() {
            return HookInstallStatus::HerdrManaged;
        }

        HookInstallStatus::NotInstalled
    }

    fn install_command_code(home: &Path) -> Result<HookActionOutcome, HookError> {
        let dest = Self::command_code_mod_path(home);
        if let Ok(existing) = fs::read_to_string(&dest) {
            if existing == COMMAND_CODE_MOD_TEMPLATE {
                return Ok(HookActionOutcome::AlreadyCurrent);
            }
        }
        write_executable_file(&dest, COMMAND_CODE_MOD_TEMPLATE)?;
        Ok(HookActionOutcome::Installed)
    }

    fn uninstall_command_code(home: &Path) -> Result<HookActionOutcome, HookError> {
        let dest = Self::command_code_mod_path(home);
        if dest.exists() {
            fs::remove_file(&dest)?;
            Ok(HookActionOutcome::Uninstalled)
        } else {
            Ok(HookActionOutcome::NotInstalled)
        }
    }

    // ---------------------------------------------------------------------
    // Cursor
    // ---------------------------------------------------------------------
    fn cursor_hook_path(home: &Path) -> PathBuf {
        home.join(".cursor").join("shardlane-agent-state.sh")
    }

    fn cursor_status(home: &Path) -> HookInstallStatus {
        let dest = Self::cursor_hook_path(home);
        if dest.exists() {
            return HookInstallStatus::Installed;
        }
        if home.join(".cursor").join("herdr-agent-state.sh").exists() {
            return HookInstallStatus::HerdrManaged;
        }
        HookInstallStatus::NotInstalled
    }

    fn install_cursor(home: &Path) -> Result<HookActionOutcome, HookError> {
        let dest = Self::cursor_hook_path(home);
        write_executable_file(&dest, SHELL_HOOK_TEMPLATE)?;
        Ok(HookActionOutcome::Installed)
    }

    fn uninstall_cursor(home: &Path) -> Result<HookActionOutcome, HookError> {
        let dest = Self::cursor_hook_path(home);
        if dest.exists() {
            fs::remove_file(&dest)?;
            Ok(HookActionOutcome::Uninstalled)
        } else {
            Ok(HookActionOutcome::NotInstalled)
        }
    }

    // ---------------------------------------------------------------------
    // Copilot
    // ---------------------------------------------------------------------
    fn copilot_hook_path(home: &Path) -> PathBuf {
        home.join(".copilot")
            .join("hooks")
            .join("shardlane-agent-state.sh")
    }

    fn copilot_status(home: &Path) -> HookInstallStatus {
        let dest = Self::copilot_hook_path(home);
        if dest.exists() {
            return HookInstallStatus::Installed;
        }
        if home
            .join(".copilot")
            .join("hooks")
            .join("herdr-agent-state.sh")
            .exists()
        {
            return HookInstallStatus::HerdrManaged;
        }
        HookInstallStatus::NotInstalled
    }

    fn install_copilot(home: &Path) -> Result<HookActionOutcome, HookError> {
        let dest = Self::copilot_hook_path(home);
        write_executable_file(&dest, SHELL_HOOK_TEMPLATE)?;
        Ok(HookActionOutcome::Installed)
    }

    fn uninstall_copilot(home: &Path) -> Result<HookActionOutcome, HookError> {
        let dest = Self::copilot_hook_path(home);
        if dest.exists() {
            fs::remove_file(&dest)?;
            Ok(HookActionOutcome::Uninstalled)
        } else {
            Ok(HookActionOutcome::NotInstalled)
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_claude_hook_install_idempotence_and_uninstall() {
        let temp = TempDir::new().unwrap();
        let home = temp.path();

        // 1. Initial status: NotInstalled
        assert_eq!(
            AgentHookRegistry::read_status(AgentId::ClaudeCode, home),
            HookInstallStatus::NotInstalled
        );

        // 2. First install
        let outcome = AgentHookRegistry::install(AgentId::ClaudeCode, home).unwrap();
        assert_eq!(outcome, HookActionOutcome::Installed);
        assert_eq!(
            AgentHookRegistry::read_status(AgentId::ClaudeCode, home),
            HookInstallStatus::Installed
        );

        // Verify settings.json content
        let settings = AgentHookRegistry::claude_settings_path(home);
        assert!(settings.exists());
        let content = fs::read_to_string(&settings).unwrap();
        assert!(content.contains("shardlane-claude-hook.sh"));
        assert!(content.contains("\"matcher\": \"*\""));
        assert!(content.contains("\"hooks\": ["));

        // 3. Second install (idempotent)
        let outcome2 = AgentHookRegistry::install(AgentId::ClaudeCode, home).unwrap();
        assert_eq!(outcome2, HookActionOutcome::AlreadyCurrent);

        // 4. Uninstall
        let un_outcome = AgentHookRegistry::uninstall(AgentId::ClaudeCode, home).unwrap();
        assert_eq!(un_outcome, HookActionOutcome::Uninstalled);
        assert_eq!(
            AgentHookRegistry::read_status(AgentId::ClaudeCode, home),
            HookInstallStatus::NotInstalled
        );

        // Verify settings.json cleaned up
        let content_after = fs::read_to_string(&settings).unwrap();
        assert!(!content_after.contains("shardlane-claude-hook.sh"));
    }

    #[test]
    fn test_codex_hook_install_idempotence_and_uninstall() {
        let temp = TempDir::new().unwrap();
        let home = temp.path();

        assert_eq!(
            AgentHookRegistry::read_status(AgentId::Codex, home),
            HookInstallStatus::NotInstalled
        );

        let outcome = AgentHookRegistry::install(AgentId::Codex, home).unwrap();
        assert_eq!(outcome, HookActionOutcome::Installed);
        assert_eq!(
            AgentHookRegistry::read_status(AgentId::Codex, home),
            HookInstallStatus::Installed
        );

        let outcome2 = AgentHookRegistry::install(AgentId::Codex, home).unwrap();
        assert_eq!(outcome2, HookActionOutcome::AlreadyCurrent);

        let un_outcome = AgentHookRegistry::uninstall(AgentId::Codex, home).unwrap();
        assert_eq!(un_outcome, HookActionOutcome::Uninstalled);
        assert_eq!(
            AgentHookRegistry::read_status(AgentId::Codex, home),
            HookInstallStatus::NotInstalled
        );
    }

    #[test]
    fn test_opencode_plugin_lifecycle() {
        let temp = TempDir::new().unwrap();
        let home = temp.path();

        assert_eq!(
            AgentHookRegistry::read_status(AgentId::Opencode, home),
            HookInstallStatus::NotInstalled
        );

        let outcome = AgentHookRegistry::install(AgentId::Opencode, home).unwrap();
        assert_eq!(outcome, HookActionOutcome::Installed);
        assert_eq!(
            AgentHookRegistry::read_status(AgentId::Opencode, home),
            HookInstallStatus::Installed
        );

        let outcome2 = AgentHookRegistry::install(AgentId::Opencode, home).unwrap();
        assert_eq!(outcome2, HookActionOutcome::AlreadyCurrent);

        let un_outcome = AgentHookRegistry::uninstall(AgentId::Opencode, home).unwrap();
        assert_eq!(un_outcome, HookActionOutcome::Uninstalled);
        assert_eq!(
            AgentHookRegistry::read_status(AgentId::Opencode, home),
            HookInstallStatus::NotInstalled
        );
    }

    #[test]
    fn test_pi_extension_lifecycle() {
        let temp = TempDir::new().unwrap();
        let home = temp.path();

        assert_eq!(
            AgentHookRegistry::read_status(AgentId::Pi, home),
            HookInstallStatus::NotInstalled
        );

        let outcome = AgentHookRegistry::install(AgentId::Pi, home).unwrap();
        assert_eq!(outcome, HookActionOutcome::Installed);
        assert_eq!(
            AgentHookRegistry::read_status(AgentId::Pi, home),
            HookInstallStatus::Installed
        );

        let un_outcome = AgentHookRegistry::uninstall(AgentId::Pi, home).unwrap();
        assert_eq!(un_outcome, HookActionOutcome::Uninstalled);
    }

    #[test]
    fn test_command_code_mod_lifecycle() {
        let temp = TempDir::new().unwrap();
        let home = temp.path();

        assert_eq!(
            AgentHookRegistry::read_status(AgentId::CommandCode, home),
            HookInstallStatus::NotInstalled
        );

        let outcome = AgentHookRegistry::install(AgentId::CommandCode, home).unwrap();
        assert_eq!(outcome, HookActionOutcome::Installed);
        assert_eq!(
            AgentHookRegistry::read_status(AgentId::CommandCode, home),
            HookInstallStatus::Installed
        );

        let outcome2 = AgentHookRegistry::install(AgentId::CommandCode, home).unwrap();
        assert_eq!(outcome2, HookActionOutcome::AlreadyCurrent);

        let un_outcome = AgentHookRegistry::uninstall(AgentId::CommandCode, home).unwrap();
        assert_eq!(un_outcome, HookActionOutcome::Uninstalled);
        assert_eq!(
            AgentHookRegistry::read_status(AgentId::CommandCode, home),
            HookInstallStatus::NotInstalled
        );
    }

    #[test]
    fn test_cursor_and_copilot_hook_lifecycle() {
        let temp = TempDir::new().unwrap();
        let home = temp.path();

        for &agent in &[AgentId::Cursor, AgentId::Copilot] {
            assert_eq!(
                AgentHookRegistry::read_status(agent, home),
                HookInstallStatus::NotInstalled
            );

            let outcome = AgentHookRegistry::install(agent, home).unwrap();
            assert_eq!(outcome, HookActionOutcome::Installed);
            assert_eq!(
                AgentHookRegistry::read_status(agent, home),
                HookInstallStatus::Installed
            );

            let un_outcome = AgentHookRegistry::uninstall(agent, home).unwrap();
            assert_eq!(un_outcome, HookActionOutcome::Uninstalled);
            assert_eq!(
                AgentHookRegistry::read_status(agent, home),
                HookInstallStatus::NotInstalled
            );
        }
    }

    #[test]
    fn test_herdr_managed_detection() {
        let temp = TempDir::new().unwrap();
        let home = temp.path();

        // Simulate Herdr official Claude hook
        let herdr_claude = home
            .join(".claude")
            .join("hooks")
            .join("herdr-agent-state.sh");
        fs::create_dir_all(herdr_claude.parent().unwrap()).unwrap();
        fs::write(&herdr_claude, "#!/bin/sh\n# Herdr official\n").unwrap();

        assert_eq!(
            AgentHookRegistry::read_status(AgentId::ClaudeCode, home),
            HookInstallStatus::HerdrManaged
        );

        // When user chooses to Integrate, it installs Shardlane hook over it
        let outcome = AgentHookRegistry::install(AgentId::ClaudeCode, home).unwrap();
        assert_eq!(outcome, HookActionOutcome::Installed);
        assert_eq!(
            AgentHookRegistry::read_status(AgentId::ClaudeCode, home),
            HookInstallStatus::Installed
        );
    }

    #[test]
    fn test_audit_all_returns_all_supported_agents() {
        let temp = TempDir::new().unwrap();
        let home = temp.path();

        let audits = AgentHookRegistry::audit_all(home);
        assert_eq!(audits.len(), AgentHookRegistry::SUPPORTED_AGENTS.len());
        for meta in audits {
            assert!(AgentHookRegistry::SUPPORTED_AGENTS.contains(&meta.agent));
            assert_eq!(meta.status, HookInstallStatus::NotInstalled);
        }
    }
}
