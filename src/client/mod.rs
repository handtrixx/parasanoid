//! The client role: a persistent, host-scanning **API bridge**.
//!
//! It investigates the host it runs on (filesystems, external tools such as
//! rsync / sanoid / zfs) and exposes the result over HTTP for the server's
//! discovery loop to pull. It performs no backup work itself — that stays on the
//! server. See `api::not_implemented`.
//!
//! The server reaches this role over the LAN; nothing dials out.

mod api;
pub mod fs;
pub mod host;
pub mod scan;
pub mod tools;

use std::sync::Arc;

use axum::routing::{get, post};
use axum::Router;
use tokio::sync::RwLock;
use tracing::{info, Level};

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

    // CORS is handy for local `next dev`; drop in prod if the UI is co-located.
    let app = Router::new()
        .route("/api/health", get(api::health))
        .route("/api/inventory", get(api::inventory))
        .route("/api/status", get(api::status))
        .route("/api/scan", post(api::refresh))
        .route("/api/backup/streams", get(api::not_implemented))
        .route("/api/backup/snapshots", post(api::not_implemented))
        .with_state(state);

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
