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
    /// Backend that serves this instance (e.g. "herdr", "tmux"). Non-Herdr
    /// instances carry capability degradations (docs/multiplexer-api.md §7).
    pub backend: String,
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
    /// Best-effort live agent count for this instance (0 when the instance is
    /// stopped, unreachable, or the backend has no agent capability).
    pub agent_count: usize,
}

/// The wire id for a registry listing: Herdr instances keep their bare
/// session name (wire compatibility with the pinned multi-instance fixtures);
/// other backends are backend-qualified ("tmux:default") so two backends can
/// never collide on one id.
pub(crate) fn qualified_instance_id(listing: &shardlane_host::mux::InstanceListing) -> String {
    if listing.backend == "herdr" {
        listing.name.clone()
    } else {
        format!("{}:{}", listing.backend, listing.name)
    }
}

pub async fn list_instances(State(state): State<Arc<RemoteState>>) -> Response {
    // Workspaces ARE backend instances: enumerate from the backend registry
    // (no Shardlane-side registry). Display names come from the Shardlane-
    // owned per-instance override, else the raw instance name.
    let registry = state.mux_registry.clone();
    let listings = tokio::task::spawn_blocking(move || registry.list_instances())
        .await
        .unwrap_or_default();
    // Per-instance live agent counts, probed in parallel (picker-facing data:
    // "which workspace has agents" is a quick-entry affordance).
    let count_handles: Vec<_> = listings
        .iter()
        .map(|listing| {
            let state = state.clone();
            let id = qualified_instance_id(listing);
            tokio::task::spawn_blocking(move || {
                crate::bootstrap::connect_instance_for(&state, Some(&id))
                    .ok()
                    .and_then(|connection| connection.host_bootstrap_state().ok())
                    .map(|snapshot| snapshot.agents.len())
                    .unwrap_or(0)
            })
        })
        .collect();
    let counts = futures_util::future::join_all(count_handles)
        .await
        .into_iter()
        .map(|joined| joined.unwrap_or(0))
        .collect::<Vec<usize>>();
    let instances: Vec<InstanceSummary> = listings
        .iter()
        .enumerate()
        .map(|(index, listing)| {
            let display_name = listing
                .display_name
                .clone()
                .unwrap_or_else(|| listing.name.clone());
            let id = qualified_instance_id(listing);
            // Non-Herdr rows carry a qualified name too, so the label alone
            // tells the two backends apart (desktop picker parity).
            let name = if listing.backend == "herdr" {
                listing.name.clone()
            } else {
                id.clone()
            };
            InstanceSummary {
                id,
                name,
                backend: listing.backend.clone(),
                display_name,
                session: listing.name.clone(),
                device_id: None,
                adopted: false,
                running: listing.running,
                is_default: listing.is_default,
                remote_api_version: crate::config::REMOTE_API_VERSION,
                agent_count: counts[index],
            }
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
    let registry = state.mux_registry.clone();
    let probe_id = id.clone();
    let known = tokio::task::spawn_blocking(move || {
        probe_id == "default"
            || registry.list_instances().iter().any(|listing| {
                qualified_instance_id(listing) == probe_id || listing.name == probe_id
            })
    })
    .await
    .unwrap_or_default();
    if !known {
        return crate::error::ApiError::not_found(format!("unknown instance: {id}"), request_id)
            .into_response();
    }
    let registry = state.mux_registry.clone();
    let write = tokio::task::spawn_blocking(move || {
        // Route the rename to the listing's own backend (qualified ids select
        // the backend; bare herdr ids keep the wire-compatible path).
        let listing = registry
            .list_instances()
            .into_iter()
            .find(|listing| qualified_instance_id(listing) == id || listing.name == id)
            .ok_or_else(|| format!("unknown instance: {id}"))?;
        registry
            .backend(&listing.backend)
            .ok_or_else(|| format!("backend {:?} is not registered", listing.backend))
            .and_then(|backend| {
                backend
                    .rename_instance(&listing.name, &name)
                    .map_err(|e| e.to_string())
            })
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
    let registry = state.mux_registry.clone();
    let probe_id = id.clone();
    let known = tokio::task::spawn_blocking(move || {
        registry.list_instances().iter().any(|listing| {
            listing.name == probe_id || (probe_id == "default" && listing.is_default)
        })
    })
    .await
    .unwrap_or_default();
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
