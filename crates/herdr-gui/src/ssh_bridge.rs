//! [INPUT]: the `ssh` CLI (non-interactive key auth assumed: ssh-copy-id /
//! Tailscale), the local machine's `~/.shardlane` directory.
//! [OUTPUT]: `SshBridge { target, local_socket, child }` + `bring_up(target)`
//! — spawns `ssh -N -L <local-unix-sock>:<remote-herdr-sock> <target>` and
//! probes the forwarded socket with a herdr ping, so remote Herdr instances
//! become addressable through the SAME client path as local ones.
//! [POS]: Leaf process bridge in `crates/herdr-gui`; bridge children are
//! detached (they must outlive the calling task) and tracked process-globally
//! so they die with the app.

use std::process::{Child, Command, Stdio};
use std::sync::{Mutex, OnceLock};

/// Live SSH bridges, process-global: the `ssh` child must outlive the task
/// that spawned it (dropping `Child` does not kill it, but keeping the handle
/// documents ownership and lets a future settings page stop bridges).
fn bridges() -> &'static Mutex<Vec<(String, Child)>> {
    static BRIDGES: OnceLock<Mutex<Vec<(String, Child)>>> = OnceLock::new();
    BRIDGES.get_or_init(|| Mutex::new(Vec::new()))
}

fn bridge_dir() -> std::path::PathBuf {
    let home = std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    home.join(".shardlane").join("ssh-bridges")
}

fn local_socket_path(target: &str) -> std::path::PathBuf {
    let slug: String = target
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
        .collect();
    bridge_dir().join(format!("{slug}.sock"))
}

/// Resolves the remote user's HOME (needed to point at
/// `<remote-home>/.config/herdr/herdr.sock`).
fn remote_home(target: &str) -> Result<String, String> {
    let output = Command::new("ssh")
        .args([
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=8",
            target,
            "echo $HOME",
        ])
        .stdin(Stdio::null())
        .output()
        .map_err(|error| format!("ssh spawn: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "ssh {} failed: {}",
            target,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let home = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if home.is_empty() {
        return Err("remote $HOME resolved empty".to_string());
    }
    Ok(home)
}

/// Brings up the socket forward and verifies the remote herdr answers a ping
/// through it. Blocking; run on the background executor.
pub(crate) fn bring_up(device_id: &str, target: &str) -> Result<SshBridge, String> {
    let remote_home = remote_home(target)?;
    let local_socket = local_socket_path(target);
    let _ = std::fs::remove_file(&local_socket);
    let parent = local_socket
        .parent()
        .ok_or_else(|| "bridge dir has no parent".to_string())?;
    std::fs::create_dir_all(parent).map_err(|error| format!("bridge dir: {error}"))?;
    let remote_socket = format!("{remote_home}/.config/herdr/herdr.sock");
    let forward = format!("{}:{}", local_socket.display(), remote_socket);

    let mut command = Command::new("ssh");
    command
        .args(["-N"])
        .args(["-o", "BatchMode=yes"])
        .args(["-o", "ExitOnForwardFailure=yes"])
        .args(["-o", "ServerAliveInterval=15"])
        .args(["-o", "ServerAliveCountMax=3"])
        .args(["-L", &forward])
        .arg(target)
        .stdin(Stdio::null());
    let child = command
        .spawn()
        .map_err(|error| format!("ssh -N spawn: {error}"))?;

    // Probe: the local socket appears once the remote forward is established.
    let mut pinged: Option<Result<(), String>> = None;
    for _ in 0..40 {
        std::thread::sleep(std::time::Duration::from_millis(250));
        if local_socket.exists() {
            let probe = shardlane_host::mux::InstanceRef::socket("herdr", local_socket.clone());
            let probe_result =
                std::sync::Arc::new(shardlane_host::mux::MuxRegistry::with_builtins())
                    .connect_instance(&probe)
                    .and_then(|connection| connection.ping());
            match probe_result {
                Ok(()) => {
                    pinged = Some(Ok(()));
                    break;
                }
                Err(error) => pinged = Some(Err(error.to_string())),
            }
        }
    }
    match pinged {
        Some(Ok(())) => {
            bridges()
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .push((target.to_string(), child));
            Ok(SshBridge {
                device_id: device_id.to_string(),
                target: target.to_string(),
                local_socket,
            })
        }
        other => {
            // Kill the forward so a dead bridge never lingers.
            let mut child = child;
            let _ = child.kill();
            match other {
                Some(Err(error)) => {
                    Err(format!("remote herdr unreachable through bridge: {error}"))
                }
                _ => Err("forwarded socket never became ready (10s)".to_string()),
            }
        }
    }
}

/// Enumerates the Herdr instances on a remote machine over SSH
/// (`ssh <target> herdr session list --json`). Blocking; background executor.
pub(crate) fn list_remote_sessions(
    target: &str,
) -> Result<Vec<shardlane_host::herdr::HerdrSessionListing>, String> {
    let output = Command::new("ssh")
        .args(["-o", "BatchMode=yes", "-o", "ConnectTimeout=8"])
        .args([target, "herdr", "session", "list", "--json"])
        .stdin(Stdio::null())
        .output()
        .map_err(|error| format!("ssh spawn: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "remote session list failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let parsed: serde_json::Value =
        serde_json::from_slice(&output.stdout).map_err(|error| error.to_string())?;
    let sessions = parsed
        .get("sessions")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| "remote session list missing sessions".to_string())?;
    Ok(sessions
        .iter()
        .filter_map(|session| {
            let name = session.get("name")?.as_str()?.to_string();
            let running = session
                .get("running")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false);
            let is_default = session
                .get("default")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false);
            Some(shardlane_host::herdr::HerdrSessionListing {
                name,
                running,
                is_default,
            })
        })
        .collect())
}

/// A live bridge to one SSH machine's herdr socket. The fields become read
/// once instance resolution gains the device dimension (bind-by-bridge-socket);
/// today the bridge is verified live at bring-up and kept for teardown.
#[derive(Clone)]
#[allow(dead_code)]
pub(crate) struct SshBridge {
    /// The hosting device's registry id (settings.devices).
    pub(crate) device_id: String,
    pub(crate) target: String,
    pub(crate) local_socket: std::path::PathBuf,
}
