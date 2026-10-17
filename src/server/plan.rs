//! M2 — backup policy, plan building, and execution.
//!
//! The server owns the backup policy (the plan) and the executor; every tool
//! that actually touches data still runs on the client. A run proceeds in three
//! phases:
//!
//!   1. **Plan** — expand a [`PolicyEntry`] into an ordered list of
//!      [`ToolCall`]s (an rsync invocation per source).
//!   2. **Execute** — forward each call to the client via
//!      [`crate::server::backup::AppState::forward_tool`] and record the result.
//!   3. **Prune** — once a run has succeeded, ask the client to drop the oldest
//!      snapshots beyond the entry's retention.
//!
//! Hardlink retention mirrors the classic rsnapshot style: each run lands in a
//! `dest/<timestamp>/` directory and links unchanged files against the previous
//! run with `rsync --link-dest`, so a run only costs the bytes that changed.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tracing::{info, warn};
use uuid::Uuid;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;

use crate::client::toolapi::ToolCall;
use crate::server::api::AppState;

/// A loaded policy set: the ordered list of backup entries the server honours.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicySet {
    /// One entry per source tree / destination.
    pub entries: Vec<PolicyEntry>,
}

/// Load and parse a policy set from a JSON file, wrapped in an `Arc` so it can
/// be shared (cheaply cloned) into [`AppState`].
impl PolicySet {
    pub fn load_from_file(path: &str) -> anyhow::Result<Arc<Self>> {
        let data =
            std::fs::read_to_string(path).with_context(|| format!("reading policy file {path}"))?;
        let set: PolicySet =
            serde_json::from_str(&data).with_context(|| format!("parsing policy file {path}"))?;
        Ok(Arc::new(set))
    }
}

/// A single backup target: what to copy, where, and how many versions to keep.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyEntry {
    /// Client (target host) this entry applies to.
    pub client_id: String,
    /// Source paths on the client to back up.
    pub sources: Vec<String>,
    /// Destination dir on the client where snapshots land
    /// (`dest/<timestamp>/<basename(source)>/` per source).
    pub destination: String,
    /// Number of most-recent snapshots to keep.
    pub retention: u32,
    /// Optional per-rsync timeout in seconds (falls back to the client default).
    pub timeout_secs: Option<u64>,
}

/// The result of running a single rsync invocation.
#[derive(Debug, Clone, Serialize)]
pub struct StepResult {
    pub tool: String,
    pub args: Vec<String>,
    pub success: bool,
    pub exit_code: Option<i32>,
    pub elapsed_ms: u64,
    pub stdout: String,
    pub stderr: String,
}

/// The full result of one backup run across a policy's sources.
#[derive(Debug, Clone, Serialize)]
pub struct BackupRun {
    pub run_id: String,
    pub client_id: String,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub steps: Vec<StepResult>,
    pub succeeded: bool,
}

/// In-memory run history, keyed by run id.
#[derive(Clone)]
pub struct RunStore {
    inner: Arc<RwLock<Vec<BackupRun>>>,
}

impl RunStore {
    pub fn new() -> Self {
        RunStore {
            inner: Arc::new(RwLock::new(Vec::new())),
        }
    }

    pub async fn insert(&self, run: BackupRun) {
        self.inner.write().await.push(run);
    }

    pub async fn get(&self, run_id: &str) -> Option<BackupRun> {
        self.inner
            .read()
            .await
            .iter()
            .find(|r| r.run_id == run_id)
            .cloned()
    }

    pub async fn recent(&self, limit: usize) -> Vec<BackupRun> {
        self.inner
            .read()
            .await
            .iter()
            .rev()
            .take(limit)
            .cloned()
            .collect()
    }
}

impl Default for RunStore {
    fn default() -> Self {
        Self::new()
    }
}

