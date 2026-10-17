//! HTTP API served by the server (what the Next.js UI talks to).
//!
//! Endpoints:
//!   * `GET  /api/health`             — liveness
//!   * `GET  /api/discovery/clients`  — list clients (with latest inventory summary)
//!   * `GET  /api/discovery/clients/{id}`         — client detail
//!   * `GET  /api/discovery/clients/{id}/inventory` — latest inventory payload

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use serde::Serialize;
use std::sync::Arc;
use tracing::info;

use crate::server::plan::{PolicySet, RunStore};
use crate::server::state::Registry;

#[derive(Clone)]
pub struct AppState {
    pub reg: Registry,
    /// Shared HTTP client for dialing out to clients (data plane + discovery).
    pub http: reqwest::Client,
    /// In-memory backup-run history (see `server::plan`).
    pub exec: RunStore,
    /// The loaded backup policy (see `server::plan`). Empty if no policy file.
    pub policy: Arc<PolicySet>,
}

#[derive(Serialize)]
struct ClientSummary {
    id: String,
    name: String,
    url: String,
    up_since: Option<String>,
    created_at: String,
    last_seen: Option<String>,
    last_inventory_taken_at: Option<String>,
}

impl ClientSummary {
    fn from_record(rec: &crate::server::state::ClientRecord) -> Self {
        ClientSummary {
            id: rec.id.clone(),
            name: rec.name.clone(),
            url: rec.url.clone(),
            up_since: rec.up_since.map(|d| d.to_rfc3339()),
            created_at: rec.created_at.to_rfc3339(),
            last_seen: rec.last_seen.map(|d| d.to_rfc3339()),
            last_inventory_taken_at: rec
                .last_inventory
                .as_ref()
                .and_then(|v| v.get("taken_at"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
        }
    }
}

pub async fn health() -> Response {
    Json(serde_json::json!({ "status": "ok" })).into_response()
}

/// Registration payload a client POSTs to announce itself to the server.
///
/// Only `client_id` and `published_url` are used by the server; any other
/// fields a client sends (e.g. a worker self-description) are ignored, so the
/// server struct intentionally does not model them.
#[derive(Debug, Deserialize)]
pub struct Registration {
    /// Stable client id / friendly name (also used as the display name).
    pub client_id: String,
    /// Where the server can reach this client (its published address).
    pub published_url: String,
}

/// A client announces itself so the server can reach it without a static list.
///
/// Called once on client startup and periodically thereafter (the heartbeat).
/// The server upserts the client keyed by `client_id`, using `published_url` as
/// the address its discovery loop polls. Rejects a registration missing either
/// field rather than inserting a half-formed record.
pub async fn register(State(state): State<AppState>, Json(reg): Json<Registration>) -> Response {
    if reg.client_id.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "client_id is required" })),
        )
            .into_response();
    }
    if reg.published_url.trim().is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "published_url is required" })),
        )
            .into_response();
    }

    info!(id = %reg.client_id, url = %reg.published_url, "client registered");
    state
        .reg
        .register(&reg.client_id, &reg.client_id, &reg.published_url)
        .await;
    (
        StatusCode::CREATED,
        Json(serde_json::json!({ "status": "registered", "id": reg.client_id })),
    )
        .into_response()
}

pub async fn clients(State(state): State<AppState>) -> Response {
    let list = state.reg.list().await;
    let summaries: Vec<ClientSummary> = list.iter().map(ClientSummary::from_record).collect();
    Json(serde_json::json!({ "clients": summaries })).into_response()
}

pub async fn client_detail(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    match state.reg.get(&id).await {
        Some(rec) => Json(ClientSummary::from_record(&rec)).into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "unknown client" })),
        )
            .into_response(),
    }
}

pub async fn inventory(State(state): State<AppState>, Path(id): Path<String>) -> Response {
    match state.reg.get(&id).await {
        Some(rec) => match rec.last_inventory {
            Some(value) => Json(value).into_response(),
            None => (StatusCode::NO_CONTENT).into_response(),
        },
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "unknown client" })),
        )
            .into_response(),
    }
}
