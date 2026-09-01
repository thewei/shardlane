//! Local Unix Domain Socket listener and runtime descriptor manager.
//!
//! [INPUT]: Connections from local provider companion hooks or synthetic test clients.
//! [OUTPUT]: Managed socket endpoint, atomic descriptor write, framed JSON message
//! handling, and graceful cancellation on client disconnect.
//! [POS]: S3 local provider bridge IPC. Local-only; user-permissions restricted;
//! bounds every frame to 256 KiB; timeout and disconnect fail closed.

use super::protocol::{
    ProviderBridgeEnvelope, ProviderBridgeReply, BRIDGE_PROTOCOL_VERSION, MAX_BRIDGE_FRAME_BYTES,
};
use super::registry::ProviderBridgeRegistry;
use crate::conversation_interactions::{BridgeResolutionDisposition, InteractionCancelReason};
use crate::ids::BridgeRequestId;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread::{self, JoinHandle};
use std::time::Duration;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BridgeRuntimeDescriptor {
    pub protocol: u16,
    pub pid: u32,
    pub generation: String,
    pub socket_path: String,
}

pub struct LocalProviderBridgeServer {
    registry: Arc<ProviderBridgeRegistry>,
    runtime_dir: PathBuf,
    descriptor_path: PathBuf,
    socket_path: PathBuf,
    shutdown_flag: Arc<AtomicBool>,
    listener_thread: Option<JoinHandle<()>>,
}

impl LocalProviderBridgeServer {
    /// Start a local provider bridge server in the specified runtime directory.
    pub fn start(
        registry: Arc<ProviderBridgeRegistry>,
        runtime_dir: impl AsRef<Path>,
    ) -> Result<Self, String> {
        let runtime_dir = runtime_dir.as_ref().to_path_buf();
        fs::create_dir_all(&runtime_dir).map_err(|e| {
            format!(
                "failed to create runtime dir {}: {e}",
                runtime_dir.display()
            )
        })?;

        // Restrict runtime directory permissions to current user only (0700)
        let _ = fs::set_permissions(&runtime_dir, fs::Permissions::from_mode(0o700));

        let generation = uuid::Uuid::new_v4().to_string();
        let short_id = &generation[..8];
        let socket_name = format!("b-{short_id}.sock");
        let socket_path = runtime_dir.join(socket_name);
        let descriptor_path = runtime_dir.join("provider-bridge.json");

        // Clean up previous stale socket if any
        if socket_path.exists() {
            let _ = fs::remove_file(&socket_path);
        }

        let listener = UnixListener::bind(&socket_path)
            .map_err(|e| format!("failed to bind unix socket {}: {e}", socket_path.display()))?;

        // Socket file permissions 0600
        let _ = fs::set_permissions(&socket_path, fs::Permissions::from_mode(0o600));

        // Atomic write descriptor
        let descriptor = BridgeRuntimeDescriptor {
            protocol: BRIDGE_PROTOCOL_VERSION,
            pid: std::process::id(),
            generation: generation.clone(),
            socket_path: socket_path.to_string_lossy().to_string(),
        };

        let descriptor_json = serde_json::to_string_pretty(&descriptor)
            .map_err(|e| format!("failed to serialize descriptor: {e}"))?;

        let temp_descriptor_path = runtime_dir.join(format!("provider-bridge-{generation}.tmp"));
        fs::write(&temp_descriptor_path, descriptor_json)
            .map_err(|e| format!("failed to write temp descriptor: {e}"))?;
        fs::rename(&temp_descriptor_path, &descriptor_path)
            .map_err(|e| format!("failed to rename descriptor: {e}"))?;

        let shutdown_flag = Arc::new(AtomicBool::new(false));
        let shutdown_flag_clone = Arc::clone(&shutdown_flag);
        let registry_clone = Arc::clone(&registry);

        let listener_thread = thread::Builder::new()
            .name("shardlane-bridge-listener".to_string())
            .spawn(move || {
                // Set non-blocking on listener with short sleep to allow clean shutdown checks
                listener.set_nonblocking(true).ok();
                while !shutdown_flag_clone.load(Ordering::Relaxed) {
                    match listener.accept() {
                        Ok((stream, _)) => {
                            let reg = Arc::clone(&registry_clone);
                            thread::Builder::new()
                                .name("shardlane-bridge-conn".to_string())
                                .spawn(move || {
                                    handle_client_connection(stream, reg);
                                })
                                .ok();
                        }
                        Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(15));
                        }
                        Err(_) => {
                            break;
                        }
                    }
                }
            })
            .map_err(|e| format!("failed to spawn listener thread: {e}"))?;

        Ok(Self {
            registry,
            runtime_dir,
            descriptor_path,
            socket_path,
            shutdown_flag,
            listener_thread: Some(listener_thread),
        })
    }

    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    pub fn runtime_dir(&self) -> &Path {
        &self.runtime_dir
    }

    pub fn descriptor_path(&self) -> &Path {
        &self.descriptor_path
    }

    pub fn registry(&self) -> &Arc<ProviderBridgeRegistry> {
        &self.registry
    }
}