/// Expand a policy entry into an ordered list of rsync `ToolCall`s.
///
/// `previous` is the previous snapshot directory (the whole `dest/<ts>`), or
/// `None` on the first run. Every source gets one rsync invocation under a
/// shared `dest/<ts>/<basename(source)>/` layout; the sources are kept in policy
/// order so the plan is deterministic.
pub fn build_sync_plan(
    entry: &PolicyEntry,
    previous: Option<&str>,
    snapshot: &str,
) -> Vec<ToolCall> {
    entry
        .sources
        .iter()
        .map(|src| {
            let base = basename(src);
            // `--link-dest` must mirror where this source lives in the new
            // snapshot (dest/<ts>/<base>), so point at the previous run's same
            // path. rsync resolves it relative to the destination, hence the
            // shared destination prefix.
            let link_dest = previous.map(|p| format!("{p}/{base}"));
            let dest = format!(
                "{}/{}/{}",
                entry.destination.trim_end_matches('/'),
                snapshot,
                base
            );

            let mut args: Vec<String> = vec![
                "-a".into(),
                "--delete".into(),
                "--stats".into(),
                "--human-readable".into(),
            ];
            if let Some(ld) = link_dest {
                args.push("--link-dest".into());
                args.push(ld);
            }
            // Trailing slash on the source: rsync copies the *contents* of the
            // directory into the destination (`<ts>/<base>`), not a nested copy
            // at `<ts>/<base>/<base>`. It also lets `--link-dest` see the
            // previous contents, which is what makes the hardlinks work.
            args.push(format!("{}/", src.trim_end_matches('/')));
            args.push(dest);

            ToolCall {
                args,
                timeout_secs: entry.timeout_secs,
            }
        })
        .collect()
}

/// Take the last path component of a source (handles trailing slashes).
fn basename(path: &str) -> String {
    path.trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or("")
        .to_string()
}

/// `dest/<ts>` snapshot dir naming: `YYYYmmdd-HHMMSS` (UTC).
fn utc_snapshot() -> String {
    Utc::now().format("%Y%m%d-%H%M%S").to_string()
}

/// Run every policy that targets `client_id`.
///
/// Returns the stored [`BackupRun`]. Returns `None` if the client is unknown or
/// has no `rsync` in its last inventory — nothing to execute against.
pub async fn run_policy(state: &AppState, entry: &PolicyEntry) -> Option<(String, BackupRun)> {
    let rec = match state.reg.get(&entry.client_id).await {
        Some(rec) => rec,
        None => {
            warn!(client_id = %entry.client_id, "run: unknown client");
            return None;
        }
    };

    if !client_has_tool(&rec, "rsync") {
        warn!(client_id = %entry.client_id, "run: client has no rsync in inventory");
        return None;
    }

    let snapshot = utc_snapshot();
    let previous = state.latest_snapshot(&rec.id, &entry.destination).await;
    let plan = build_sync_plan(entry, previous.as_deref(), &snapshot);

    info!(
        client_id = %entry.client_id,
        sources = plan.len(),
        snapshot = %snapshot,
        previous = previous.as_deref().unwrap_or("<none>"),
        "backup run started"
    );

    let run_id = Uuid::new_v4().to_string();
    let started_at = Utc::now();
    let mut steps: Vec<StepResult> = Vec::with_capacity(plan.len());
    let mut succeeded = true;

    for call in &plan {
        match state.forward_tool(&rec.id, "rsync", call).await {
            Ok(body) => {
                let step = parse_step(call, &body);
                if !step.success {
                    succeeded = false;
                    warn!(
                        client_id = %entry.client_id,
                        tool = %step.tool,
                        exit_code = ?step.exit_code,
                        "backup step failed",
                    );
                }
                steps.push(step);
            }
            Err(status) => {
                succeeded = false;
                warn!(
                    client_id = %entry.client_id,
                    status = ?status,
                    "backup step rejected",
                );
                // A call rejected before execution (unknown tool / missing
                // inventory) means the whole run is not trustworthy.
                break;
            }
        }
    }

    if succeeded {
        match state
            .prune_snapshots(&rec.id, &entry.destination, entry.retention)
            .await
        {
            Ok(pruned) => {
                info!(client_id = %entry.client_id, pruned, "backup run pruned old snapshots");
            }
            Err(e) => {
                warn!(error = %e, "snapshot pruning failed after successful backup");
            }
        }
    }

    let ended_at = Utc::now();
    let run = BackupRun {
        run_id: run_id.clone(),
        client_id: entry.client_id.clone(),
        started_at,
        ended_at: Some(ended_at),
        steps,
        succeeded,
    };
    state.exec.insert(run.clone()).await;
    Some((run_id, run))
}

