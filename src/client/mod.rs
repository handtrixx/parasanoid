//! The client role: a persistent, host-scanning **API bridge**.
//!
//! It investigates the host it runs on (filesystems, external tools such as
//! rsync / sanoid / zfs) and exposes the result over HTTP for the server's
//! discovery loop to pull. It performs no backup work itself — that stays on the
//! server. See `api::not_implemented`.
//!
//! The server reaches this role over the LAN and pulls inventory; the client
//! also dials out to self-register (see `heartbeat`) so no static client list
//! is required.

mod api;
pub mod fs;
pub mod heartbeat;
pub mod host;
pub mod scan;
pub mod tools;

use std::sync::Arc;
use std::time::Duration;

use axum::routing::{get, post};
use axum::Router;
use tokio::sync::RwLock;
use tracing::{info, warn, Level};

use crate::client::heartbeat::Heartbeat;
use crate::inventory::ClientInventory;

/// Shared application state held by the API handlers.
#[derive(Clone)]
pub struct AppState {
    pub snapshot: Arc<RwLock<scan::Snapshot>>,
    pub client_id: String,
    pub version: String,
}

/// Run the client role until the process is terminated.
pub async fn run(version: &str) -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(Level::INFO)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "parasanoid=debug,info".into()),
        )
        .init();

    let client_id = std::env::var("PARASANOID_CLIENT_ID").unwrap_or_else(|_| host::hostname());
    let listen = std::env::var("PARASANOID_LISTEN").unwrap_or_else(|_| ":9100".to_string());

    info!(client_id = %client_id, "parasanoid-worker (client) starting");

    let inventory = scan::collect(&client_id);
    let up_since = chrono::Utc::now();
    let snapshot = Arc::new(RwLock::new(scan::Snapshot {
        inventory,
        up_since,
    }));
    let state = AppState {
        snapshot: snapshot.clone(),
        client_id: client_id.clone(),
        version: version.to_string(),
    };

    // Self-registration: the client learns where the server is and tells the
    // server where it can be reached, so the server can dial it in without a
    // static client list. If no server URL is configured the client still serves
    // its inventory locally (the server falls back to its static list).
    //
    //   * PARASANOID_SERVER_URL        -> where the server is reachable
    //   * PARASANOID_PUBLISHED_URL     -> where the server can reach THIS client
    //                                    (defaults to the local listen address)
    //   * PARASANOID_HEARTBEAT_INTERVAL-> seconds between registration pings
    let server_url = std::env::var("PARASANOID_SERVER_URL").ok();
    let published_url = std::env::var("PARASANOID_PUBLISHED_URL").unwrap_or_else(|_| {
        // `listen` looks like ":9100" or "127.0.0.1:9100"; fall back to the
        // local loopback so the server can reach us when co-located.
        let (host, port) = match listen.split_once(':') {
            Some((h, p)) if !h.is_empty() => (h, p),
            _ => ("127.0.0.1", listen.trim_start_matches(':')),
        };
        format!("http://{host}:{port}")
    });

    if server_url.is_none() {
        warn!("PARASANOID_SERVER_URL not set; self-registration disabled (server must know us via a static list or DNS)");
    }

    let heartbeat = server_url.map(|server_url| Heartbeat {
        server_url,
        published_url,
        client_id: client_id.clone(),
        version: version.to_string(),
        snapshot: snapshot.clone(),
    });

    let heartbeat_interval = std::env::var("PARASANOID_HEARTBEAT_INTERVAL")
        .ok()
        .and_then(|v| v.parse().ok())
        .map(Duration::from_secs)
        .unwrap_or(Duration::from_secs(10));

    // CORS is handy for local `next dev`; drop in prod if the UI is co-located.
    let app = Router::new()
        .route("/api/health", get(api::health))
        .route("/api/inventory", get(api::inventory))
        .route("/api/status", get(api::status))
        .route("/api/scan", post(api::refresh))
        .route("/api/backup/streams", get(api::not_implemented))
        .route("/api/backup/snapshots", post(api::not_implemented))
        .with_state(state);

    // Self-registration runs in the background and never blocks the API. It is
    // a no-op when PARASANOID_SERVER_URL is unset. Spawn it before serve(), which
    // blocks for the life of the process.
    if let Some(hb) = heartbeat {
        tokio::spawn(heartbeat::run(hb, heartbeat_interval));
    }

    info!(listen = %listen, "listening (client bridge)");
    let listener = tokio::net::TcpListener::bind(&listen).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

/// Build the client router (also used by tests).
pub(crate) fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/health", get(api::health))
        .route("/api/inventory", get(api::inventory))
        .route("/api/status", get(api::status))
        .route("/api/scan", post(api::refresh))
        .route("/api/backup/streams", get(api::not_implemented))
        .route("/api/backup/snapshots", post(api::not_implemented))
        .with_state(state)
}

/// Convenience for callers/tests that just want an inventory snapshot now.
pub fn one_shot_inventory(client_id: &str) -> ClientInventory {
    scan::collect(client_id)
}
