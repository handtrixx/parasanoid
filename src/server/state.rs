//! In-memory discovery registry — the server's only source of truth for clients.
//!
//! The discovery loop updates the registry as it polls clients; the API handlers
//! read from this hot path, so there is no separate database in the worker.

use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use tokio::sync::RwLock;

/// A known client as held in the live registry.
#[derive(Clone)]
pub struct ClientRecord {
    pub id: String,
    pub name: String,
    pub url: String,
    pub up_since: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub last_seen: Option<DateTime<Utc>>,
    pub last_inventory: Option<serde_json::Value>,
}

/// Thread-safe registry of discovered clients.
#[derive(Clone)]
pub struct Registry {
    inner: Arc<RwLock<HashMap<String, ClientRecord>>>,
}

impl Registry {
    pub fn new() -> Self {
        Registry {
            inner: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register or overwrite a client.
    pub async fn put(&self, rec: ClientRecord) {
        self.inner.write().await.insert(rec.id.clone(), rec);
    }

    /// Seed a static fallback client (no self-registration) so it appears even
    /// before the first successful poll.
    pub async fn seed_static(&self, url: &str) {
        let id = slug(url);
        self.put(ClientRecord {
            id: id.clone(),
            name: id,
            url: url.to_string(),
            up_since: None,
            created_at: chrono::Utc::now(),
            last_seen: None,
            last_inventory: None,
        })
        .await;
    }

    /// Mark a client reachable at `now`.
    pub async fn touch(&self, id: &str, now: DateTime<Utc>) {
        if let Some(rec) = self.inner.write().await.get_mut(id) {
            rec.last_seen = Some(now);
        }
    }

    /// Attach the latest inventory snapshot for a client.
    pub async fn set_inventory(&self, id: &str, json: serde_json::Value) {
        if let Some(rec) = self.inner.write().await.get_mut(id) {
            rec.last_inventory = Some(json);
        }
    }

    /// All clients sorted by name.
    pub async fn list(&self) -> Vec<ClientRecord> {
        let mut v: Vec<ClientRecord> = self.inner.read().await.values().cloned().collect();
        v.sort_by(|a, b| a.name.cmp(&b.name));
        v
    }

    pub async fn get(&self, id: &str) -> Option<ClientRecord> {
        self.inner.read().await.get(id).cloned()
    }
}

impl Default for Registry {
    fn default() -> Self {
        Self::new()
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
