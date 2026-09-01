//! Bearer authentication middleware: the single gate in front of all
//! endpoints (including WS upgrade).
//!
//! [INPUT]: Depends on axum middleware, subtle (constant-time comparison,
//! tokens never logged), and crate::cors's BEARER_SUBPROTOCOL (the browser
//! WS alternative channel)
//! [OUTPUT]: Exposes the require_bearer middleware (Authorization header
//! first; Sec-WebSocket-Protocol subprotocol compatible with
//! [`shardlane.bearer`, token])
//! [POS]: The authentication boundary of shardlane-remote; when R7
//! pairing/device credentials arrive, they replace this layer without
//! touching handlers or DTOs

use crate::cors::BEARER_SUBPROTOCOL;
use crate::error::ApiError;
use crate::state::RemoteState;
use axum::extract::State;
use axum::http::header::{AUTHORIZATION, SEC_WEBSOCKET_PROTOCOL};
use axum::http::Request;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use std::sync::Arc;
use subtle::ConstantTimeEq;

/// Constant-time token comparison (early return on length difference is an
/// acceptable information leak — token length is not a secret; content
/// comparison never leaks timing).
fn token_matches(expected: &str, presented: &str) -> bool {
    let expected_bytes = expected.as_bytes();
    let presented_bytes = presented.as_bytes();
    if expected_bytes.len() != presented_bytes.len() {
        return false;
    }
    expected_bytes.ct_eq(presented_bytes).into()
}

/// Browser WS authentication channel:
/// `Sec-WebSocket-Protocol: shardlane.bearer, <token>`. The browser
/// WebSocket API cannot carry an Authorization header; passing the token as
/// the second protocol item keeps it out of URLs/logs, and the server echoes
/// the selected protocol in its 101 response. Only the exact two-item form
/// with a matching first item is accepted; anything extra/missing is None.
fn subprotocol_token(request: &Request<axum::body::Body>) -> Option<String> {
    let header = request
        .headers()
        .get(SEC_WEBSOCKET_PROTOCOL)?
        .to_str()
        .ok()?;
    let items: Vec<&str> = header
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .collect();
    if items.len() == 2 && items[0] == BEARER_SUBPROTOCOL {
        return Some(items[1].to_string());
    }
    None
}

pub async fn require_bearer(
    State(state): State<Arc<RemoteState>>,
    request: Request<axum::body::Body>,
    next: Next,
) -> Response {
    let request_id = state.next_request_id();
    let Some(expected) = state.config.access_token.as_deref() else {
        // enabled without a token is a configuration error: reject
        // everything (fail-closed).
        return ApiError::unauthorized("remote access is not configured", request_id)
            .into_response();
    };
    if expected.is_empty() {
        return ApiError::unauthorized("remote access is not configured", request_id)
            .into_response();
    }

    let presented = request
        .headers()
        .get(AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .map(str::to_string)
        .or_else(|| subprotocol_token(&request));

    match presented.as_deref() {
        Some(token) if token_matches(expected, token) => next.run(request).await,
        _ => ApiError::unauthorized("invalid or missing bearer token", request_id).into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_comparison_is_exact() {
        assert!(token_matches("abc123", "abc123"));
        assert!(!token_matches("abc123", "abc124"));
        assert!(!token_matches("abc123", "abc1234"));
        assert!(!token_matches("abc123", ""));
    }

    #[test]
    fn subprotocol_parsing_requires_exact_pair() {
        let build = |header: &str| {
            axum::http::Request::builder()
                .header("sec-websocket-protocol", header)
                .body(axum::body::Body::empty())
                .unwrap_or_else(|error| panic!("build: {error}"))
        };
        assert_eq!(
            subprotocol_token(&build("shardlane.bearer, abc123")),
            Some("abc123".to_string())
        );
        assert_eq!(
            subprotocol_token(&build("shardlane.bearer,abc123")),
            Some("abc123".to_string())
        );
        // Extra items / mismatched first item / single item → reject.
        assert_eq!(
            subprotocol_token(&build("shardlane.bearer, abc, extra")),
            None
        );
        assert_eq!(subprotocol_token(&build("other.proto, abc123")), None);
        assert_eq!(subprotocol_token(&build("shardlane.bearer")), None);
        assert_eq!(subprotocol_token(&build("")), None);
    }
}
