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
use serde::Serialize;

use crate::server::state::Registry;

#[derive(Clone)]
pub struct AppState {
    pub reg: Registry,
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
