//! Throwaway prototype — a Rust *server-rendered* UI process for parasanoid.
//!
//! Design questions it answers (so we can compare DX against the Next.js UI):
//!   * SQLite lives in the UI process (`sqlx`, `Send`+async) — never the worker.
//!   * HTML is rendered server-side with `askama` templates (compile-time checked).
//!   * The UI dials out to the paranoid server's JSON API and caches results in
//!     SQLite, then serves Bootstrap 6 as static assets.
//!
//! It is deliberately self-contained and separate from the worker crate — the
//! "worker stays a dumb bridge" boundary is preserved.

mod api;
mod db;
mod models;

use std::collections::HashMap;
use std::io::Read;
use std::sync::Arc;

use askama::Template;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{Html, IntoResponse, Json, Response},
    routing::get,
    Router,
};
use serde_json::json;
use sqlx::SqlitePool;
use tokio::time::{interval, Duration};
use tracing::{info, warn};

#[derive(Clone)]
struct AppState {
    pool: SqlitePool,
    server_url: String,
    http: reqwest::Client,
    /// Path to the server process log, passed via `PARASANOID_SERVER_LOG`. The
    /// Logs page tails this file (see `/api/logs`). Empty if not configured
    /// (e.g. standalone `cargo run` without start-dev.sh).
    log_file: Option<String>,
}

const REFRESH_SECS: u64 = 10;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "parasanoid_ui=debug,info".into()),
        )
        .init();

    let listen = std::env::var("PARASANOID_UI_LISTEN").unwrap_or_else(|_| ":3100".into());
    let server_url = std::env::var("PARASANOID_SERVER_URL")
        .unwrap_or_else(|_| "http://localhost:8080".into())
        .trim_end_matches('/')
        .to_string();
    let db_path = std::env::var("PARASANOID_DB_PATH").unwrap_or_else(|_| "data.db".into());
    // Optional: absolute path to the server's log file (from start-dev.sh).
    let log_file = std::env::var("PARASANOID_SERVER_LOG")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let pool = db::open(&db_path).await?;
    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()?;
    let state = Arc::new(AppState {
        pool: pool.clone(),
        server_url: server_url.clone(),
        http: http.clone(),
        log_file: log_file.clone(),
    });

    // Seed once up front so the first page isn't empty even before the ticker.
    refresh(&state).await;
    let mut ticker = interval(Duration::from_secs(REFRESH_SECS));
    let st = state.clone();
    tokio::spawn(async move {
        loop {
            ticker.tick().await;
            refresh(&st).await;
        }
    });

    let assets_dir = std::path::PathBuf::from(
        std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".into()),
    )
    .join("assets");
    let app = Router::new()
        // Pages.
        .route("/", get(dashboard))
        .route("/activities", get(activities))
        .route("/backups", get(backups))
        .route("/logs", get(logs))
        // Kept for continuity: the client detail route is reachable from the
        // dashboard client list.
        .route("/clients/:id", get(client_detail))
        .route("/health", get(health))
        // Control plane: trigger all backups, then return the queued run ids.
        .route("/api/backup/run", get(run_backups))
        // Server-log tail (JSON) — polled by the Logs page for live follow.
        .route("/api/logs", get(api_logs))
        .nest_service(
            "/assets",
            tower_http::services::fs::ServeDir::new(assets_dir),
        )
        .with_state(state)
        .layer(tower_http::trace::TraceLayer::new_for_http());

    info!(
        listen = %listen,
        server = %server_url,
        log = %log_file.as_deref().unwrap_or("<none>"),
        "parasanoid-ui-rust starting"
    );
    let listener = tokio::net::TcpListener::bind(&listen).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

/// One discovery cycle: fetch clients + backup runs from the server, mirror
/// both into SQLite. Best-effort — a server blip keeps whatever we cached.
async fn refresh(state: &AppState) {
    match api::fetch_clients(&state.server_url, &state.http).await {
        Ok(list) => {
            let ids: Vec<String> = list.iter().map(|c| c.id.clone()).collect();
            for c in &list {
                if let Err(e) = db::upsert_client(&state.pool, c).await {
                    warn!(client = %c.id, error = %e, "upsert client failed");
                }
            }
            db::touch_stale(&state.pool, &ids).await.ok();
            // Pull inventory for any client we don't have cached yet.
            let to_fetch = client_needing_inventory(&state.pool, &ids).await;
            for id in &to_fetch {
                match api::fetch_inventory(&state.server_url, id, &state.http).await {
                    Ok(inv) => {
                        if let Err(e) = db::set_inventory(&state.pool, id, &inv).await {
                            warn!(client = %id, error = %e, "set inventory failed");
                        }
                    }
                    Err(e) => info!(client = %id, error = %e, "no inventory yet (skipped)"),
                }
            }
            db::prune_inventory(&state.pool, &ids).await.ok();
            info!(count = ids.len(), "discovery refresh complete");
        }
        Err(e) => warn!(error = %e, "discovery refresh failed; keeping cached data"),
    }

    // Backup runs: fetch recent, upsert only the ones we don't have yet, prune
    // past the retention ceiling. The server records a run only after it
    // finishes, so a completed run shows up within one refresh cycle.
    if let Err(e) = refresh_runs(&state).await {
        warn!(error = %e, "run refresh failed; keeping cached runs");
    }
}

