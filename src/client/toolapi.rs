//! Generic tool-invocation API — the client's slice of the data plane.
//!
//! The client exposes the tools it detected
//! (`rsync`, `sanoid`, `syncoid`, `zfs`, `zpool`, `btrfs`, `restic`) as HTTP
//! routes the server can call to steer backups. Execution always happens on
//! this host (it owns the filesystems and the tools); the server only
//! orchestrates by forwarding a plan here.
//!
//!   * `POST /api/tools/:tool` — run `tool <args>` on this host.
//!
//! Arguments are passed as a plain argv vector, **never** through a shell, so
//! there is no command-injection surface beyond the tool itself. The tool name
//! is validated against [`crate::client::tools::KNOWN_TOOLS`] — anything else is
//! rejected, so you cannot e.g. reach `/bin/sh` via the path segment.

use std::time::{Duration, Instant};

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::client::tools::KNOWN_TOOLS;
use crate::client::AppState;

/// Default per-call timeout when a caller does not specify one.
const DEFAULT_TIMEOUT_SECS: u64 = 300;

/// A request to run a tool: the tool name is the route segment, the argv is the body.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ToolCall {
    /// Arguments passed to the tool (argv; no shell is involved).
    pub args: Vec<String>,
    /// Optional timeout in seconds (defaults to 300).
    pub timeout_secs: Option<u64>,
}

impl Default for ToolCall {
    fn default() -> Self {
        Self {
            args: Vec::new(),
            timeout_secs: Some(DEFAULT_TIMEOUT_SECS),
        }
    }
}

/// Run a detected tool on this host. The server forwards its backup plan here.
///
/// The tool name is validated before anything is spawned; a call that runs
/// longer than its timeout is aborted with `408`.
pub async fn run_tool(
    State(_state): State<AppState>,
    Path(tool): Path<String>,
    Json(call): Json<ToolCall>,
) -> Response {
    if !KNOWN_TOOLS.contains(&tool.as_str()) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": format!("unknown tool '{tool}'") })),
        )
            .into_response();
    }

    let timeout = call.timeout_secs.unwrap_or(DEFAULT_TIMEOUT_SECS);
    match tokio::time::timeout(Duration::from_secs(timeout), run(&tool, &call.args)).await {
        Ok(response) => response,
        Err(_) => (
            StatusCode::REQUEST_TIMEOUT,
            Json(serde_json::json!({
                "error": format!("tool '{tool}' timed out after {timeout}s"),
            })),
        )
            .into_response(),
    }
}

/// Run `tool <args>` directly (no shell) and capture stdout/stderr/exit status.
async fn run(tool: &str, args: &[String]) -> Response {
    let start = Instant::now();

    let output = match tokio::process::Command::new(tool).args(args).output().await {
        Ok(o) => o,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": format!("failed to spawn '{tool}': {e}"),
                })),
            )
                .into_response()
        }
    };

    let elapsed_ms = start.elapsed().as_millis() as u64;
    Json(serde_json::json!({
        "tool": tool,
        "exit_code": output.status.code(),
        "success": output.status.success(),
        "elapsed_ms": elapsed_ms,
        "stdout": String::from_utf8_lossy(&output.stdout).to_string(),
        "stderr": String::from_utf8_lossy(&output.stderr).to_string(),
    }))
    .into_response()
}

#[cfg(test)]
mod tests {
    use crate::client::router;
    use crate::client::scan;
    use crate::client::AppState;
    use axum::body::Body;
    use axum::http::StatusCode;
    use std::sync::Arc;
    use tokio::sync::RwLock;
    use tower::util::ServiceExt;

    fn test_state() -> AppState {
        AppState {
            snapshot: Arc::new(RwLock::new(scan::Snapshot {
                inventory: scan::collect("test"),
                up_since: chrono::Utc::now(),
            })),
            client_id: "test".into(),
            version: "test".into(),
        }
    }

    // `echo` is not a KNOWN_TOOL, so the route must reject it before spawning
    // anything. This exercises route matching + the name check without touching
    // real host tooling.
    #[tokio::test]
    async fn unknown_tool_is_rejected() {
        let app = router(test_state());
        let body = Body::from(serde_json::to_vec(&serde_json::json!({ "args": ["hi"] })).unwrap());
        let req = axum::http::Request::builder()
            .method("POST")
            .uri("/api/tools/echo")
            .header("content-type", "application/json")
            .body(body)
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }
}
