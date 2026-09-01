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

/// One row of `GET /api/v2/instances`.
#[derive(Debug, Serialize)]
pub struct InstanceSummary {
    pub id: String,
    pub name: String,
    /// Herdr session name; `None` = the default instance.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_id: Option<String>,
    /// Always false: instances are Herdr-owned, never Shardlane-maintained.
    pub adopted: bool,
    /// Cosmetic display name (desktop rename override; falls back to the
    /// session name / "Default").
    pub display_name: String,
    /// Whether the instance's socket answers a ping right now.
    pub running: bool,
    /// Convenience flag for the default instance (session == None).
    pub is_default: bool,
    /// Remote API version (mirrors hello; convenience for clients that only
    /// call this endpoint first).
    pub remote_api_version: u32,
}

/// Reads the desktop's display-name overrides from the injected settings file.
fn load_display_names(path: &std::path::Path) -> std::collections::HashMap<String, String> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|json| serde_json::from_str::<serde_json::Value>(&json).ok())
        .and_then(|config| config.get("instance_display_names").cloned())
        .and_then(|value| serde_json::from_value(value).ok())
        .unwrap_or_default()
}

/// Cosmetic display name: override, else "Default", else the session name.
fn display_name_for(
    overrides: &std::collections::HashMap<String, String>,
    session: Option<&str>,
) -> String {
    let key = session.unwrap_or("default");
    overrides.get(key).cloned().unwrap_or_else(|| {
        if key == "default" {
            "Default".to_string()
        } else {
            key.to_string()
        }
    })
}

pub async fn list_instances(State(state): State<Arc<RemoteState>>) -> Response {
    let settings_path = state.settings_path.clone();
    // Workspaces ARE Herdr instances: enumerate from the CLI (no
    // Shardlane-side registry). Display names come from the desktop's
    // settings overrides.
    let (sessions, overrides) = tokio::task::spawn_blocking(move || {
        let overrides = load_display_names(&settings_path);
        let sessions = shardlane_host::herdr::list_sessions().unwrap_or_default();
        (sessions, overrides)
    })
    .await
    .unwrap_or_default();
    let instances: Vec<InstanceSummary> = sessions
        .iter()
        .map(|session| {
            let session_name = (!session.is_default).then(|| session.name.clone());
            InstanceSummary {
                id: session.name.clone(),
                name: session.name.clone(),
                display_name: display_name_for(&overrides, session_name.as_deref()),
                session: session_name,
                device_id: None,
                adopted: false,
                running: session.running,
                is_default: session.is_default,
                remote_api_version: crate::config::REMOTE_API_VERSION,
            }
        })
        .collect();
    Json(serde_json::json!({ "instances": instances })).into_response()
}

/// `POST /api/v2/instances/{id}/rename {name}`: writes the desktop's
/// display-name override (cosmetic rename; herdr sessions have no rename).
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
    let settings_path = state.settings_path.clone();
    let write = tokio::task::spawn_blocking(move || {
        let json = std::fs::read_to_string(&settings_path).map_err(|e| e.to_string())?;
        let mut config: serde_json::Value =
            serde_json::from_str(&json).map_err(|e| e.to_string())?;
        let object = config
            .as_object_mut()
            .ok_or_else(|| "config is not a JSON object".to_string())?;
        let overrides = object
            .entry("instance_display_names")
            .or_insert_with(|| serde_json::Value::Object(Default::default()));
        overrides
            .as_object_mut()
            .ok_or_else(|| "instance_display_names is not an object".to_string())?
            .insert(id, serde_json::Value::String(name));
        let pretty = serde_json::to_string_pretty(&config).map_err(|e| e.to_string())?;
        std::fs::write(&settings_path, pretty).map_err(|e| e.to_string())
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
