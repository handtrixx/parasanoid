//! Host-level information gathering. Runs on linux.

use anyhow::{bail, Context, Result};
use std::process::Command;

/// Best-effort hostname resolution.
pub fn hostname() -> String {
    Command::new("hostname")
        .output()
        .ok()
        .and_then(|o| {
            let h = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if h.is_empty() {
                None
            } else {
                Some(h)
            }
        })
        .unwrap_or_else(|| "unknown".to_string())
}

fn read_proc_file(path: &str, key: &str) -> Option<u64> {
    let content = std::fs::read_to_string(path).ok()?;
    for line in content.lines() {
        let rest = line.strip_prefix(key)?;
        let rest = rest.strip_prefix(':').unwrap_or(rest);
        let tok = rest.split_whitespace().next()?;
        // /proc/uptime reports fractional seconds; meminfo reports kB.
        if let Ok(n) = tok.parse::<u64>() {
            return Some(n);
        }
        if let Ok(f) = tok.parse::<f64>() {
            return Some(f as u64);
        }
    }
    None
}

/// Collect basic host information.
pub fn collect() -> Result<crate::inventory::HostInfo> {
    let hostname = hostname();

    let os = std::fs::read_to_string("/etc/os-release")
        .ok()
        .and_then(|c| {
            c.lines().find_map(|l| {
                l.strip_prefix("PRETTY_NAME=")
                    .map(|v| v.trim_matches('"').to_string())
            })
        })
        .unwrap_or_else(|| "unknown".to_string());

    let release = uname("-r")?;
    let architecture = uname("-m")?;

    let uptime_seconds = read_proc_file("/proc/uptime", "uptime").unwrap_or(0);
    // meminfo values are in kB; convert to bytes.
    let ram_bytes = read_proc_file("/proc/meminfo", "MemTotal")
        .map(|b| b.saturating_mul(1024))
        .unwrap_or(0);

    Ok(crate::inventory::HostInfo {
        hostname,
        os,
        release,
        architecture,
        uptime_seconds,
        ram_bytes,
    })
}

fn uname(flag: &str) -> Result<String> {
    let out = Command::new("uname")
        .arg(flag)
        .output()
        .with_context(|| format!("running `uname {flag}`"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        bail!("`uname {flag}` failed")
    }
}
