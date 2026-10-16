//! Server-side fetch of the paranoid *server* JSON API.
//!
//! The Rust UI owns the database and renders HTML server-side, but the client
//! discovery data still lives in the server's in-memory registry. This module
//! dials out to it (same-LAN HTTP) and caches the results in SQLite.

use anyhow::{anyhow, Context, Result};

use crate::models::{ClientSummary, Inventory};

/// Poll `GET /api/discovery/clients` from the paranoid server.
pub async fn fetch_clients(server_url: &str, http: &reqwest::Client) -> Result<Vec<ClientSummary>> {
    let url = format!("{}/api/discovery/clients", server_url.trim_end_matches('/'));
    let resp = http
        .get(&url)
        .send()
        .await
        .with_context(|| format!("GET {url}"))?;
    if !resp.status().is_success() {
        return Err(anyhow!("server returned {} for {}", resp.status(), url));
    }
    let body: serde_json::Value = resp
        .json()
        .await
        .context("decoding {\"clients\": [...]} response")?;
    let list: Vec<ClientSummary> = body
        .get("clients")
        .and_then(|c| c.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| serde_json::from_value::<ClientSummary>(v.clone()).ok())
                .collect()
        })
        .unwrap_or_default();
    Ok(list)
}

/// Fetch the latest inventory snapshot for a single client.
pub async fn fetch_inventory(
    server_url: &str,
    id: &str,
    http: &reqwest::Client,
) -> Result<Inventory> {
    let url = format!(
        "{}/api/discovery/clients/{}/inventory",
        server_url.trim_end_matches('/'),
        url_encode(id)
    );
    let resp = http
        .get(&url)
        .send()
        .await
        .with_context(|| format!("GET {url}"))?;
    if resp.status() == reqwest::StatusCode::NO_CONTENT {
        return Err(anyhow!("no inventory yet for {id}"));
    }
    resp.status()
        .is_success()
        .then(|| ())
        .ok_or_else(|| anyhow!("server returned {} for {}", resp.status(), url))?;
    let inv: Inventory = resp
        .json()
        .await
        .with_context(|| format!("decoding inventory for {id}"))?;
    Ok(inv)
}

/// Minimal percent-encoding for client ids (they are host slugs, but encode
/// anyway so arbitrary ids survive the path).
fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}
