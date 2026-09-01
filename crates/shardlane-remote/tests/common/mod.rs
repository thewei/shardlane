//! Shared integration test harness: minimal HTTP client + isolated Herdr
//! instance lifecycle.
//!
//! [INPUT]: Depends on the herdr binary (HERDR_BIN or PATH), tempfile, and
//! tokio TcpStream
//! [OUTPUT]: Provides http_get/http_post and the IsolatedHerdr guard (Drop
//! cleans up the process and the temp dir)
//! [POS]: The common base of loopback_bootstrap.rs (and later batch tests);
//! same discipline throughout: temp HOME + unique socket + post-test
//! process cleanup

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::Duration;

pub const TOKEN: &str = "integration-test-token-0123456789abcdef";

const HERDR_IDENTITY_ENV_KEYS: &[&str] = &[
    "HERDR_BIN_PATH",
    "HERDR_CONFIG_PATH",
    "HERDR_ENV",
    "HERDR_PANE_ID",
    "HERDR_SESSION",
    "HERDR_SOCKET_PATH",
    "HERDR_STARTUP_CWD",
    "HERDR_TAB_ID",
    "HERDR_WORKSPACE_ID",
];

fn isolate_herdr_command(command: &mut Command, home: &Path, socket: &Path) {
    for key in HERDR_IDENTITY_ENV_KEYS {
        command.env_remove(key);
    }
    command
        .env("HOME", home)
        .env("HERDR_SOCKET_PATH", socket)
        .env("HERDR_SESSION", "shardlane-remote-test");
}

/// Minimal HTTP/1.1 request (loopback, no TLS; axum returns content-length
/// for JSON responses).
pub async fn http_request(
    addr: std::net::SocketAddr,
    method: &str,
    path: &str,
    token: Option<&str>,
    body: Option<&str>,
) -> (u16, String) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let mut stream = tokio::net::TcpStream::connect(addr)
        .await
        .unwrap_or_else(|e| panic!("connect: {e}"));
    let auth = match token {
        Some(token) => format!("Authorization: Bearer {token}\r\n"),
        None => String::new(),
    };
    let (content_headers, body) = match body {
        Some(json) => (
            "Content-Type: application/json\r\n".to_string(),
            json.to_string(),
        ),
        None => (String::new(), String::new()),
    };
    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: localhost\r\n{auth}{content_headers}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream
        .write_all(request.as_bytes())
        .await
        .unwrap_or_else(|e| panic!("write: {e}"));
    let mut raw = Vec::new();
    stream
        .read_to_end(&mut raw)
        .await
        .unwrap_or_else(|e| panic!("read: {e}"));
    let text = String::from_utf8_lossy(&raw).to_string();
    let status = text
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse::<u16>().ok())
        .unwrap_or_else(|| panic!("malformed response: {text}"));
    let body = text
        .split_once("\r\n\r\n")
        .map(|(_, body)| body.to_string())
        .unwrap_or_default();
    (status, body)
}

pub async fn http_get(
    addr: std::net::SocketAddr,
    path: &str,
    token: Option<&str>,
) -> (u16, String) {
    http_request(addr, "GET", path, token, None).await
}

pub fn herdr_binary_for_tests() -> PathBuf {
    herdr_binary()
}

/// Whether the loopback integration tests can spawn a real Herdr binary.
/// Fresh clones and CI without Herdr installed skip these tests instead of
/// failing; install Herdr or set `HERDR_BIN` to run them.
pub fn herdr_available() -> bool {
    std::env::var_os("HERDR_BIN").is_some() || which_via_path_lookup().is_ok()
}

fn herdr_binary() -> PathBuf {
    if let Some(path) = std::env::var_os("HERDR_BIN") {
        return PathBuf::from(path);
    }
    if let Ok(path) = which_via_path_lookup() {
        return path;
    }
    panic!("herdr binary not found; set HERDR_BIN to run loopback integration tests");
}

