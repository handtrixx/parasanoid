//! Client-side snapshot bookkeeping — the *server-side* part of hardlink
//! retention lives on the server (`src/server/plan.rs`), but the destination
//! directory is on the client's host, so the client answers two questions for
//! the server:
//!
//!   * `GET  /api/plan/latest?dest=…` — which snapshots currently exist
//!     (`dest/<ts>/` dirs), newest first, so the server can pass a `--link-dest`.
//!   * `POST /api/plan/prune?dest=…` — delete the oldest snapshots beyond a
//!     `keep` count once a run has succeeded.
//!
//! Snapshots are named `dest/<YYYYmmdd-HHMMSS>/<basename(source)>/`, so this
//! module only treats entries that match that timestamp shape as snapshots.

use std::fs;

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use tracing::warn;

use crate::client::AppState;

#[derive(Debug, Deserialize)]
pub struct DestParam {
    dest: String,
    /// How many recent snapshots to keep. Optional; the server supplies it.
    #[serde(default)]
    keep: u32,
}

/// List `dest/<ts>` snapshots, newest first (lexicographic == chronological for
/// the `YYYYmmdd-HHMMSS` shape).
fn list_snapshots(dest: &str) -> Vec<String> {
    let Ok(entries) = fs::read_dir(dest) else {
        return Vec::new();
    };
    let mut snaps: Vec<String> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|s| is_snapshot(s.as_str()))
        .collect();
    snaps.sort();
    snaps.reverse();
    snaps
}

/// A directory entry is a snapshot if its name is `YYYYmmdd-HHMMSS`.
fn is_snapshot(name: &str) -> bool {
    if name.len() != 15 || name.get(8..9) != Some("-") {
        return false;
    }
    name.chars().all(|c| c.is_ascii_digit() || c == '-')
}

/// Report the newest snapshot dir for a destination (or `null` if none).
pub async fn latest(State(_state): State<AppState>, Query(q): Query<DestParam>) -> Response {
    let dest = q.dest.trim_end_matches('/');
    if dest.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "missing dest" })),
        )
            .into_response();
    }

    let snaps = list_snapshots(dest);
    Json(serde_json::json!({
        "destination": dest,
        "latest": snaps.first().cloned(),
        "count": snaps.len(),
    }))
    .into_response()
}

/// Delete the oldest snapshots beyond `keep`. Returns how many were pruned.
pub async fn prune(State(_state): State<AppState>, Query(q): Query<DestParam>) -> Response {
    let dest = q.dest.trim_end_matches('/');
    let keep = q.keep.max(1);
    if dest.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "missing dest" })),
        )
            .into_response();
    }

    let snaps = list_snapshots(dest);
    let mut pruned: u32 = 0;
    for name in snaps.iter().skip(keep as usize) {
        let path = format!("{dest}/{name}");
        match fs::remove_dir_all(&path) {
            Ok(()) => pruned += 1,
            Err(e) => warn!(path = %path, error = %e, "failed to prune snapshot"),
        }
    }

    Json(serde_json::json!({ "pruned": pruned, "kept": keep })).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_snapshot_names() {
        assert!(is_snapshot("20261017-120000"));
        assert!(!is_snapshot("20261017120000"));
        assert!(!is_snapshot("2026-10-17"));
        assert!(!is_snapshot("not-a-snapshot"));
        assert!(!is_snapshot("20261017"));
    }

    // Regression: the server sends `keep` in the query string (the handler
    // reads it from `Query<DestParam>`). An earlier version sent it in the
    // request body, where it silently parsed to the default 0 -> clamped to 1,
    // collapsing retention to "keep only the newest". This drives the real
    // router to pin that behavior.
    #[tokio::test]
    async fn prune_keeps_requested_count_from_query() {
        use crate::client::router;
        use crate::client::scan;
        use crate::client::AppState;
        use axum::body::{to_bytes, Body};
        use axum::http::StatusCode;
        use std::sync::Arc;
        use tokio::sync::RwLock;
        use tower::util::ServiceExt;

        let tmp = std::env::temp_dir()
            .join(format!("parasanoid-prune-{}", std::process::id()))
            .to_string_lossy()
            .into_owned();
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        for name in [
            "20261017-100000",
            "20261017-100001",
            "20261017-100002",
            "20261017-100003",
        ] {
            std::fs::create_dir_all(format!("{tmp}/{name}")).unwrap();
        }

        let app = router(AppState {
            snapshot: Arc::new(RwLock::new(scan::Snapshot {
                inventory: scan::collect("test"),
                up_since: chrono::Utc::now(),
            })),
            client_id: "test".into(),
            version: "test".into(),
        });

        let resp = app
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri(format!("/api/plan/prune?dest={tmp}&keep=3"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let json: serde_json::Value =
            serde_json::from_slice(&to_bytes(resp.into_body(), usize::MAX).await.unwrap()).unwrap();
        assert_eq!(json["kept"], 3);
        assert_eq!(json["pruned"], 1);
        drop(app);

        let mut remaining: Vec<String> = std::fs::read_dir(&tmp)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|s| is_snapshot(s))
            .collect();
        remaining.sort();
        assert_eq!(
            remaining,
            ["20261017-100001", "20261017-100002", "20261017-100003"]
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
