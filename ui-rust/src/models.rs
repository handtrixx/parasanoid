//! Types mirroring the paranoid server API and the client inventory payload.
//!
//! These are intentionally a faithful mirror of `parasanoid/src/inventory.rs`
//! (the inventory) and `parasanoid/src/server/api.rs` (the discovery summaries),
//! so the prototype renders real server data without a shared-crate coupling.

use serde::{Deserialize, Serialize};

/// One row of `GET /api/discovery/clients` — the client list the UI shows.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientSummary {
    pub id: String,
    pub name: String,
    pub url: String,
    pub up_since: Option<String>,
    pub created_at: String,
    pub last_seen: Option<String>,
    pub last_inventory_taken_at: Option<String>,
}

/// Host-level block of an inventory snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostInfo {
    pub hostname: String,
    pub os: String,
    pub release: String,
    pub architecture: String,
    pub uptime_seconds: u64,
    pub ram_bytes: u64,
}

/// A filesystem type detected on the host.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectedFs {
    pub fs_type: String,
    pub count: u32,
    pub strategy: String,
}

/// An external tool found on the host (`rsync`, `sanoid`, ...).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectedTool {
    pub name: String,
    pub available: bool,
    pub version: Option<String>,
}

/// Full inventory snapshot reported by a client worker (`GET /inventory`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Inventory {
    pub client_id: String,
    pub taken_at: String,
    pub host: HostInfo,
    pub filesystems: Vec<DetectedFs>,
    pub tools: Vec<DetectedTool>,
}

/// One step of a backup run (`GET /api/backup/runs` and `/run/:id`).
///
/// A "step" is a single external tool invocation (an `rsync`, a `syncoid`, a
/// `zstd` compress, ...). The strings carry the tool's raw stdout/stderr so the
/// UI can show what the backup actually did.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepModel {
    pub tool: String,
    pub args: Vec<String>,
    pub success: bool,
    pub exit_code: Option<i32>,
    pub elapsed_ms: u64,
    pub stdout: String,
    pub stderr: String,
}

/// A completed backup run as returned by `GET /api/backup/runs` and rendered
/// inline (with steps) from the UI's cache. Dates are RFC3339 strings (chrono
/// serializes `DateTime<Utc>` that way), matching what the server emits.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunModel {
    pub run_id: String,
    pub client_id: String,
    pub started_at: Option<String>,
    pub ended_at: Option<String>,
    pub steps: Vec<StepModel>,
    pub succeeded: bool,
}
