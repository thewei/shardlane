//! Local IPC Server and OSC Escape Sequence parser for Agent Status sniffing.
//!
//! [INPUT]: Incoming reports from Shardlane Agent Hooks via local Unix Domain Socket
//! or inline terminal stream (OSC 1337).
//! [OUTPUT]: Normalized `AgentHookReport` instances dispatched through an async channel
//! to ShardlaneApp navigation & status projection.
//! [POS]: IPC listener and OSC parser for Tier 1 hook sniffing across backends.

use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{self, BufRead, BufReader};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

pub const DEFAULT_SOCKET_NAME: &str = "agent-status.sock";

/// A reported Agent state event.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentHookReport {
    pub agent: String,
    pub status: String,
    #[serde(default)]
    pub pane_id: Option<String>,
    #[serde(default)]
    pub tmux_pane: Option<String>,
    #[serde(default)]
    pub herdr_pane: Option<String>,
    #[serde(default)]
    pub tty: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub event: Option<String>,
    #[serde(default)]
    pub timestamp: u64,
}

impl AgentHookReport {
    /// Canonical resolved pane id: checks `tmux_pane`, `herdr_pane`, or generic `pane_id`.
    pub fn resolved_pane_id(&self) -> Option<&str> {
        self.tmux_pane
            .as_deref()
            .or(self.herdr_pane.as_deref())
            .or(self.pane_id.as_deref())
    }
}

/// Parses OSC 1337 escape sequence for AgentStatus.
///
/// Format: `\x1b]1337;AgentStatus=<status>;agent=<agent>;pane=<pane_id>\x07`
/// Or terminated with `\x1b\\` (ST).
pub fn parse_osc_agent_status(bytes: &[u8]) -> Option<AgentHookReport> {
    const PREFIX: &[u8] = b"\x1b]1337;AgentStatus=";
    let pos = bytes.windows(PREFIX.len()).position(|w| w == PREFIX)?;
    let remainder = &bytes[pos + PREFIX.len()..];

    // Find terminator: 0x07 (BEL) or 0x1b 0x5c (ST)
    let end_pos = remainder
        .iter()
        .position(|&b| b == 0x07)
        .or_else(|| remainder.windows(2).position(|w| w == b"\x1b\\"))?;

    let payload = std::str::from_utf8(&remainder[..end_pos]).ok()?;

    // Parse key=value pairs: `working;agent=claude;pane=%1` or `working`
    let mut parts = payload.split(';');
    let first = parts.next()?.trim();
    if first.is_empty() {
        return None;
    }

    let status = if first.contains('=') {
        let (k, v) = first.split_once('=')?;
        if k == "status" || k == "state" {
            v.trim().to_string()
        } else {
            return None;
        }
    } else {
        first.to_string()
    };

    let mut agent = "unknown".to_string();
    let mut pane_id = None;
    let mut session_id = None;

    for part in parts {
        let part = part.trim();
        if let Some((k, v)) = part.split_once('=') {
            match k.trim() {
                "agent" => agent = v.trim().to_string(),
                "pane" | "pane_id" => pane_id = Some(v.trim().to_string()),
                "session" | "session_id" => session_id = Some(v.trim().to_string()),
                _ => {}
            }
        }
    }

    Some(AgentHookReport {
        agent,
        status,
        pane_id: pane_id.clone(),
        tmux_pane: pane_id.clone(),
        herdr_pane: pane_id,
        tty: None,
        session_id,
        cwd: None,
        event: None,
        timestamp: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
    })
}

/// Local Unix Domain Socket server for receiving AgentHookReports.
pub struct AgentHookIpcServer {
    socket_path: PathBuf,
    shutdown: Arc<AtomicBool>,
    listener_thread: Option<JoinHandle<()>>,
}

impl AgentHookIpcServer {
    /// Default socket path: `~/.shardlane/run/agent-status.sock`
    pub fn default_socket_path() -> Option<PathBuf> {
        std::env::var_os("HOME").map(|h| {
            PathBuf::from(h)
                .join(".shardlane")
                .join("run")
                .join(DEFAULT_SOCKET_NAME)
        })
    }

