//! The discovery loop.
//!
//! Every `interval` it dials out to every known client (the static list the
//! operator provides), fetches `/api/health` and `/api/inventory`, and updates
//! the in-memory registry. This is the "server reaches out to client" path.

use std::time::Duration;

use chrono::Utc;
use tracing::{info, warn};

use crate::server::state::Registry;

/// A client to probe during one poll cycle.
struct ProbeTarget {
    id: String,
    name: String,
    url: String,
}

/// Run the discovery loop until the process is terminated.
///
/// `stale_after` bounds how long a client stays in the registry after its
/// presence goes quiet. Registered clients refresh `last_seen` on each poll
/// (and on each heartbeat), so a client that has gone away is pruned once it
/// stops responding for longer than `stale_after`.
pub async fn run(reg: Registry, interval: Duration, clients: Vec<String>, stale_after: Duration) {
    info!(
        interval_secs = interval.as_secs(),
        stale_after_secs = stale_after.as_secs(),
        "discovery loop started"
    );
    loop {
        prune_stale(&reg, stale_after).await;
        poll_once(&reg, &clients).await;
        tokio::time::sleep(interval).await;
    }
}

/// Drop clients whose presence hasn't been refreshed within `stale_after`.
async fn prune_stale(reg: &Registry, stale_after: Duration) {
    let cutoff = Utc::now() - chrono::TimeDelta::seconds(stale_after.as_secs() as i64);
    reg.prune_stale(cutoff).await;
}

async fn poll_once(reg: &Registry, clients: &[String]) {
    let targets = build_targets(reg, clients).await;
    info!(count = targets.len(), "polling clients");

    for target in targets {
        probe_target(reg, target).await;
    }
}

/// Assemble the deduplicated set of clients to probe this cycle.
async fn build_targets(reg: &Registry, clients: &[String]) -> Vec<ProbeTarget> {
    let mut seen = std::collections::BTreeSet::new();
    let mut out: Vec<ProbeTarget> = Vec::new();

    for rec in reg.list().await {
        if seen.insert(rec.url.clone()) {
            out.push(ProbeTarget {
                id: rec.id,
                name: rec.name,
                url: rec.url,
            });
        }
    }

    for url in clients {
        if seen.insert(url.clone()) {
            out.push(ProbeTarget {
                id: slug(url),
                name: slug(url),
                url: url.clone(),
            });
        }
    }

    out
}

async fn probe_target(reg: &Registry, target: ProbeTarget) {
    let base = target.url.trim_end_matches('/');
    let health_url = format!("{base}/api/health");
    let inv_url = format!("{base}/api/inventory");

    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            warn!(url = %target.url, error = %e, "failed to build http client");
            return;
        }
    };

    let alive = match client.get(&health_url).send().await {
        Ok(resp) => resp.status().is_success(),
        Err(e) => {
            warn!(url = %target.url, error = %e, "client unreachable");
            false
        }
    };

    if !alive {
        return;
    }

    // Mark the client reachable so the UI shows a fresh `last_seen`.
    reg.touch(&target.id, Utc::now()).await;

    // Fetch inventory (best effort — a transient failure shouldn't drop the client).
    match client.get(&inv_url).send().await {
        Ok(resp) if resp.status().is_success() => match resp.json::<serde_json::Value>().await {
            Ok(value) => {
                reg.set_inventory(&target.id, value).await;
                info!(id = %target.id, "client polled");
            }
            Err(e) => warn!(url = %target.url, error = %e, "invalid inventory json"),
        },
        Err(e) => warn!(url = %target.url, error = %e, "inventory request failed"),
        Ok(resp) => warn!(url = %target.url, status = %resp.status(), "inventory request failed"),
    }
}

/// Derive a stable, URL-safe id from a client URL's host component.
fn slug(url: &str) -> String {
    let host = url
        .strip_prefix("http://")
        .or_else(|| url.strip_prefix("https://"))
        .unwrap_or(url);
    let host = host.split('/').next().unwrap_or(host);
    host.split(':').next().unwrap_or(host).to_string()
}
