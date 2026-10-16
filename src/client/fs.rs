//! Filesystem detection.
//!
//! Two signals are combined:
//!   1. `/proc/mounts` gives every mounted filesystem and its type, grouped by
//!      fstype with a count (covers ext4/xfs/btrfs/vfat/... without tools).
//!   2. If the ZFS userland is present (`zpool`/`zfs`), `zfs list` yields the
//!      authoritative dataset/pool count so ZFS is reported precisely.

use anyhow::Result;
use std::collections::BTreeMap;
use std::process::Command;

use crate::inventory::{BackupStrategy, DetectedFs};

/// Map a kernel fstype to paranoid's backup strategy.
fn strategy_for(fs_type: &str) -> BackupStrategy {
    match fs_type {
        "zfs" | "btrfs" => BackupStrategy::Snapshot,
        "ext4" | "xfs" | "ntfs" | "vfat" | "exfat" | "f2fs" => BackupStrategy::Copy,
        _ => BackupStrategy::Unsupported,
    }
}

/// Mount-based counts of fstypes.
fn from_mounts() -> Result<BTreeMap<String, u32>> {
    let content = std::fs::read_to_string("/proc/mounts")?;
    let mut counts: BTreeMap<String, u32> = BTreeMap::new();
    for line in content.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        // root_dev  mount  fstype  options ...
        if let Some(fs_type) = parts.get(2) {
            *counts.entry(fs_type.to_string()).or_insert(0) += 1;
        }
    }
    Ok(counts)
}

/// ZFS dataset count, if ZFS userland is installed.
fn zfs_dataset_count() -> Option<u32> {
    let out = Command::new("zfs")
        .arg("list")
        .arg("-H")
        .arg("-o")
        .arg("name")
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout);
    let count = s.lines().filter(|l| !l.trim().is_empty()).count();
    if count == 0 {
        return None;
    }
    Some(count as u32)
}

/// Gather detected filesystems on the host.
pub fn collect() -> Result<Vec<DetectedFs>> {
    let counts = from_mounts()?;

    let mut out: Vec<DetectedFs> = counts
        .into_iter()
        .map(|(fs_type, count)| {
            let strategy = strategy_for(&fs_type);
            DetectedFs {
                fs_type,
                count,
                strategy,
            }
        })
        .collect();

    // Augment / override ZFS with the authoritative dataset count.
    if let Some(count) = zfs_dataset_count() {
        match out.iter_mut().find(|f| f.fs_type == "zfs") {
            Some(f) => f.count = count,
            None => out.push(DetectedFs {
                fs_type: "zfs".to_string(),
                count,
                strategy: BackupStrategy::Snapshot,
            }),
        }
    }

    Ok(out)
}
