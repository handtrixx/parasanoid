//! Server-side data-plane orchestration: the server forwards backup tool calls
//! to the clients that can run them.
//!
//! The client exposes `POST /api/tools/:tool`; this module calls it. That is
//! the "server steers the backup" half of the data plane — a higher layer builds
//! a [`ToolCall`] (which tool, which argv) from a client's inventory and a
//! backup policy, and this module just ships it to the right client.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::Value;

use crate::client::toolapi::ToolCall;
use crate::server::api::AppState;

/// Forward a tool call to a client and return its result as JSON.
///
/// The client's published URL is looked up in the live registry. If the client
/// has reported an inventory, the tool must be present and usable on that host —
/// we reject the call early rather than asking a client to run a tool it lacks.
pub async fn forward_tool_call(
    State(state): State<AppState>,
    Path(client_id): Path<String>,
    Path(tool): Path<String>,
    Json(call): Json<ToolCall>,
) -> Response {
    let rec = match state.reg.get(&client_id).await {
        Some(rec) => rec,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": "unknown client" })),
            )
                .into_response();
        }
    };

    if let Some(inv) = &rec.last_inventory {
        if !tool_available_in_inventory(inv, &tool) {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": format!("tool '{tool}' not available on client '{}'", rec.id)
                })),
            )
                .into_response();
        }
    }

    let url = format!("{}/api/tools/{tool}", rec.url.trim_end_matches('/'));

    match state.http.post(&url).json(&call).send().await {
        Ok(resp) if resp.status().is_success() => match resp.json::<Value>().await {
            Ok(body) => Json(body).into_response(),
            Err(e) => bad_gateway(&rec.id, format!("invalid response from client: {e}")),
        },
        Ok(resp) => bad_gateway(&rec.id, format!("client returned status {}", resp.status())),
        Err(e) => bad_gateway(&rec.id, format!("failed to reach client '{}': {e}", rec.id)),
    }
}

/// Whether `tool` is listed as available in an inventory JSON payload.
///
/// Returns `false` if the inventory has no tools list, so a caller can treat
/// "no tools reported" the same as "tool not available".
pub(crate) fn tool_available_in_inventory(inventory: &serde_json::Value, tool: &str) -> bool {
    inventory
        .get("tools")
        .and_then(|t| t.as_array())
        .map(|tools| {
            tools.iter().any(|t| {
                t.get("name").and_then(|n| n.as_str()) == Some(tool)
                    && t.get("available").and_then(|a| a.as_bool()) == Some(true)
            })
        })
        .unwrap_or(false)
}

fn bad_gateway(id: &str, msg: String) -> Response {
    (
        StatusCode::BAD_GATEWAY,
        Json(serde_json::json!({ "error": format!("client '{id}': {msg}") })),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::state::Registry;
    use axum::extract::{Path, State};

    fn state_with_registry() -> (AppState, Registry) {
        let reg = Registry::new();
        let state = AppState {
            reg: reg.clone(),
            http: reqwest::Client::new(),
        };
        (state, reg)
    }

    #[test]
    fn availability_checks_inventory() {
        let inv = serde_json::json!({
            "tools": [
                { "name": "rsync", "available": true },
                { "name": "zfs", "available": false }
            ]
        });
        assert!(tool_available_in_inventory(&inv, "rsync"));
        assert!(!tool_available_in_inventory(&inv, "zfs"));
        assert!(!tool_available_in_inventory(&inv, "restic"));
        // No tools list at all -> treat as unavailable.
        assert!(!tool_available_in_inventory(
            &serde_json::json!({}),
            "rsync"
        ));
    }

    #[tokio::test]
    async fn unknown_client_returns_404() {
        let (state, _reg) = state_with_registry();
        let call = ToolCall {
            args: vec!["--version".into()],
            timeout_secs: None,
        };
        let resp = forward_tool_call(
            State(state),
            Path("missing".into()),
            Path("rsync".into()),
            Json(call),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn missing_tool_in_inventory_returns_400() {
        let (state, reg) = state_with_registry();
        // Register the client first (the 404 path requires it to exist); the
        // 400 path short-circuits before any network call, so the URL is never
        // actually hit.
        reg.register("c1", "c1", "http://127.0.0.1:0").await;
        reg.set_inventory(
            "c1",
            serde_json::json!({ "tools": [{ "name": "rsync", "available": true }] }),
        )
        .await;

        let call = ToolCall {
            args: vec![],
            timeout_secs: None,
        };
        let resp = forward_tool_call(
            State(state),
            Path("c1".into()),
            Path("zfs".into()),
            Json(call),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }
}
