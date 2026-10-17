//! Runtime configuration, read from the environment.

/// All configuration the server needs at startup.
#[derive(Debug, Clone)]
pub struct Config {
    /// Address the server listens on (`:8080` by default).
    pub listen: String,
    /// How often the discovery loop polls clients.
    pub poll_interval_secs: u64,
    /// How long (seconds) a client stays in the registry after its presence
    /// goes quiet. Registered clients refresh on each poll; the static fallback
    /// list (which never self-registers) is never pruned.
    pub stale_after_secs: u64,
    /// Static list of client base URLs (comma-separated). The server dials each
    /// of these on an interval; no client self-registration is required.
    pub clients: Vec<String>,
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        let poll = std::env::var("PARASANOID_POLL_INTERVAL")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(10);

        let stale_after = std::env::var("PARASANOID_STALE_AFTER")
            .ok()
            .and_then(|v| v.parse().ok())
            // Prune after ~3 missed polls, but keep a sane floor so a slow LAN
            // doesn't drop a healthy client.
            .map(|s: u64| s.max(poll * 3))
            .unwrap_or(30);

        Ok(Config {
            listen: std::env::var("PARASANOID_LISTEN").unwrap_or_else(|_| ":8080".to_string()),
            poll_interval_secs: poll,
            stale_after_secs: stale_after,
            clients: std::env::var("PARASANOID_CLIENTS")
                .unwrap_or_default()
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect(),
        })
    }
}
