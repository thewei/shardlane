//! [INPUT]: `RemoteState` (Project registry snapshot), `shardlane-host`
//! client resolution (`connect_herdr_for` / `build_bootstrap_for`).
//! [OUTPUT]: `GET /api/v2/instances` (registry + running status) and
//! `GET /api/v2/instances/{id}/bootstrap` (per-instance HostBootstrap) —
//! the multi-instance surface mobile clients use to pick a workspace and
//! read its live state.
//! [POS]: Route handlers in `shardlane-remote`; registered in
//! `server.rs::build_router`. Scoped mutations ride `?instance=` through the
//! instance-scope middleware instead of dedicated routes.

use crate::state::RemoteState;
use axum::extract::{Path, State};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;
use std::sync::Arc;

/// One row of `GET /api/v2/instances`. One instance = one Herdr session =
/// one workspace; herdr's own `default` session is an ordinary member.
#[derive(Debug, Serialize)]
pub struct InstanceSummary {
    pub id: String,
    pub name: String,
    /// The Herdr session name this instance runs on.
    pub session: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_id: Option<String>,
    /// Always false: instances are Herdr-owned, never Shardlane-maintained.
    pub adopted: bool,
    /// Cosmetic display name (desktop rename override; falls back to the
    /// session name).
    pub display_name: String,
    /// Whether the instance's socket answers a ping right now.
    pub running: bool,
    /// Mirrors herdr's own `default` session flag (data, not a special case).
    pub is_default: bool,
    /// Remote API version (mirrors hello; convenience for clients that only
    /// call this endpoint first).
    pub remote_api_version: u32,
}

/// Cosmetic display name: the session's metadata file name, else the raw
/// session name.
fn display_name_for(session: &str) -> String {
    shardlane_host::herdr::read_session_display_name(session).unwrap_or_else(|| session.to_string())
}

pub async fn list_instances(State(_state): State<Arc<RemoteState>>) -> Response {
    // Workspaces ARE Herdr instances: enumerate from the CLI (no
    // Shardlane-side registry). Display names come from each session's
    // metadata file on herdr's own disk.
    let sessions = tokio::task::spawn_blocking(shardlane_host::herdr::list_sessions)
        .await
        .unwrap_or_default()
        .unwrap_or_default();
    let instances: Vec<InstanceSummary> = sessions
        .iter()
        .map(|session| InstanceSummary {
            id: session.name.clone(),
            name: session.name.clone(),
            display_name: display_name_for(&session.name),
            session: session.name.clone(),
            device_id: None,
            adopted: false,
            running: session.running,
            is_default: session.is_default,
            remote_api_version: crate::config::REMOTE_API_VERSION,
        })
        .collect();
    Json(serde_json::json!({ "instances": instances })).into_response()
}

/// `POST /api/v2/instances/{id}/rename {name}`: writes the session's
/// metadata display name (cosmetic rename; herdr sessions have no rename).
#[derive(serde::Deserialize)]
pub struct RenameInstanceRequest {
    pub name: String,
}

pub async fn rename_instance(
    State(state): State<Arc<RemoteState>>,
    Path(id): Path<String>,
    body: Result<Json<RenameInstanceRequest>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let request_id = state.next_request_id();
    let body = match crate::server::decode_body(body, &request_id) {
        Ok(body) => body,
        Err(error) => return error.into_response(),
    };
    let name = body.name.trim().to_string();
    if name.is_empty() {
        return crate::error::ApiError::invalid_request("name must not be empty", request_id)
            .into_response();
    }
    let known = id == "default"
        || shardlane_host::herdr::list_sessions()
            .unwrap_or_default()
            .iter()
            .any(|session| session.name == id);
    if !known {
        return crate::error::ApiError::not_found(format!("unknown instance: {id}"), request_id)
            .into_response();
    }
    let write = tokio::task::spawn_blocking(move || {
        shardlane_host::herdr::write_session_display_name(&id, &name)
    })
    .await;
    match write {
        Ok(Ok(())) => Json(serde_json::json!({ "ok": true })).into_response(),
        Ok(Err(message)) => crate::error::ApiError::internal(message, request_id).into_response(),
        Err(error) => {
            crate::error::ApiError::internal(format!("rename failed: {error}"), request_id)
                .into_response()
        }
    }
}

pub async fn instance_bootstrap(
    State(state): State<Arc<RemoteState>>,
    Path(id): Path<String>,
) -> Response {
    let request_id = state.next_request_id();
    let known = shardlane_host::herdr::list_sessions()
        .unwrap_or_default()
        .iter()
        .any(|session| session.name == id || (id == "default" && session.is_default));
    if !known {
        return crate::error::ApiError::not_found(format!("unknown instance: {id}"), request_id)
            .into_response();
    }
    let result = crate::server::blocking_projection_scoped(&state, Some(id), |state, instance| {
        crate::bootstrap::build_bootstrap_for(state, instance.as_deref())
    })
    .await;
    match result {
        Ok(bootstrap) => axum::Json(bootstrap).into_response(),
        Err(error) => error.into_response(),
    }
}