impl Drop for LocalProviderBridgeServer {
    fn drop(&mut self) {
        self.shutdown_flag.store(true, Ordering::Relaxed);
        if let Some(handle) = self.listener_thread.take() {
            let _ = handle.join();
        }
        let _ = fs::remove_file(&self.socket_path);
        let _ = fs::remove_file(&self.descriptor_path);
    }
}

fn handle_client_connection(mut stream: UnixStream, registry: Arc<ProviderBridgeRegistry>) {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(300)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));

    let read_stream = match stream.try_clone() {
        Ok(s) => s,
        Err(_) => return,
    };
    let mut reader = BufReader::new(read_stream);
    let mut buffer = Vec::new();

    // Read up to MAX_BRIDGE_FRAME_BYTES
    let mut take_reader = reader.by_ref().take(MAX_BRIDGE_FRAME_BYTES as u64 + 1);
    let read_result = take_reader.read_until(b'\n', &mut buffer);

    if let Err(e) = read_result {
        let reply = ProviderBridgeReply::Error {
            message: format!("read error: {e}"),
        };
        send_reply(&mut stream, &reply);
        return;
    }

    if buffer.len() > MAX_BRIDGE_FRAME_BYTES {
        // Drain the remainder of the oversized line so the peer finishes sending without RST
        let mut sink = Vec::new();
        let _ = reader.read_until(b'\n', &mut sink);

        let total_size = buffer.len() + sink.len();
        let reply = ProviderBridgeReply::Error {
            message: format!(
                "frame size {total_size} exceeds maximum allowed {} bytes",
                MAX_BRIDGE_FRAME_BYTES
            ),
        };
        send_reply(&mut stream, &reply);
        return;
    }

    let raw_str = String::from_utf8_lossy(&buffer);
    let trimmed = raw_str.trim();
    if trimmed.is_empty() {
        return;
    }

    let envelope: ProviderBridgeEnvelope = match serde_json::from_str(trimmed) {
        Ok(env) => env,
        Err(e) => {
            let reply = ProviderBridgeReply::Error {
                message: format!("invalid JSON frame: {e}"),
            };
            send_reply(&mut stream, &reply);
            return;
        }
    };

    let (tx, rx) = mpsc::sync_channel::<BridgeResolutionDisposition>(1);
    let bridge_req_id = BridgeRequestId::new(&envelope.request_id);

    let responder: Arc<dyn Fn(BridgeResolutionDisposition) + Send + Sync + 'static> =
        Arc::new(move |disp| {
            let _ = tx.send(disp);
        });

    let initial_reply = registry.handle_envelope(&envelope, Some(responder));

    match initial_reply {
        ProviderBridgeReply::CaptureAndWait { .. } => {
            // Wait for resolution or client disconnect
            match rx.recv_timeout(Duration::from_secs(3600)) {
                Ok(BridgeResolutionDisposition::Resolved(resolution)) => {
                    match registry.encode_resolution(&envelope, &resolution) {
                        Ok(provider_resp) => {
                            let reply = ProviderBridgeReply::Resolved {
                                response: provider_resp,
                            };
                            send_reply(&mut stream, &reply);
                        }
                        Err(err) => {
                            let reply = ProviderBridgeReply::Error {
                                message: format!("failed to encode resolution: {err}"),
                            };
                            send_reply(&mut stream, &reply);
                        }
                    }
                }
                Ok(BridgeResolutionDisposition::NativeFallback) => {
                    send_reply(&mut stream, &ProviderBridgeReply::NativeFallback);
                }
                Ok(BridgeResolutionDisposition::Cancelled(reason)) => {
                    let reply = ProviderBridgeReply::Cancelled {
                        reason: format!("{reason:?}"),
                    };
                    send_reply(&mut stream, &reply);
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    registry.broker().cancel_by_bridge(
                        &bridge_req_id,
                        InteractionCancelReason::ProviderCancelled,
                    );
                    let reply = ProviderBridgeReply::Cancelled {
                        reason: "interaction timed out waiting for resolution".to_string(),
                    };
                    send_reply(&mut stream, &reply);
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    registry.broker().cancel_by_bridge(
                        &bridge_req_id,
                        InteractionCancelReason::BridgeDisconnected,
                    );
                }
            }
        }
        other => {
            send_reply(&mut stream, &other);
        }
    }
}

fn send_reply(stream: &mut UnixStream, reply: &ProviderBridgeReply) {
    if let Ok(json) = serde_json::to_string(reply) {
        let _ = stream.write_all(json.as_bytes());
        let _ = stream.write_all(b"\n");
        let _ = stream.flush();
    }
}