fn which_via_path_lookup() -> Result<PathBuf, String> {
    let path = std::env::var("PATH").map_err(|e| e.to_string())?;
    for dir in path.split(':') {
        let candidate = Path::new(dir).join("herdr");
        if candidate.exists() {
            return Ok(candidate);
        }
    }
    Err("not on PATH".into())
}

/// Isolated Herdr instance guard: Drop kills the process and removes the
/// temp dir.
pub struct IsolatedHerdr {
    pub home: PathBuf,
    pub socket: PathBuf,
    server: Option<Child>,
}

impl IsolatedHerdr {
    pub fn spawn() -> Self {
        let home = tempfile::Builder::new()
            .prefix("shardlane-remote-test-")
            .tempdir_in("/tmp")
            .unwrap_or_else(|e| panic!("tempdir: {e}"))
            .keep();
        let socket = home.join("herdr.sock");
        let mut server_command = Command::new(herdr_binary());
        server_command.arg("server");
        isolate_herdr_command(&mut server_command, &home, &socket);
        let server = server_command
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap_or_else(|e| panic!("spawn herdr server: {e}"));
        let instance = Self {
            home,
            socket,
            server: Some(server),
        };
        instance.wait_for_socket();
        instance
    }

    fn wait_for_socket(&self) {
        for _ in 0..100 {
            if self.socket.exists() {
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        panic!("herdr socket never appeared at {}", self.socket.display());
    }

    pub fn cli(&self, args: &[&str], cwd: Option<&Path>) -> String {
        let mut command = Command::new(herdr_binary());
        command.args(args);
        isolate_herdr_command(&mut command, &self.home, &self.socket);
        command
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        if let Some(cwd) = cwd {
            command.current_dir(cwd);
        }
        let output = command
            .output()
            .unwrap_or_else(|e| panic!("herdr {}: {e}", args.join(" ")));
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        if !output.status.success() {
            panic!(
                "herdr {} failed: {} {}",
                args.join(" "),
                stdout,
                String::from_utf8_lossy(&output.stderr)
            );
        }
        stdout
    }

    /// Seed: create a runtime workspace with home as cwd (workspace create
    /// comes with the first tab/pane; the cwd is the source of the project
    /// path).
    pub fn seed_workspace(&self) {
        self.cli(&["workspace", "create"], Some(&self.home));
    }
}

pub fn spawn_against(herdr: &IsolatedHerdr) -> shardlane_remote::RemoteServerHandle {
    spawn_against_with(herdr, "shardlane-host-integration", "integration-mac")
}

pub fn spawn_against_with(
    herdr: &IsolatedHerdr,
    host_id: &str,
    host_name: &str,
) -> shardlane_remote::RemoteServerHandle {
    use shardlane_remote::config::{ListenerMode, RemoteConfig};
    use shardlane_remote::{spawn_remote_server, RemoteServerOptions};

    let config = RemoteConfig {
        enabled: true,
        listener_mode: ListenerMode::Loopback,
        port: 0,
        host_id: Some(host_id.into()),
        access_token: Some(TOKEN.into()),
        web_url: None,
    };
    spawn_remote_server(RemoteServerOptions {
        conversation_sessions: None,
        shared_tui: None,
        delivery_coordinator: None,
        config,
        host_name: host_name.into(),
        host_version: "0.1.11".into(),
        settings_path: PathBuf::from("/nonexistent/settings.json"),
        herdr_socket_override: Some(herdr.socket.clone()),
        web_bundle_path: None,
    })
    .unwrap_or_else(|error| panic!("spawn remote: {error}"))
}

impl Drop for IsolatedHerdr {
    fn drop(&mut self) {
        if let Some(mut server) = self.server.take() {
            let _ = server.kill();
            let _ = server.wait();
        }
        let _ = std::fs::remove_dir_all(&self.home);
    }
}
