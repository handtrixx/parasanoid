//! HTTP API exposed by the worker in the client role — the "bridge" the server
//! connects to.
//!
//! Endpoints:
//!   * `GET  /api/health`      — liveness + last inventory time
//!   * `GET  /api/inventory`   — the latest host inventory
//!   * `GET  /api/status`      — worker self-description (role, version, tools)
//!   * `POST /api/scan`        — force a fresh host scan (server-driven)
//!
//! The data plane (snapshot + stream host data to the server) is intentionally
//! *not* implemented yet: it returns `501` with a clear marker so the control
//! surface exists without pretending to work. See `TODO(data-plane)`.

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;

use crate::client::AppState;

/// Liveness. The server polls this to decide a client is reachable.
pub async fn health(State(state): State<AppState>) -> Response {
    let snap = state.snapshot.read().await;
    Json(serde_json::to_value(snap.status()).expect("status serializes")).into_response()
}

/// Latest host inventory. The server calls this during discovery polling.
pub async fn inventory(State(state): State<AppState>) -> Response {
    let snap = state.snapshot.read().await;
    Json(serde_json::to_value(&snap.inventory).expect("inventory serializes")).into_response()
}

/// Worker self-description used by the server's bridge handshake.
pub async fn status(State(state): State<AppState>) -> Response {
    let snap = state.snapshot.read().await;
    let st = snap.worker_status(&state.version);
    Json(serde_json::to_value(st).expect("status serializes")).into_response()
}

/// Force a fresh host scan. Returns the new inventory.
pub async fn refresh(State(state): State<AppState>) -> Response {
    let client_id = state.client_id.clone();
    let up_since = { state.snapshot.read().await.up_since };
    let inventory = crate::client::scan::collect(&client_id);
    {
        let mut snap = state.snapshot.write().await;
        *snap = crate::client::scan::Snapshot {
            inventory: inventory.clone(),
            up_since,
        };
    }
    Json(serde_json::to_value(&inventory).expect("inventory serializes")).into_response()
}

/// Placeholder for the data plane. Implementing snapshot/stream here means the
/// client role starts doing real backup work — which the design keeps on the
/// server. Until then, refuse politely.
pub async fn not_implemented() -> Response {
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(serde_json::json!({
            "error": "data plane (snapshot + stream) not implemented yet",
        })),
    )
        .into_response()
}

/// Generic JSON error response.
pub(crate) fn json_error(msg: &str) -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({ "error": msg })),
    )
        .into_response()
}