/// Mirror the server's recent backup runs into the local cache.
async fn refresh_runs(state: &AppState) -> anyhow::Result<()> {
    let runs = api::fetch_runs(&state.server_url, &state.http, 50).await?;
    if runs.is_empty() {
        return Ok(());
    }
    let ids: Vec<String> = runs.iter().map(|r| r.run_id.clone()).collect();
    let new_ids = db::new_run_ids(&state.pool, &ids).await;
    let mut n = 0;
    for r in &runs {
        if new_ids.contains(&r.run_id) && db::upsert_run(&state.pool, r).await.is_ok() {
            n += 1;
        }
    }
    db::prune_runs(&state.pool).await.ok();
    info!(new = n, "run refresh complete");
    Ok(())
}

/// Which known client ids still lack a cached inventory snapshot.
async fn client_needing_inventory(pool: &SqlitePool, ids: &[String]) -> Vec<String> {
    if ids.is_empty() {
        return Vec::new();
    }
    let sql = format!(
        "SELECT id FROM clients WHERE id IN ({placeholders}) \n         AND id NOT IN (SELECT client_id FROM inventory)",
        placeholders = ids.iter().map(|_| "?").collect::<Vec<_>>().join(",")
    );
    let mut q = sqlx::query_as::<_, (String,)>(&sql);
    for id in ids {
        q = q.bind(id.as_str());
    }
    let rows: Vec<(String,)> = q.fetch_all(pool).await.unwrap_or_default();
    rows.into_iter().map(|(s,)| s).collect()
}

/// Human-readable uptime from seconds.
fn uptime_human(secs: &u64) -> String {
    let secs = *secs;
    let d = secs / 86_400;
    let h = (secs % 86_400) / 3_600;
    if d > 0 {
        format!("{d}d {h}h")
    } else if h > 0 {
        format!("{h}h")
    } else {
        format!("{}m", secs / 60)
    }
}

/// Format a byte count as B/KB/MB/GB/TB.
fn format_bytes(bytes: &u64) -> String {
    let bytes = *bytes;
    const U: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = bytes as f64;
    let mut i = 0;
    while v >= 1024.0 && i < U.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{bytes} {}", U[0])
    } else {
        format!("{v:.1} {}", U[i])
    }
}

/// Relative label for an RFC3339 timestamp (best-effort; returns raw value if
/// unparsable).
fn relative(rfc: Option<&str>) -> String {
    let Some(s) = rfc else {
        return "n/a".into();
    };
    let Some(dt) = chrono::DateTime::parse_from_rfc3339(s).ok() else {
        return s.into();
    };
    let now = chrono::Utc::now().with_timezone(&dt.timezone());
    let delta = (now - dt).num_seconds();
    let secs = delta.abs();
    let unit = if delta < 0 { "in" } else { "ago" };
    let value = match secs {
        n if n < 60 => format!("{n}s"),
        n => match n / 60 {
            m if m < 60 => format!("{m}m"),
            h => match h / 24 {
                d if d == 0 => format!("{h}h"),
                d => format!("{d}d"),
            },
        },
    };
    format!("{unit} {value}")
}

/// Duration between two RFC3339 stamps, human-readable (e.g. "3m12s").
/// Missing/unparsable ends render as a "-" placeholder.
fn run_duration_human(started_at: Option<&str>, ended_at: Option<&str>) -> String {
    let (Some(start), Some(end)) = (
        started_at.and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok()),
        ended_at.and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok()),
    ) else {
        return "-".into();
    };
    let secs = (end - start).num_seconds().max(0);
    let (h, m, s) = (secs / 3600, (secs % 3600) / 60, secs % 60);
    if h > 0 {
        format!("{h}h{m}m{s}s")
    } else if m > 0 {
        format!("{m}m{s}s")
    } else {
        format!("{s}s")
    }
}

/// Render a run's step count with correct pluralization, e.g. "1 step", "3 steps".
/// Exposed to askama templates as `crate::run_step_label`.
fn run_step_label(n: usize) -> String {
    format!("{} {}", n, if n == 1 { "step" } else { "steps" })
}

/// Render a run's operation count with correct pluralization, e.g. "1 op", "4 ops".
/// Exposed to askama templates as `crate::run_op_label`.
fn run_op_label(n: usize) -> String {
    format!("{} {}", n, if n == 1 { "op" } else { "ops" })
}

