//! [INPUT]: std::net (TcpListener/TcpStream/SocketAddr, loopback only),
//!          std::io Read/Write, std::sync (Mutex/MutexGuard + atomic),
//!          std::thread, std::time (Duration/Instant)
//! [OUTPUT]: DiscoveredServer, PROBE_HOST, PROBE_PORTS, MAX_SCAN_PORTS,
//!           CONNECT_TIMEOUT, PROBE_TIMEOUT, candidate_ports, dedup_ports,
//!           probe_port, probe_port_with, scan_once, ScanState
//! [POS]: Bounded loopback service discovery for the right-panel Browser
//!        omnibox suggestions; consumed only by browser_view.rs.
//! [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

/// Red line: the probe NEVER leaves the loopback interface. The host is a
/// hard-coded constant — no API in this module accepts a caller-supplied host.
pub(crate) const PROBE_HOST: &str = "127.0.0.1";

/// Whitelist of common dev-server ports. Bounded by design: the scan cost is
/// ports x per-port budget, so the whitelist itself is the resource contract.
pub(crate) const PROBE_PORTS: [u16; 24] = [
    3000, 3001, 3002, 3333, 4000, 4001, 4173, 4200, 4500, 5000, 5001, 5173, 5174, 5175, 7000, 8000,
    8001, 8008, 8080, 8081, 8088, 8888, 9000, 9090,
];

/// Hard cap for the normalized candidate list, enforced after dedup.
pub(crate) const MAX_SCAN_PORTS: usize = 64;

pub(crate) const CONNECT_TIMEOUT: Duration = Duration::from_millis(150);
pub(crate) const PROBE_TIMEOUT: Duration = Duration::from_millis(500);

/// Response-head size cap: a discovery only needs the status line plus a
/// couple of headers, so anything larger is treated as noise and dropped.
const MAX_HEAD_BYTES: usize = 8 * 1024;

/// One discovered HTTP service on loopback.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DiscoveredServer {
    pub port: u16,
    /// Well-known product name by port, else the ~BT~Server~BT~ response header,
    /// else a generic label. Lives in memory only — never persisted.
    pub name: String,
}

/// The normalized scan target list: sorted, deduplicated, hard-truncated, so
/// a future whitelist edit can never silently widen the scan beyond the bound.
pub(crate) fn candidate_ports() -> Vec<u16> {
    dedup_ports(&PROBE_PORTS)
}

pub(crate) fn dedup_ports(ports: &[u16]) -> Vec<u16> {
    let mut normalized = ports.to_vec();
    normalized.sort_unstable();
    normalized.dedup();
    normalized.truncate(MAX_SCAN_PORTS);
    normalized
}

pub(crate) fn probe_port(port: u16) -> Option<DiscoveredServer> {
    probe_port_with(port, CONNECT_TIMEOUT, PROBE_TIMEOUT)
}

/// TCP connect (150 ms) + one ~BT~GET /~BT~ whose response head must arrive within
/// ~BT~probe_timeout~BT~ (500 ms). Only 200..=399 counts as a discovery: dev
/// servers answer 2xx/3xx, error pages and non-HTTP noise never qualify.
pub(crate) fn probe_port_with(
    port: u16,
    connect_timeout: Duration,
    probe_timeout: Duration,
) -> Option<DiscoveredServer> {
    let address = SocketAddr::from(([127, 0, 0, 1], port));
    let mut stream = TcpStream::connect_timeout(&address, connect_timeout).ok()?;
    let deadline = Instant::now() + probe_timeout;
    let _ = stream.set_write_timeout(Some(connect_timeout));
    let request = format!(
        "GET / HTTP/1.1\r\nHost: {PROBE_HOST}:{port}\r\nConnection: close\r\n\
         User-Agent: shardlane-loopback-probe\r\n\r\n"
    );
    stream.write_all(request.as_bytes()).ok()?;
    let head = read_head(&mut stream, deadline)?;
    parse_response_head(&head, port)
}

/// Reads until the end of the response head (~BT~\r\n\r\n~BT~), the deadline, EOF,
/// or the size cap — whichever comes first. A hanging server therefore costs
/// exactly ~BT~probe_timeout~BT~, never more.
fn read_head(stream: &mut TcpStream, deadline: Instant) -> Option<Vec<u8>> {
    let mut buffer = [0u8; 1024];
    let mut head = Vec::with_capacity(buffer.len());
    loop {
        // None here means the deadline has passed.
        let remaining = deadline.checked_duration_since(Instant::now())?;
        let _ = stream.set_read_timeout(Some(remaining));
        match stream.read(&mut buffer) {
            Ok(0) => return None,
            Ok(read) => {
                head.extend_from_slice(&buffer[..read]);
                if head.windows(4).any(|window| window == b"\r\n\r\n") {
                    return Some(head);
                }
                if head.len() > MAX_HEAD_BYTES {
                    return None;
                }
            }
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock
                        | std::io::ErrorKind::TimedOut
                        | std::io::ErrorKind::Interrupted
                ) =>
            {
                // The loop re-checks the deadline; the next read timeout is
                // exactly the remaining budget.
            }
            Err(_) => return None,
        }
    }
}

