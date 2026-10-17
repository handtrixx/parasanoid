//! The server role: a JSON API for the Next.js UI plus a discovery loop that
//! dials out to clients on the LAN to pull their host inventory.
//!
//! This is the "brain". All backup work stays on the client side; the server
//! only collects inventory snapshots and surfaces them. The client exposes
//! `/api/*` endpoints (see `crate::client`); this module talks to them.

mod api;
pub mod backup;
pub mod config;
pub mod discovery;
pub mod state;

use std::time::Duration;

use axum::routing::{get, post};
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
    // A single shared HTTP client for both the data plane (server -> client)
    // and the discovery loop (client inventory polls). reqwest::Client is a
    // cheap Arc clone to hand out.
    let http = reqwest::Client::new();

    // Seed the static fallback list so air-gapped clients still show up in the
    // UI even when they cannot reach the server to self-register.
    for url in &cfg.clients {
        reg.seed_static(url).await;
        info!(%url, "seeded static fallback client");
    }

    let app = Router::new()
        .route("/api/health", get(api::health))
        // Clients POST here on startup and periodically (heartbeat) so the
        // server can reach them without a preconfigured static client list.
        .route("/api/discovery/register", post(api::register))
        .route("/api/discovery/clients", get(api::clients))
        .route("/api/discovery/clients/:id", get(api::client_detail))
        .route("/api/discovery/clients/:id/inventory", get(api::inventory))
        // Data plane: the server forwards a backup tool call to this client,
        // which then runs the tool on its own host, e.g.
        // `POST /api/discovery/clients/:id/tools/:tool`.
        .route(
            "/api/discovery/clients/:id/tools/:tool",
            post(backup::forward_tool_call),
        )
        .with_state(api::AppState {
            reg: reg.clone(),
            http: http.clone(),
        });

    // Discovery loop runs in the background and self-limits to the clients it
    // actually knows about, so it is safe to start unconditionally.
    let interval = Duration::from_secs(cfg.poll_interval_secs);
    let stale_after = Duration::from_secs(cfg.stale_after_secs);
    let clients = cfg.clients.clone();
    tokio::spawn(discovery::run(
        reg,
        http.clone(),
        interval,
        clients,
        stale_after,
    ));

    let listener = tokio::net::TcpListener::bind(&cfg.listen).await?;
    info!(listen = %cfg.listen, "listening (server API)");
    axum::serve(listener, app).await?;
    Ok(())
}
