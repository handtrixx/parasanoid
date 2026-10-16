//! The server role: a JSON API for the Next.js UI plus a discovery loop that
//! dials out to clients on the LAN to pull their host inventory.
//!
//! This is the "brain". All backup work stays on the client side; the server
//! only collects inventory snapshots and surfaces them. The client exposes
//! `/api/*` endpoints (see `crate::client`); this module talks to them.

mod api;
pub mod config;
pub mod discovery;
pub mod state;

use std::time::Duration;

use axum::routing::get;
use axum::Router;
use tracing::{info, Level};

use crate::server::state::Registry;

/// Run the server role until the process is terminated.
pub async fn run() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(Level::INFO)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "parasanoid=debug,info".into()),
        )
        .init();

    let cfg = config::Config::from_env()?;
    info!(listen = %cfg.listen, "parasanoid-worker (server) starting");

    let reg = Registry::new();

    // Seed the static fallback list so air-gapped clients still show up in the
    // UI before the first successful poll. Self-registration is a future hardening.
    for url in &cfg.clients {
        reg.seed_static(url).await;
        info!(%url, "seeded static fallback client");
    }

    let app = Router::new()
        .route("/api/health", get(api::health))
        .route("/api/discovery/clients", get(api::clients))
        .route("/api/discovery/clients/:id", get(api::client_detail))
        .route("/api/discovery/clients/:id/inventory", get(api::inventory))
        .with_state(api::AppState { reg: reg.clone() });

    // Discovery loop runs in the background and self-limits to the clients it
    // actually knows about, so it is safe to start unconditionally.
    let interval = Duration::from_secs(cfg.poll_interval_secs);
    let clients = cfg.clients.clone();
    tokio::spawn(discovery::run(reg, interval, clients));

    let listener = tokio::net::TcpListener::bind(&cfg.listen).await?;
    info!(listen = %cfg.listen, "listening (server API)");
    axum::serve(listener, app).await?;
    Ok(())
}