/// Run *all* policies, grouped by client.
pub async fn run_all(state: &AppState, policy: &PolicySet) -> Vec<(String, BackupRun)> {
    let mut out = Vec::new();
    for entry in &policy.entries {
        if let Some(pair) = run_policy(state, entry).await {
            out.push(pair);
        }
    }
    out
}

/// Whether the client's last inventory lists `tool` as available.
fn client_has_tool(rec: &crate::server::state::ClientRecord, tool: &str) -> bool {
    rec.last_inventory
        .as_ref()
        .and_then(|v| v.get("tools"))
        .and_then(|t| t.as_array())
        .is_some_and(|tools| {
            tools
                .iter()
                .any(|t| t.get("name").and_then(|n| n.as_str()) == Some(tool))
        })
}

/// Pull the per-step envelope the client's `/api/tools/rsync` returns.
fn parse_step(call: &ToolCall, body: &serde_json::Value) -> StepResult {
    StepResult {
        tool: body
            .get("tool")
            .and_then(|v| v.as_str())
            .unwrap_or("rsync")
            .to_string(),
        args: call.args.clone(),
        success: body
            .get("success")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
        exit_code: body
            .get("exit_code")
            .and_then(|v| v.as_i64())
            .map(|v| v as i32),
        elapsed_ms: body.get("elapsed_ms").and_then(|v| v.as_u64()).unwrap_or(0),
        stdout: body
            .get("stdout")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        stderr: body
            .get("stderr")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
    }
}

/// Ask a client which snapshots currently exist under `dest`, returning the
/// newest (`dest/<ts>`) or `None` on the first run / missing destination.
impl AppState {
    pub async fn latest_snapshot(&self, client_id: &str, dest: &str) -> Option<String> {
        let rec = self.reg.get(client_id).await?;
        let base = rec.url.trim_end_matches('/');
        let req = self
            .http
            .get(format!("{}/api/plan/latest", base))
            .query(&[("dest", dest)]);
        match req.send().await {
            Ok(resp) if resp.status().is_success() => {
                let v = resp.json::<serde_json::Value>().await.ok()?;
                let name = v.get("latest").and_then(|x| x.as_str());
                // Return the full `dest/<ts>` path, not just the ts name: the
                // caller builds an absolute `--link-dest` from it, and rsync
                // resolves that link-dest relative to the *new* snapshot's
                // destination, so a bare name would point at the wrong place.
                name.map(|n| format!("{}/{}", dest.trim_end_matches('/'), n))
            }
            Ok(resp) => {
                warn!(
                    client_id = %client_id,
                    status = %resp.status(),
                    "snapshot discovery returned an error",
                );
                None
            }
            Err(e) => {
                warn!(client_id = %client_id, error = %e, "failed to discover snapshots");
                None
            }
        }
    }

    /// Delete snapshots beyond `keep` on the client. Returns how many were
    /// deleted.
    pub async fn prune_snapshots(
        &self,
        client_id: &str,
        dest: &str,
        keep: u32,
    ) -> Result<u32, String> {
        let rec = self
            .reg
            .get(client_id)
            .await
            .ok_or_else(|| "unknown client".to_string())?;
        let base = rec.url.trim_end_matches('/');
        // Both `dest` and `keep` go in the query string: the client's `prune`
        // handler reads it from `Query<DestParam>`, so `keep` would otherwise
        // arrive only in the JSON body and deserialize to its `0` default
        // (clamped to 1), silently collapsing retention to "keep newest only".
        let keep_str = keep.to_string();
        let req = self
            .http
            .post(format!("{}/api/plan/prune", base))
            .query(&[("dest", dest), ("keep", keep_str.as_str())]);
        match req.send().await {
            Ok(resp) if resp.status().is_success() => {
                let v = resp
                    .json::<serde_json::Value>()
                    .await
                    .map_err(|e| e.to_string())?;
                Ok(v.get("pruned").and_then(|x| x.as_u64()).unwrap_or(0) as u32)
            }
            Ok(resp) => Err(format!("client returned {}", resp.status())),
            Err(e) => Err(e.to_string()),
        }
    }
}

