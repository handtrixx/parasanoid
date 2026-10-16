//! Data model a client reports about the host it runs on.
//!
//! This module is the single source of truth for the inventory payload the
//! worker exposes on the client side and the server caches in its registry.
//! Keeping it in one place keeps the two roles of the same binary in lock-step.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Full inventory snapshot reported by a single worker (client role).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientInventory {
    /// Identity of the worker that produced this snapshot.
    pub client_id: String,
    /// When the snapshot was taken (client-local time, UTC).
    pub taken_at: DateTime<Utc>,
    /// Host-level information (hostname, os, kernel, etc.).
    pub host: HostInfo,
    /// Filesystem types detected on the host.
    pub filesystems: Vec<DetectedFs>,
    /// External tools that were found on `$PATH` / usable.
    pub tools: Vec<DetectedTool>,
}

/// Host-level information reported by the client.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostInfo {
    pub hostname: String,
    pub os: String,
    pub release: String,
    pub architecture: String,
    /// Uptime in seconds (from `/proc/uptime` on linux).
    pub uptime_seconds: u64,
    /// Bytes of installed RAM (from `/proc/meminfo` on linux).
    pub ram_bytes: u64,
}

/// A filesystem type detected on the host, plus how paranoid treats it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectedFs {
    /// e.g. `zfs`, `btrfs`, `ext4`, `xfs`.
    pub fs_type: String,
    /// Number of filesystems of this type seen on the host.
    pub count: u32,
    /// The backup strategy paranoid would use for this fs type.
    pub strategy: BackupStrategy,
}

/// How paranoid backs up a given filesystem type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackupStrategy {
    /// Native snapshot + replication (ZFS via sanoid/syncoid, btrfs via send/receive).
    Snapshot,
    /// Directory-level copy (rsync / restic).
    Copy,
    /// Not yet supported.
    Unsupported,
}

/// An external tool found on the host.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct DetectedTool {
    /// Tool name, e.g. `rsync`, `sanoid`, `syncoid`, `zfs`, `zpool`.
    pub name: String,
    /// Whether the tool is usable.
    pub available: bool,
    /// `tool --version` output, truncated, when available.
    pub version: Option<String>,
}

/// Lightweight liveness/health a worker reports.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientStatus {
    pub client_id: String,
    pub alive: bool,
    pub last_inventory_at: Option<DateTime<Utc>>,
    pub up_since: Option<DateTime<Utc>>,
}

/// Worker self-description, exposed by the client role for the server's bridge.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerStatus {
    pub client_id: String,
    pub mode: &'static str,
    pub version: String,
    pub up_since: DateTime<Utc>,
    /// Whether the host backup tooling (rsync / sanoid / zfs) is usable.
    pub host_tools_ready: bool,
}
