//! Client self-registration heartbeat.
//!
//! A client learns where the server is via `PARASANOID_SERVER_URL` and tells the
//! server where it can be reached via `PARASANOID_PUBLISHED_URL`, then calls the
//! server's registration endpoint on startup and periodically thereafter. This
//! lets the server reach clients on the LAN without a preconfigured static
//! client list.
//!
//! Registration failures are logged, never fatal — the client keeps serving its
//! inventory locally regardless of whether the server can currently be reached.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::RwLock;
use tracing::{info, warn};

use super::scan::Snapshot;
use crate::inventory::WorkerStatus;

/// A configured heartbeat: it knows where the server is and where it can be
/// reached itself.
pub struct Heartbeat {
    /// Where the server is reachable (from `PARASANOID_SERVER_URL`).
    pub server_url: String,
    /// Where the server can reach THIS client (from `PARASANOID_PUBLISHED_URL`).
    pub published_url: String,
    /// The client's stable id.
    pub client_id: String,
    /// Version reported in the registration.
    pub version: String,
    /// Shared snapshot so the heartbeat can report current status.
    pub snapshot: Arc<RwLock<Snapshot>>,
}

/// The body a client POSTs to the server's registration endpoint.
#[derive(serde::Serialize)]
struct RegistrationPayload {
    client_id: String,
    published_url: String,
    status: Option<WorkerStatus>,
}

/// Register once, then on every `interval` until the process is terminated.
pub async fn run(hb: Heartbeat, interval: Duration) {
    info!(server = %hb.server_url, "client self-registration started");
    loop {
        register_once(&hb).await;
        tokio::time::sleep(interval).await;
    }
}

async fn register_once(hb: &Heartbeat) {
    let status = { hb.snapshot.read().await.worker_status(&hb.version) };

    let payload = RegistrationPayload {
        client_id: hb.client_id.clone(),
        published_url: hb.published_url.clone(),
        status: Some(status),
    };

    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            warn!(error = %e, "failed to build http client");
            return;
        }
    };

    let url = format!(
        "{}/api/discovery/register",
        hb.server_url.trim_end_matches('/')
    );

    match client.post(&url).json(&payload).send().await {
        Ok(resp) if resp.status().is_success() => {
            info!(status = %resp.status(), "self-registered with server")
        }
        Ok(resp) => warn!(status = %resp.status(), "registration rejected by server"),
        Err(e) => warn!(error = %e, "registration failed (client keeps running)"),
    }
}
