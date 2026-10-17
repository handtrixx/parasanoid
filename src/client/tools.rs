//! External tool detection.
//!
//! For each known tool we try `tool --version` (falling back to `-V`). A non-zero
//! exit means the tool is unavailable. We keep the first line of output as a
//! rough version string.

use anyhow::Result;
use std::collections::BTreeSet;
use std::process::Command;

use crate::inventory::DetectedTool;

/// The tools paranoid is interested in. Public so the tool-invocation API can
/// validate tool names before spawning anything.
pub const KNOWN_TOOLS: &[&str] = &[
    "rsync", "sanoid", "syncoid", "zfs", "zpool", "btrfs", "restic",
];

fn first_line(out: &std::process::Output) -> Option<String> {
    let text = String::from_utf8_lossy(&out.stdout);
    text.lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .map(str::to_string)
        .or_else(|| {
            let text = String::from_utf8_lossy(&out.stderr);
            text.lines()
                .map(str::trim)
                .find(|l| !l.is_empty())
                .map(str::to_string)
        })
}

fn tool_present(name: &str) -> bool {
    Command::new("sh")
        .arg("-c")
        .arg(format!("command -v {name}"))
        .output()
        .ok()
        .is_some_and(|o| o.status.success())
}

/// Detect known external tools on the host.
pub fn collect() -> Result<Vec<DetectedTool>> {
    let mut tools = BTreeSet::new();

    for name in KNOWN_TOOLS {
        if !tool_present(name) {
            tools.insert(DetectedTool {
                name: (*name).to_string(),
                available: false,
                version: None,
            });
            continue;
        }

        let ok = run_version(name)
            .map(|version| DetectedTool {
                name: name.to_string(),
                available: true,
                version: Some(version),
            })
            .unwrap_or_else(|| DetectedTool {
                name: name.to_string(),
                available: true,
                version: None,
            });
        tools.insert(ok);
    }

    let mut out: Vec<DetectedTool> = tools.into_iter().collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

fn run_version(name: &str) -> Option<String> {
    for flag in ["--version", "-V"] {
        let out = Command::new(name).arg(flag).output().ok()?;
        if out.status.success() {
            return first_line(&out);
        }
    }
    None
}
