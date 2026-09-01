//! Stable error envelope: the unified wire format of every non-2xx
//! response, independent of raw Herdr error strings.
//!
//! [INPUT]: Depends on axum (IntoResponse), serde (envelope serialization),
//! and shardlane-host's diagnostics (used when auth-failure counts etc. go
//! to lag_log)
//! [OUTPUT]: Exposes ApiError (status+code+message+request_id, optional
//! created Agent target) and all v1 error-code constants; IntoResponse emits
//! the stable error envelope.
//! [POS]: The error boundary of shardlane-remote;
//! `forbidden/unsupported_capability` are reserved wire codes with no
//! constructor (pinned by the D25 vocabulary test); stack traces/socket
//! paths/raw Herdr errors must never be carried. Every envelope response
//! echoes its request_id in the `x-request-id` header (D12).

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;

// ---- v1 error-code vocabulary (plan §2; the last three reserved for R7+) ----
pub const CODE_UNAUTHORIZED: &str = "unauthorized";
pub const CODE_INVALID_REQUEST: &str = "invalid_request";
pub const CODE_NOT_FOUND: &str = "not_found";
pub const CODE_TIMEOUT: &str = "timeout";
pub const CODE_HOST_UNAVAILABLE: &str = "host_unavailable";
pub const CODE_RUNTIME_UNAVAILABLE: &str = "runtime_unavailable";
pub const CODE_INTERNAL: &str = "internal";
pub const CODE_CONFLICT: &str = "conflict";
/// R2-05: stable wire code for "the mutation may already be accepted".
pub const CODE_DELIVERY_UNCERTAIN: &str = "delivery_uncertain";
/// The runtime Agent exists, but a post-start setup/readiness step failed.
/// This is a committed, reconcileable terminal outcome, not a retryable
/// "start failed" transport error.
pub const CODE_AGENT_CREATED: &str = "agent_created";
#[allow(dead_code)]
pub const CODE_FORBIDDEN: &str = "forbidden";
#[allow(dead_code)]
pub const CODE_UNSUPPORTED_CAPABILITY: &str = "unsupported_capability";

#[derive(Clone, Debug)]
pub struct ApiError {
    pub status: StatusCode,
    pub code: &'static str,
    pub message: String,
    pub request_id: String,
    /// Post-commit Agent target for reconcileable `agent_created` outcomes.
    /// None preserves the legacy error envelope for unrelated failures.
    pub created_target: Option<Box<AgentCreatedTarget>>,
}

/// Exact runtime structure that already exists after a launch commit.  This
/// is deliberately transport-shaped (opaque ids only); clients can focus or
/// reconcile the target without parsing human-readable error text.
#[derive(Clone, Debug, Serialize)]
pub struct AgentCreatedTarget {
    pub agent_ref: String,
    pub tab_id: String,
    pub pane_id: String,
    pub phase: String,
}

impl ApiError {
    fn new(
        status: StatusCode,
        code: &'static str,
        message: impl Into<String>,
        request_id: String,
    ) -> Self {
        Self {
            status,
            code,
            message: message.into(),
            request_id,
            created_target: None,
        }
    }

    pub fn unauthorized(message: impl Into<String>, request_id: String) -> Self {
        Self::new(
            StatusCode::UNAUTHORIZED,
            CODE_UNAUTHORIZED,
            message,
            request_id,
        )
    }

    /// M6: History continuation needs a Project before any runtime mutation.
    pub fn needs_project_selection(message: impl Into<String>, request_id: String) -> Self {
        Self::new(
            StatusCode::CONFLICT,
            "needs_project_selection",
            message,
            request_id,
        )
    }

    pub fn invalid_request(message: impl Into<String>, request_id: String) -> Self {
        Self::new(
            StatusCode::BAD_REQUEST,
            CODE_INVALID_REQUEST,
            message,
            request_id,
        )
    }

    pub fn not_found(message: impl Into<String>, request_id: String) -> Self {
        Self::new(StatusCode::NOT_FOUND, CODE_NOT_FOUND, message, request_id)
    }

    pub fn timeout(message: impl Into<String>, request_id: String) -> Self {
        Self::new(
            StatusCode::GATEWAY_TIMEOUT,
            CODE_TIMEOUT,
            message,
            request_id,
        )
    }

    pub fn host_unavailable(message: impl Into<String>, request_id: String) -> Self {
        Self::new(
            StatusCode::SERVICE_UNAVAILABLE,
            CODE_HOST_UNAVAILABLE,
            message,
            request_id,
        )
    }

    pub fn runtime_unavailable(message: impl Into<String>, request_id: String) -> Self {
        Self::new(
            StatusCode::SERVICE_UNAVAILABLE,
            CODE_RUNTIME_UNAVAILABLE,
            message,
            request_id,
        )
    }

    pub fn internal(message: impl Into<String>, request_id: String) -> Self {
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            CODE_INTERNAL,
            message,
            request_id,
        )
    }

    /// R2-10: a request id reused for a different mutation body.
    pub fn conflict(message: impl Into<String>, request_id: String) -> Self {
        Self::new(StatusCode::CONFLICT, CODE_CONFLICT, message, request_id)
    }

    /// R2-05: the mutation may already be accepted (post-write response loss).
    /// Stable wire state — idempotent replays observe the same uncertainty and
    /// must never re-execute the mutation.
    pub fn delivery_uncertain(message: impl Into<String>, request_id: String) -> Self {
        Self::new(
            StatusCode::SERVICE_UNAVAILABLE,
            CODE_DELIVERY_UNCERTAIN,
            message,
            request_id,
        )
    }

    pub fn agent_created(message: impl Into<String>, request_id: String) -> Self {
        Self::new(
            StatusCode::CONFLICT,
            CODE_AGENT_CREATED,
            message,
            request_id,
        )
    }

    pub fn agent_created_with_target(
        message: impl Into<String>,
        request_id: String,
        created_target: AgentCreatedTarget,
    ) -> Self {
        let mut error = Self::agent_created(message, request_id);
        error.created_target = Some(Box::new(created_target));
        error
    }
}