/// Format a millisecond duration (per-step timing) human-readably, e.g. 1540ms
/// → "1.5s", 900ms → "0.9s", 42ms → "42ms". Takes `&u64` so askama can pass the
/// per-step field (`s.elapsed_ms`) by reference (askama borrows field accesses).
fn elapsed_human(ms: &u64) -> String {
    let ms = *ms;
    if ms < 1000 {
        format!("{}ms", ms)
    } else {
        format!("{:.1}s", ms as f64 / 1000.0)
    }
}

/// Strip ANSI color/escape sequences from a log line (server logs are tagged
/// with color codes; the JSON tail is meant to be read, not styled).
fn strip_ansi(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1B}' {
            // CSI sequence: ESC [ ... ; ... <params> <final byte 0x40–0x7E>.
            let terminator = chars
                .by_ref()
                .find(|&c2| ('\u{40}'..='\u{7e}').contains(&c2));
            if terminator.is_none() {
                out.push(c);
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Return the last `n` non-empty lines of the log file at `path`, ANSI-stripped.
fn tail_log(path: &str, n: usize) -> std::io::Result<Vec<String>> {
    let mut f = std::fs::File::open(path)?;
    let mut buf = String::new();
    f.read_to_string(&mut buf)?;
    let mut lines: Vec<String> = buf
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(strip_ansi)
        .collect();
    lines.reverse();
    let tail: Vec<String> = lines.into_iter().take(n).collect();
    Ok(tail.into_iter().rev().collect::<Vec<String>>())
}

async fn health() -> impl IntoResponse {
    (StatusCode::OK, "ok")
}

/// Render an askama template to an HTML response (500 on render failure).
fn render_page<T: Template>(page: T) -> Response {
    match page.render() {
        Ok(body) => Html(body).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Html(format!(
                "<div class=\"alert alert-danger\">render error: {e}</div>\n",
            )),
        )
            .into_response(),
    }
}

// ---------------------------------------------------------------------------
// Pages
// ---------------------------------------------------------------------------

#[derive(askama::Template)]
#[template(path = "dashboard.html")]
struct DashboardPage {
    page: &'static str,
    client_count: usize,
    total_runs: usize,
    succeeded_runs: usize,
    failed_runs: usize,
    recent: Vec<models::RunModel>,
    warning: Option<String>,
}

/// Dashboard: stat cards + a compact table of recent runs.
async fn dashboard(State(state): State<Arc<AppState>>) -> Response {
    let client_count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM clients")
        .fetch_one(&state.pool)
        .await
        .unwrap_or((0,));

    let (total, succeeded): (i64, i64) =
        sqlx::query_as("SELECT COUNT(*), COALESCE(SUM(succeeded), 0) FROM backup_runs")
            .fetch_one(&state.pool)
            .await
            .unwrap_or((0, 0));
    let failed = total.saturating_sub(succeeded);

    let recent = db::load_recent(&state.pool, 10).await.unwrap_or_default();

    // Live reachability check for the warning banner (mirrors the clients page).
    let warning = match api::fetch_clients(&state.server_url, &state.http).await {
        Ok(_) => None,
        Err(e) => Some(format!("server unreachable — showing cached data ({e})")),
    };

    render_page(DashboardPage {
        page: "dashboard",
        client_count: client_count.0 as usize,
        total_runs: total as usize,
        succeeded_runs: succeeded as usize,
        failed_runs: failed as usize,
        recent,
        warning,
    })
}

#[derive(askama::Template)]
#[template(path = "activities.html")]
struct ActivitiesPage {
    page: &'static str,
    runs: Vec<models::RunModel>,
    triggered: Option<String>,
    warning: Option<String>,
}

/// Activities: every recent backup run (newest first), expandable to show the
/// steps each run executed. The "Run now" button triggers all policies; a
/// `?ran=1` reload re-renders with a confirmation banner.
async fn activities(
    State(state): State<Arc<AppState>>,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    let runs = db::load_recent(&state.pool, 100).await.unwrap_or_default();

    // Reachability check drives the warning banner.
    let warning = match api::fetch_clients(&state.server_url, &state.http).await {
        Ok(_) => None,
        Err(e) => Some(format!("server unreachable — showing cached data ({e})")),
    };

    let triggered = if q.get("ran").map_or(false, |v| v == "1") {
        Some("Backup run triggered. New runs appear here within a few seconds.")
    } else {
        None
    };

    render_page(ActivitiesPage {
        page: "activities",
        runs,
        triggered: triggered.map(String::from),
        warning,
    })
}

#[derive(askama::Template)]
#[template(path = "backups.html")]
struct BackupsPage {
    page: &'static str,
    runs: Vec<models::RunModel>,
    warning: Option<String>,
}

