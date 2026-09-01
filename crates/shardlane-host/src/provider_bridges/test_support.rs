//! Test support helpers and synthetic provider bridge client.
//!
//! [INPUT]: Socket path and synthetic requests.
//! [OUTPUT]: End-to-end synthetic bridge client and verification fixtures.
//! [POS]: S3 provider bridge test support.

use super::protocol::{ProviderBridgeEnvelope, ProviderBridgeReply};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::time::Duration;

pub struct SyntheticBridgeClient {
    socket_path: PathBuf,
}

impl SyntheticBridgeClient {
    pub fn new(socket_path: impl Into<PathBuf>) -> Self {
        Self {
            socket_path: socket_path.into(),
        }
    }

    /// Send an envelope over the local Unix socket and wait for the reply.
    pub fn send_envelope(
        &self,
        envelope: &ProviderBridgeEnvelope,
        timeout: Duration,
    ) -> Result<ProviderBridgeReply, String> {
        let mut stream = UnixStream::connect(&self.socket_path).map_err(|e| {
            format!(
                "failed to connect to socket {}: {e}",
                self.socket_path.display()
            )
        })?;

        stream
            .set_read_timeout(Some(timeout))
            .map_err(|e| format!("set read timeout failed: {e}"))?;
        stream
            .set_write_timeout(Some(Duration::from_secs(5)))
            .map_err(|e| format!("set write timeout failed: {e}"))?;

        let json = serde_json::to_string(envelope)
            .map_err(|e| format!("failed to serialize envelope: {e}"))?;

        stream
            .write_all(json.as_bytes())
            .map_err(|e| format!("write error: {e}"))?;
        stream
            .write_all(b"\n")
            .map_err(|e| format!("write newline error: {e}"))?;
        stream.flush().map_err(|e| format!("flush error: {e}"))?;

        let mut reader = BufReader::new(&stream);
        let mut response_line = String::new();
        reader
            .read_line(&mut response_line)
            .map_err(|e| format!("read reply error: {e}"))?;

        let reply: ProviderBridgeReply = serde_json::from_str(response_line.trim())
            .map_err(|e| format!("failed to deserialize reply: {e} (raw: {response_line:?})"))?;

        Ok(reply)
    }

    /// Send raw bytes over the socket (for testing boundary/oversized frames).
    pub fn send_raw(&self, data: &[u8], timeout: Duration) -> Result<ProviderBridgeReply, String> {
        let mut stream = UnixStream::connect(&self.socket_path).map_err(|e| {
            format!(
                "failed to connect to socket {}: {e}",
                self.socket_path.display()
            )
        })?;

        stream
            .set_read_timeout(Some(timeout))
            .map_err(|e| format!("set read timeout failed: {e}"))?;
        stream
            .set_write_timeout(Some(Duration::from_secs(5)))
            .map_err(|e| format!("set write timeout failed: {e}"))?;

        stream
            .write_all(data)
            .map_err(|e| format!("write error: {e}"))?;
        stream.flush().map_err(|e| format!("flush error: {e}"))?;

        let mut reader = BufReader::new(&stream);
        let mut response_line = String::new();
        reader
            .read_line(&mut response_line)
            .map_err(|e| format!("read reply error: {e}"))?;

        let reply: ProviderBridgeReply = serde_json::from_str(response_line.trim())
            .map_err(|e| format!("failed to deserialize reply: {e} (raw: {response_line:?})"))?;

        Ok(reply)
    }
}
