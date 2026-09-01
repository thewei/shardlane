//! Web/PWA development CORS layer: loopback-only development allowlist
//! (web-pwa-architecture.md §4).
//!
//! [INPUT]: Depends on axum middleware/http; stateless (pure-function
//! verdicts)
//! [OUTPUT]: Exposes the cors_guard middleware — a preflight short-circuit
//! 204 for allowlisted origins and ACAO headers on ordinary responses;
//! non-allowlisted origins get no headers (the browser blocks them
//! naturally)
//! [POS]: The browser adaptation layer of shardlane-remote, wrapped outside
//! the auth layer (preflight carries no credentials and must pass before a
//! 401); once W4 Host serves the static bundle same-origin, this layer
//! degrades to a no-op

use axum::extract::Request;
use axum::http::{header, HeaderValue, Method};
use axum::middleware::Next;
use axum::response::Response;

/// Subprotocol auth protocol name (the WS browser channel; the token is
/// passed as the second protocol item and never lands in a URL).
pub const BEARER_SUBPROTOCOL: &str = "shardlane.bearer";
/// Extra development allowlist (comma-separated origins for test injection;
/// never in config).
const EXTRA_ORIGINS_ENV: &str = "SHARDLANE_REMOTE_EXTRA_ORIGINS";

fn extra_origins() -> Vec<String> {
    std::env::var(EXTRA_ORIGINS_ENV)
        .ok()
        .map(|value| {
            value
                .split(',')
                .map(str::trim)
                .filter(|origin| !origin.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// Loopback development allowlist: localhost / 127.0.0.1 / [::1] (any port)
/// plus env additions. This is the "loopback-only development" discipline — the
/// formal LAN/PWA path goes through W4 same-origin serving or R8; this layer
/// never widens it.
fn is_whitelisted_origin(origin: &str) -> bool {
    if extra_origins().iter().any(|candidate| candidate == origin) {
        return true;
    }
    let Some((scheme, rest)) = origin.split_once("://") else {
        return false;
    };
    if scheme != "http" && scheme != "https" {
        return false;
    }
    let host = rest.rsplit_once(':').map_or(rest, |(host, _)| host);
    matches!(host, "localhost" | "127.0.0.1" | "[::1]")
}

fn apply_cors_headers(response: &mut Response, origin: &str) {
    let headers = response.headers_mut();
    if let Ok(value) = HeaderValue::from_str(origin) {
        headers.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN, value);
    }
    // Must stay in sync with the mounted routes: v1/v2 mount DELETE
    // (close_workspace/close_tab/close_pane/clear_pane_agent, TUI close),
    // so a preflight for DELETE would fail without it.
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_METHODS,
        HeaderValue::from_static("GET, POST, DELETE, OPTIONS"),
    );
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_HEADERS,
        HeaderValue::from_static("Authorization, Content-Type"),
    );
    headers.insert(
        header::ACCESS_CONTROL_MAX_AGE,
        HeaderValue::from_static("600"),
    );
    headers.insert(header::VARY, HeaderValue::from_static("Origin"));
}

pub async fn cors_guard(request: Request, next: Next) -> Response {
    let origin = request
        .headers()
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);

    // Preflight (browser spec: OPTIONS carries no credentials): allowlisted
    // origins short-circuit with 204 and must not fall into the auth layer
    // (otherwise 401 without ACAO makes the browser report CORS instead of
    // an auth error).
    if request.method() == Method::OPTIONS {
        return match origin.as_deref() {
            Some(origin) if is_whitelisted_origin(origin) => {
                let mut response = Response::new(axum::body::Body::empty());
                *response.status_mut() = axum::http::StatusCode::NO_CONTENT;
                apply_cors_headers(&mut response, origin);
                response
            }
            _ => {
                // OPTIONS from a non-allowlisted/absent origin: no CORS
                // headers; hand to downstream for its usual semantics.
                next.run(request).await
            }
        };
    }

    let mut response = next.run(request).await;
    if let Some(origin) = origin
        .as_deref()
        .filter(|origin| is_whitelisted_origin(origin))
    {
        apply_cors_headers(&mut response, origin);
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_origins_are_whitelisted_across_ports() {
        assert!(is_whitelisted_origin("http://localhost:8081"));
        assert!(is_whitelisted_origin("http://127.0.0.1:8090"));
        assert!(is_whitelisted_origin("http://[::1]:19006"));
        assert!(is_whitelisted_origin("https://localhost"));
    }

    #[test]
    fn remote_and_malformed_origins_are_rejected() {
        assert!(!is_whitelisted_origin("http://192.168.1.5:8090"));
        assert!(!is_whitelisted_origin("https://evil.example.com"));
        assert!(!is_whitelisted_origin("localhost:8081"));
        assert!(!is_whitelisted_origin("chrome-extension://abc"));
    }

    #[test]
    fn preflight_allows_every_mounted_method_including_delete() {
        let mut response = Response::new(axum::body::Body::empty());
        apply_cors_headers(&mut response, "http://localhost:8081");
        let methods = response
            .headers()
            .get(header::ACCESS_CONTROL_ALLOW_METHODS)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_string();
        // v1/v2 mount DELETE routes (workspace/tab/pane close, pane agent
        // clear, TUI close); the preflight must advertise them.
        assert_eq!(methods, "GET, POST, DELETE, OPTIONS");
        for method in ["GET", "POST", "DELETE", "OPTIONS"] {
            assert!(
                methods.split(',').any(|part| part.trim() == method),
                "missing method {method} in {methods}"
            );
        }
    }
}