/// ~BT~HTTP/x.y CODE~BT~ status line must parse and land in 200..=399; the service
/// name prefers the well-known port label over the ~BT~Server~BT~ header.
fn parse_response_head(head: &[u8], port: u16) -> Option<DiscoveredServer> {
    let text = String::from_utf8_lossy(head);
    let status = text.split("\r\n").next()?;
    let code = status.split_whitespace().nth(1)?.parse::<u16>().ok()?;
    if !(200..=399).contains(&code) {
        return None;
    }
    let mut header_name = None;
    for line in text.split("\r\n").skip(1) {
        if line.is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            if name.trim().eq_ignore_ascii_case("server") {
                let value = value.trim();
                if !value.is_empty() {
                    header_name = Some(value.to_string());
                }
            }
        }
    }
    let name = well_known_name(port)
        .map(str::to_string)
        .or(header_name)
        .unwrap_or_else(|| "Local server".to_string());
    Some(DiscoveredServer { port, name })
}

fn well_known_name(port: u16) -> Option<&'static str> {
    let name = match port {
        3000..=3002 => "Node dev server",
        3333 | 4000 | 4001 | 7000 | 9090 => "Dev server",
        4173 => "Vite preview",
        4200 => "Angular",
        5000 | 5001 | 8000 | 8001 | 8008 => "Python dev server",
        5173..=5175 => "Vite",
        8080 | 8081 | 8088 => "HTTP dev server",
        8888 => "Jupyter",
        9000 => "PHP dev server",
        _ => return None,
    };
    Some(name)
}

/// One bounded scan: at most MAX_SCAN_PORTS short-lived threads, each capped
/// at CONNECT_TIMEOUT + PROBE_TIMEOUT, so total wall time stays around the
/// per-port budget (~650 ms) no matter how many ports answer slowly or hang.
pub(crate) fn scan_once() -> Vec<DiscoveredServer> {
    let handles: Vec<_> = candidate_ports()
        .into_iter()
        .map(|port| thread::spawn(move || probe_port(port)))
        .collect();
    let mut found = Vec::new();
    for handle in handles {
        if let Ok(Some(server)) = handle.join() {
            found.push(server);
        }
    }
    found.sort_unstable_by_key(|server| server.port);
    found
}

// Manual-trigger-only (the deliberate choice over a low-frequency timer):
// with the setting on, traffic happens exactly when the user clicks the scan
// row in the omnibox suggestions — zero scans on focus, typing, hover, or any
// timer. The result cache below is the ONLY state this module owns; it lives
// in memory and is never written to disk (spec red line).
static SCAN_IN_FLIGHT: AtomicBool = AtomicBool::new(false);
static SCAN_RESULTS: Mutex<Vec<DiscoveredServer>> = Mutex::new(Vec::new());

/// Single-flight guard + in-memory result cache for one bounded scan.
pub(crate) struct ScanState;

impl ScanState {
    /// Claims the single scan slot; ~BT~false~BT~ means a scan is already running.
    pub(crate) fn start() -> bool {
        !SCAN_IN_FLIGHT.swap(true, Ordering::SeqCst)
    }

    pub(crate) fn is_in_flight() -> bool {
        SCAN_IN_FLIGHT.load(Ordering::SeqCst)
    }

    /// Publishes results and releases the scan slot.
    pub(crate) fn finish(results: Vec<DiscoveredServer>) {
        *Self::results_slot() = results;
        SCAN_IN_FLIGHT.store(false, Ordering::SeqCst);
    }

    pub(crate) fn results() -> Vec<DiscoveredServer> {
        Self::results_slot().clone()
    }

    fn results_slot() -> MutexGuard<'static, Vec<DiscoveredServer>> {
        SCAN_RESULTS
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    /// Binds an ephemeral loopback listener; ~BT~handler~BT~ runs once for the first
    /// accepted connection on a helper thread. Returns the port for probing.
    fn serve_once(handler: impl FnOnce(TcpStream) + Send + 'static) -> u16 {
        let listener = TcpListener::bind((PROBE_HOST, 0)).expect("bind loopback");
        let port = listener.local_addr().expect("local addr").port();
        thread::spawn(move || {
            if let Ok((stream, _)) = listener.accept() {
                handler(stream);
            }
        });
        port
    }

