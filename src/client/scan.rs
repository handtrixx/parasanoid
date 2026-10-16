//! Host scanning: assemble a full [`ClientInventory`] snapshot and a
//! self-description [`WorkerStatus`].

use crate::inventory::{ClientInventory, ClientStatus, WorkerStatus};

/// Build a fresh inventory for `client_id`.
pub fn collect(client_id: &str) -> ClientInventory {
    let (host, filesystems, tools) = (
        super::host::collect(),
        super::fs::collect(),
        super::tools::collect(),
    );

    ClientInventory {
        client_id: client_id.to_string(),
        taken_at: chrono::Utc::now(),
        host: host.unwrap_or_else(|e| {
            tracing::warn!(error = %e, "falling back to minimal host info");
            crate::inventory::HostInfo {
                hostname: super::host::hostname(),
                os: "unknown".into(),
                release: "unknown".into(),
                architecture: "unknown".into(),
                uptime_seconds: 0,
                ram_bytes: 0,
            }
        }),
        filesystems: filesystems.unwrap_or_default(),
        tools: tools.unwrap_or_default(),
    }
}

/// Whether the host backup tooling is usable (rsync required; zfs/sanoid optional).
pub fn host_tools_ready(tools: &[crate::inventory::DetectedTool]) -> bool {
    tools
        .iter()
        .find(|t| t.name == "rsync")
        .map(|t| t.available)
        .unwrap_or(false)
}

/// A single inventory snapshot plus the health view derived from it.
pub struct Snapshot {
    pub inventory: ClientInventory,
    pub up_since: chrono::DateTime<chrono::Utc>,
}

impl Snapshot {
    pub fn status(&self) -> ClientStatus {
        ClientStatus {
            client_id: self.inventory.client_id.clone(),
            alive: true,
            last_inventory_at: Some(self.inventory.taken_at),
            up_since: Some(self.up_since),
        }
    }

    pub fn worker_status(&self, version: &str) -> WorkerStatus {
        WorkerStatus {
            client_id: self.inventory.client_id.clone(),
            mode: "client",
            version: version.to_string(),
            up_since: self.up_since,
            host_tools_ready: host_tools_ready(&self.inventory.tools),
        }
    }
}