/// How long a background executor would wait between runs, before the first.
#[allow(dead_code)]
fn default_interval() -> Duration {
    Duration::from_secs(60)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry() -> PolicyEntry {
        PolicyEntry {
            client_id: "c1".into(),
            sources: vec!["/data/a".into(), "/data/b/".into()],
            destination: "/backup".into(),
            retention: 3,
            timeout_secs: None,
        }
    }

    #[test]
    fn first_run_is_a_plain_copy() {
        let calls = build_sync_plan(&entry(), None, "20261017-120000");
        assert_eq!(calls.len(), 2);
        // No --link-dest on the first run.
        assert!(!calls[0].args.contains(&"--link-dest".to_string()));
        let expected: Vec<String> = vec![
            "-a".into(),
            "--delete".into(),
            "--stats".into(),
            "--human-readable".into(),
            "/data/a/".into(),
            "/backup/20261017-120000/a".into(),
        ];
        assert_eq!(calls[0].args, expected);
        // Trailing slash on the source is normalised away in the destination,
        // but kept on the source arg (rsync needs it). The second source is
        // `/data/b/` -> `/<ts>/b`.
        assert_eq!(calls[1].args[4], "/data/b/");
        assert_eq!(calls[1].args.last().unwrap(), "/backup/20261017-120000/b");
    }

    #[test]
    fn second_run_links_against_previous_snapshot() {
        let calls = build_sync_plan(&entry(), Some("/backup/20261017-120000"), "20261017-130000");
        assert_eq!(calls.len(), 2);
        let expected: Vec<String> = vec![
            "-a".into(),
            "--delete".into(),
            "--stats".into(),
            "--human-readable".into(),
            "--link-dest".into(),
            "/backup/20261017-120000/a".into(),
            "/data/a/".into(),
            "/backup/20261017-130000/a".into(),
        ];
        assert_eq!(calls[0].args, expected);
        // The second source links against the previous run's `/data/b` copy.
        assert!(calls[1].args.contains(&"--link-dest".to_string()));
    }

    #[test]
    fn retention_is_modelled_as_a_count() {
        let e = entry();
        assert_eq!(e.retention, 3);
    }

    #[test]
    fn basename_ignores_trailing_slash() {
        assert_eq!(basename("/data/a/"), "a");
        assert_eq!(basename("/data/a"), "a");
        assert_eq!(basename("/data"), "data");
    }
}

// --- Control plane: trigger and inspect backup runs ---

/// Run every policy entry that applies to a known client.
///
/// Returns the runs that actually executed (an entry targeting an unknown client
/// or one without rsync is skipped). Stored in [`RunStore`] for later inspection.
pub async fn backup_run(State(state): State<AppState>) -> Response {
    let runs = run_all(&state, &state.policy).await;
    let ids: Vec<String> = runs.iter().map(|(id, _)| id.clone()).collect();
    if runs.is_empty() {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": "no applicable policies (none configured, or no known client with rsync)"
            })),
        )
            .into_response();
    }
    info!(runs = ?ids, "backup run triggered");
    Json(serde_json::json!({ "runs": ids })).into_response()
}

/// List the most recent runs (newest first), capped at `?limit=N`.
pub async fn backup_runs(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> Response {
    let limit = params
        .get("limit")
        .and_then(|v| v.parse().ok())
        .unwrap_or(50)
        .min(500)
        .max(1);
    Json(serde_json::json!({ "runs": state.exec.recent(limit).await })).into_response()
}

/// A single stored run by id.
pub async fn backup_run_detail(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
) -> Response {
    match state.exec.get(&run_id).await {
        Some(run) => Json(run).into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "unknown run" })),
        )
            .into_response(),
    }
}