/// Backups: browse runs grouped by client, each showing the tool invocations
/// (args + output) that made up the backup. NOTE: the server has no file-tree
/// listing API yet, so this reflects run history, not snapshot contents.
async fn backups(State(state): State<Arc<AppState>>) -> Response {
    let runs = db::load_recent(&state.pool, 100).await.unwrap_or_default();

    let warning = match api::fetch_clients(&state.server_url, &state.http).await {
        Ok(_) => None,
        Err(e) => Some(format!("server unreachable — showing cached data ({e})")),
    };

    render_page(BackupsPage {
        page: "backups",
        runs,
        warning,
    })
}

#[derive(askama::Template)]
#[template(path = "logs.html")]
struct LogsPage {
    page: &'static str,
    lines: Vec<String>,
    log_source: Option<String>,
    error: Option<String>,
}

/// Logs: server-log tail. Server-rendered initial view; the page polls
/// `/api/logs` for live follow. Missing log file -> an explanatory page.
async fn logs(State(state): State<Arc<AppState>>) -> Response {
    let (lines, error) = match &state.log_file {
        None => (
            Vec::new(),
            Some(
                "No server log path configured. Run the stack via `./start-dev.sh`, which sets \
                 PARASANOID_SERVER_LOG to the server's log file."
                    .into(),
            ),
        ),
        Some(path) => match tail_log(path, 500) {
            Ok(lines) => (lines, None),
            Err(e) => (Vec::new(), Some(format!("cannot read {path}: {e}"))),
        },
    };
    render_page(LogsPage {
        page: "logs",
        lines,
        log_source: state.log_file.clone(),
        error,
    })
}

// Kept for continuity: the client detail route.
#[derive(askama::Template)]
#[template(path = "client.html")]
struct ClientPage {
    page: &'static str,
    summary: Option<models::ClientSummary>,
    inventory: Option<models::Inventory>,
    err: Option<String>,
}

async fn client_detail(State(state): State<Arc<AppState>>, Path(id): Path<String>) -> Response {
    let row = sqlx::query_as::<_, (String, String, String, Option<String>, String, Option<String>, Option<String>)>(
        "SELECT id, name, url, up_since, created_at, last_seen, last_inventory_at FROM clients WHERE id=?",
    )
    .bind(&id)
    .fetch_optional(&state.pool)
    .await
    .unwrap_or_default();

    let summary = row.map(
        |(id, name, url, up_since, created_at, last_seen, last_inventory_at)| {
            models::ClientSummary {
                id,
                name,
                url,
                up_since,
                created_at,
                last_seen,
                last_inventory_taken_at: last_inventory_at,
            }
        },
    );

    let inventory: Option<models::Inventory> =
        match sqlx::query_as::<_, (String,)>("SELECT inventory FROM inventory WHERE client_id=?")
            .bind(&id)
            .fetch_optional(&state.pool)
            .await
        {
            Ok(Some((json,))) => match serde_json::from_str::<models::Inventory>(&json) {
                Ok(inv) => Some(inv),
                Err(e) => {
                    warn!(client = %id, error = %e, "inventory decode error");
                    None
                }
            },
            _ => None,
        };

    match summary {
        None => render_page(ClientPage {
            page: "client",
            summary: None,
            inventory: None,
            err: Some("unknown client".into()),
        }),
        Some(summary) => render_page(ClientPage {
            page: "client",
            summary: Some(summary),
            inventory,
            err: None,
        }),
    }
}

// ---------------------------------------------------------------------------
// Control plane + JSON helpers
// ---------------------------------------------------------------------------

/// Trigger all backup policies (`GET /api/backup/run`) and return the queued
/// run ids as JSON.
async fn run_backups(State(state): State<Arc<AppState>>) -> Response {
    match api::run_backups(&state.server_url, &state.http).await {
        Ok(ids) => Json(json!({ "runs": ids })).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("could not trigger backups: {e}") })),
        )
            .into_response(),
    }
}

/// Server-log tail (JSON). Returns the last `lines` (default 500, capped at
/// 5000) non-empty, ANSI-stripped lines from the configured server log. The
/// Logs page polls this for live follow.
async fn api_logs(
    State(state): State<Arc<AppState>>,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    let path = match &state.log_file {
        Some(p) => p.clone(),
        None => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({ "error": "no server log path configured" })),
            )
                .into_response();
        }
    };
    let limit = q
        .get("lines")
        .and_then(|s| s.parse().ok())
        .unwrap_or(500)
        .min(5000)
        .max(1);
    match tail_log(&path, limit) {
        Ok(lines) => {
            Json(json!({ "path": path, "count": lines.len(), "lines": lines })).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": format!("cannot read {path}: {e}") })),
        )
            .into_response(),
    }
}
