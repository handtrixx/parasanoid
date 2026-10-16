//! Runtime configuration, read from the environment.

/// All configuration the server needs at startup.
#[derive(Debug, Clone)]
pub struct Config {
    /// Address the server listens on (`:8080` by default).
    pub listen: String,
    /// How often the discovery loop polls clients.
    pub poll_interval_secs: u64,
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

        Ok(Config {
            listen: std::env::var("PARASANOID_LISTEN").unwrap_or_else(|_| ":8080".to_string()),
            poll_interval_secs: poll,
            clients: std::env::var("PARASANOID_CLIENTS")
                .unwrap_or_default()
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect(),
        })
    }
}