    fn http_head_response(status: &'static str) -> impl FnOnce(TcpStream) + Send + 'static {
        move |mut stream: TcpStream| {
            let head =
                format!("HTTP/1.1 {status}\r\nServer: TestServ\r\nContent-Length: 0\r\n\r\n");
            let _ = stream.write_all(head.as_bytes());
            let _ = stream.flush();
        }
    }

    #[test]
    fn port_whitelist_is_bounded_unique_and_sorted() {
        assert!(PROBE_PORTS.len() <= MAX_SCAN_PORTS);
        let candidates = candidate_ports();
        assert!(!candidates.is_empty());
        assert!(candidates.len() <= MAX_SCAN_PORTS);
        assert!(candidates.windows(2).all(|pair| pair[0] < pair[1]));
    }

    #[test]
    fn dedup_ports_normalizes_duplicates_and_caps() {
        let mut wide: Vec<u16> = (0..100u16).map(|offset| 10_000 + offset).collect();
        wide.extend_from_slice(&[5173, 5173, 3000, 3000, 3000]);
        let normalized = dedup_ports(&wide);
        assert_eq!(&normalized[..2], &[3000, 5173]);
        assert_eq!(normalized.len(), MAX_SCAN_PORTS);
    }

    #[test]
    fn http_success_statuses_are_discovered() {
        for status in ["200 OK", "204 No Content", "301 Moved", "399 Custom"] {
            let port = serve_once(http_head_response(status));
            let found = probe_port_with(port, CONNECT_TIMEOUT, PROBE_TIMEOUT)
                .unwrap_or_else(|| panic!("{status} should be a discovery"));
            assert_eq!(found.name, "TestServ");
        }
    }

    #[test]
    fn http_error_statuses_are_rejected() {
        for status in [
            "199 Almost",
            "400 Bad Request",
            "404 Not Found",
            "500 Server Error",
        ] {
            let port = serve_once(http_head_response(status));
            assert!(
                probe_port_with(port, CONNECT_TIMEOUT, PROBE_TIMEOUT).is_none(),
                "{status} must not be a discovery"
            );
        }
    }

    #[test]
    fn well_known_port_names_win_over_server_header() {
        let head = b"HTTP/1.1 200 OK\r\nServer: nginx\r\n\r\n";
        let vite = parse_response_head(head, 5173).expect("200 is a discovery");
        assert_eq!(vite.name, "Vite");
        let header_named = parse_response_head(head, 9999).expect("200 is a discovery");
        assert_eq!(header_named.name, "nginx");
        assert_eq!(well_known_name(4173), Some("Vite preview"));
    }

    #[test]
    fn refused_connections_are_not_discoveries() {
        let listener = TcpListener::bind((PROBE_HOST, 0)).expect("bind loopback");
        let port = listener.local_addr().expect("local addr").port();
        drop(listener);
        assert!(probe_port_with(port, CONNECT_TIMEOUT, PROBE_TIMEOUT).is_none());
    }

    #[test]
    fn pending_server_hits_the_probe_deadline() {
        let listener = TcpListener::bind((PROBE_HOST, 0)).expect("bind loopback");
        let port = listener.local_addr().expect("local addr").port();
        thread::spawn(move || {
            // Accept and hold the socket without answering; the probe must
            // give up on its own deadline.
            if let Ok((stream, _)) = listener.accept() {
                thread::sleep(Duration::from_secs(2));
                drop(stream);
            }
        });
        let started = Instant::now();
        let probe = probe_port_with(port, CONNECT_TIMEOUT, Duration::from_millis(200));
        assert!(probe.is_none());
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "the probe must be bounded by its own timeout, not the server's"
        );
    }

    #[test]
    fn scan_state_is_single_flight_and_cached_in_memory_only() {
        assert!(ScanState::start(), "the first claim wins");
        assert!(
            !ScanState::start(),
            "a second claim is rejected while in flight"
        );
        assert!(ScanState::is_in_flight());
        ScanState::finish(vec![DiscoveredServer {
            port: 5173,
            name: "Vite".to_string(),
        }]);
        assert!(!ScanState::is_in_flight());
        assert_eq!(
            ScanState::results(),
            vec![DiscoveredServer {
                port: 5173,
                name: "Vite".to_string(),
            }]
        );
    }
}