    /// Start listening on the specified socket path and forwarding reports to sender.
    pub fn start(
        socket_path: PathBuf,
        sender: async_channel::Sender<AgentHookReport>,
    ) -> Result<Self, io::Error> {
        if let Some(parent) = socket_path.parent() {
            fs::create_dir_all(parent)?;
            let _ = fs::set_permissions(parent, fs::Permissions::from_mode(0o700));
        }

        if socket_path.exists() {
            let _ = fs::remove_file(&socket_path);
        }

        let listener = UnixListener::bind(&socket_path)?;
        let _ = fs::set_permissions(&socket_path, fs::Permissions::from_mode(0o600));

        let shutdown = Arc::new(AtomicBool::new(false));
        let shutdown_clone = Arc::clone(&shutdown);
        let path_clone = socket_path.clone();

        let listener_thread = thread::Builder::new()
            .name("shardlane-agent-hook-ipc".to_string())
            .spawn(move || {
                let _ = listener.set_nonblocking(true);
                while !shutdown_clone.load(Ordering::Relaxed) {
                    match listener.accept() {
                        Ok((stream, _)) => {
                            let tx = sender.clone();
                            thread::Builder::new()
                                .name("shardlane-hook-client".to_string())
                                .spawn(move || {
                                    handle_client(stream, tx);
                                })
                                .ok();
                        }
                        Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(25));
                        }
                        Err(_) => break,
                    }
                }
            })?;

        Ok(Self {
            socket_path: path_clone,
            shutdown,
            listener_thread: Some(listener_thread),
        })
    }

    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }
}

impl Drop for AgentHookIpcServer {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
        if let Some(handle) = self.listener_thread.take() {
            let _ = handle.join();
        }
        let _ = fs::remove_file(&self.socket_path);
    }
}

fn handle_client(stream: UnixStream, sender: async_channel::Sender<AgentHookReport>) {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let reader = BufReader::new(stream);
    for line in reader.lines() {
        let Ok(text) = line else { break };
        let trimmed = text.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Ok(report) = serde_json::from_str::<AgentHookReport>(trimmed) {
            let _ = sender.send_blocking(report);
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    #[test]
    fn test_parse_osc_agent_status_bel() {
        let input = b"prefix\x1b]1337;AgentStatus=working;agent=claude;pane=%2\x07suffix";
        let report = parse_osc_agent_status(input).expect("should parse");
        assert_eq!(report.agent, "claude");
        assert_eq!(report.status, "working");
        assert_eq!(report.pane_id.as_deref(), Some("%2"));
    }

    #[test]
    fn test_parse_osc_agent_status_st() {
        let input = b"data\x1b]1337;AgentStatus=blocked;agent=codex;pane=%5\x1b\\more";
        let report = parse_osc_agent_status(input).expect("should parse");
        assert_eq!(report.agent, "codex");
        assert_eq!(report.status, "blocked");
        assert_eq!(report.pane_id.as_deref(), Some("%5"));
    }

    #[test]
    fn test_ipc_server_roundtrip() {
        let temp = TempDir::new().unwrap();
        let sock_path = temp.path().join("test.sock");
        let (tx, rx) = async_channel::unbounded();

        let server = AgentHookIpcServer::start(sock_path.clone(), tx).expect("server start");
        assert!(server.socket_path().exists());

        // Connect client and send JSON
        let mut client = UnixStream::connect(&sock_path).expect("client connect");
        let sample = AgentHookReport {
            agent: "claude".to_string(),
            status: "working".to_string(),
            pane_id: Some("%1".to_string()),
            tmux_pane: Some("%1".to_string()),
            herdr_pane: None,
            tty: None,
            session_id: Some("session-123".to_string()),
            cwd: Some("/test/dir".to_string()),
            event: Some("UserPromptSubmit".to_string()),
            timestamp: 123456,
        };

        let json = serde_json::to_string(&sample).unwrap();
        writeln!(client, "{json}").unwrap();
        client.flush().unwrap();
        drop(client);

        // Receive
        let received = rx.recv_blocking().expect("must receive report");
        assert_eq!(received.agent, "claude");
        assert_eq!(received.status, "working");
        assert_eq!(received.pane_id.as_deref(), Some("%1"));
        assert_eq!(received.resolved_pane_id(), Some("%1"));
    }

    #[test]
    fn test_parse_osc_agent_status_malformed_and_edge_cases() {
        // Missing prefix
        assert!(parse_osc_agent_status(b"plain terminal output without osc").is_none());
        // Missing terminator
        assert!(parse_osc_agent_status(b"\x1b]1337;AgentStatus=working;agent=claude").is_none());
        // Empty status
        assert!(parse_osc_agent_status(b"\x1b]1337;AgentStatus=;agent=claude\x07").is_none());
        // Extra fields & session ID
        let input = b"\x1b]1337;AgentStatus=idle;agent=pi;pane=%3;session=sess-99\x07";
        let report = parse_osc_agent_status(input).unwrap();
        assert_eq!(report.agent, "pi");
        assert_eq!(report.status, "idle");
        assert_eq!(report.pane_id.as_deref(), Some("%3"));
        assert_eq!(report.session_id.as_deref(), Some("sess-99"));
    }
}
