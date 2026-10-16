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

use std::sync::Arc;

use askama::Template;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{Html, IntoResponse, Response},
    routing::get,
    Router,
};
use sqlx::SqlitePool;
use tokio::time::{interval, Duration};
use tracing::{info, warn};

#[derive(Clone)]
struct AppState {
    pool: SqlitePool,
    server_url: String,
    http: reqwest::Client,
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

    let pool = db::open(&db_path).await?;
    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()?;
    let state = Arc::new(AppState {
        pool: pool.clone(),
        server_url: server_url.clone(),
        http: http.clone(),
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
        .route("/", get(clients_list))
        .route("/clients/:id", get(client_detail))
        .route("/health", get(health))
        .nest_service(
            "/assets",
            tower_http::services::fs::ServeDir::new(assets_dir),
        )
        .with_state(state)
        .layer(tower_http::trace::TraceLayer::new_for_http());

    info!(listen = %listen, server = %server_url, "parasanoid-ui-rust starting");
    let listener = tokio::net::TcpListener::bind(&listen).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

/// One discovery cycle: fetch clients from the server, mirror into SQLite.
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

/// Relative label for an RFC3339 timestamp (best-effort; returns raw value if unparsable).
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

#[derive(askama::Template)]
#[template(path = "clients.html")]
struct ClientsPage {
    clients: Vec<models::ClientSummary>,
    warning: Option<String>,
}

async fn clients_list(State(state): State<Arc<AppState>>) -> Response {
    let clients: Vec<models::ClientSummary> = sqlx::query_as(
        "SELECT id, name, url, up_since, created_at, last_seen, last_inventory_at FROM clients
         ORDER BY name",
    )
    .fetch_all(&state.pool)
    .await
    .unwrap_or_default()
    .into_iter()
    .map(
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
    )
    .collect();

    // On server failure, fall back to cached clients and surface a warning
    // banner. Previously this returned a bare 129-byte <div> (HTTP 503), which
    // the browser rendered as a blank page.
    let warning = match api::fetch_clients(&state.server_url, &state.http).await {
        Ok(_) => None,
        Err(e) => Some(format!("server unreachable — showing cached data ({e})")),
    };
    render_page(ClientsPage { clients, warning })
}

#[derive(askama::Template)]
#[template(path = "client.html")]
struct ClientPage {
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
            summary: None,
            inventory: None,
            err: Some("unknown client".into()),
        }),
        Some(summary) => render_page(ClientPage {
            summary: Some(summary),
            inventory,
            err: None,
        }),
    }
}