#[derive(Serialize)]
struct ErrorDetail {
    code: String,
    message: String,
    request_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    target: Option<AgentCreatedTarget>,
}

#[derive(Serialize)]
struct ErrorBody {
    error: ErrorDetail,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let Self {
            status,
            code,
            message,
            request_id,
            created_target,
        } = self;
        let body = ErrorBody {
            error: ErrorDetail {
                code: code.to_string(),
                message,
                request_id: request_id.clone(),
                target: created_target.map(|target| *target),
            },
        };
        // D12: the envelope's request id is echoed in the `x-request-id`
        // response header so header and body stay correlated; non-envelope
        // responses get a transport id from the server's request-id layer.
        let mut response = (status, Json(body)).into_response();
        if let Ok(value) = axum::http::HeaderValue::from_str(&request_id) {
            response.headers_mut().insert("x-request-id", value);
        }
        response
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_envelope_serializes_stable_shape() {
        let error = ApiError::unauthorized("bad token", "req-7".into());
        let body = axum::Json(ErrorBody {
            error: ErrorDetail {
                code: error.code.to_string(),
                message: error.message.clone(),
                request_id: error.request_id.clone(),
                target: error.created_target.as_deref().cloned(),
            },
        });
        let json = serde_json::to_string(&body.0).unwrap_or_default();
        assert_eq!(
            json,
            "{\"error\":{\"code\":\"unauthorized\",\"message\":\"bad token\",\"request_id\":\"req-7\"}}"
        );
    }

    #[test]
    fn agent_created_envelope_carries_structured_target() {
        let error = ApiError::agent_created_with_target(
            "setup needs attention",
            "req-8".into(),
            AgentCreatedTarget {
                agent_ref: "pane-8".into(),
                tab_id: "tab-8".into(),
                pane_id: "pane-8".into(),
                phase: "not_ready".into(),
            },
        );
        let body = ErrorBody {
            error: ErrorDetail {
                code: error.code.to_string(),
                message: error.message,
                request_id: error.request_id,
                target: error.created_target.map(|target| *target),
            },
        };
        let json = serde_json::to_value(body).unwrap_or_default();
        assert_eq!(json["error"]["target"]["agent_ref"], "pane-8");
        assert_eq!(json["error"]["target"]["tab_id"], "tab-8");
        assert_eq!(json["error"]["target"]["phase"], "not_ready");
    }

    /// D12: every error envelope response carries `x-request-id` equal to the
    /// body's request_id (header and envelope stay correlated).
    #[test]
    fn error_envelope_echoes_request_id_header() {
        let response = ApiError::not_found("gone", "req-42".into()).into_response();
        assert_eq!(
            response
                .headers()
                .get("x-request-id")
                .and_then(|value| value.to_str().ok()),
            Some("req-42")
        );
    }

    /// D25: the complete wire-code vocabulary, pinned. Each active code is
    /// exercised through its constructor so spelling and constructor stay in
    /// lockstep; `forbidden` and `unsupported_capability` are deliberately
    /// reserved (R7+, no constructor) and are referenced here so "reserved"
    /// cannot rot silently — adding, renaming, or promoting a code must
    /// update this test on purpose.
    #[test]
    fn wire_code_vocabulary_is_pinned() {
        let id = || "req-vocab".to_string();
        let constructed: Vec<&'static str> = vec![
            ApiError::unauthorized("m", id()).code,
            ApiError::invalid_request("m", id()).code,
            ApiError::not_found("m", id()).code,
            ApiError::timeout("m", id()).code,
            ApiError::host_unavailable("m", id()).code,
            ApiError::runtime_unavailable("m", id()).code,
            ApiError::internal("m", id()).code,
            ApiError::conflict("m", id()).code,
            ApiError::delivery_uncertain("m", id()).code,
            ApiError::agent_created("m", id()).code,
            ApiError::needs_project_selection("m", id()).code,
        ];
        let expected: Vec<&'static str> = vec![
            CODE_UNAUTHORIZED,
            CODE_INVALID_REQUEST,
            CODE_NOT_FOUND,
            CODE_TIMEOUT,
            CODE_HOST_UNAVAILABLE,
            CODE_RUNTIME_UNAVAILABLE,
            CODE_INTERNAL,
            CODE_CONFLICT,
            CODE_DELIVERY_UNCERTAIN,
            CODE_AGENT_CREATED,
            "needs_project_selection",
        ];
        assert_eq!(constructed, expected, "active wire codes drifted");

        // Reserved for R7+: no constructor emits them, but the wire spelling
        // is part of the vocabulary and must not change silently.
        assert_eq!(CODE_FORBIDDEN, "forbidden");
        assert_eq!(CODE_UNSUPPORTED_CAPABILITY, "unsupported_capability");
        assert!(!constructed.contains(&CODE_FORBIDDEN));
        assert!(!constructed.contains(&CODE_UNSUPPORTED_CAPABILITY));
    }
}
