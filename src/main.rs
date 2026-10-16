//! Paranoid dual-role worker.
//!
//! One binary, two roles, selected by an explicit flag/env:
//!
//!   * `parasanoid-worker server`  — the storage-box brain (JSON API + discovery
//!     loop). Talks only to the UI and dials out to clients on the LAN.
//!   * `parasanoid-worker client`  — a persistent, host-scanning API bridge on
//!     each backup target. Investigates the host and answers the server's poll.
//!
//! Role selection precedence: `server`/`client` subcommand > `PARASANOID_ROLE`
//! env var > default (`server`). Each role sets up its own tracing subscriber
//! and runs the tokio runtime, so this dispatcher only picks a role.

mod client;
mod inventory;
mod server;

use anyhow::Context;
use tracing::info;

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Role {
    Server,
    Client,
}

impl Role {
    fn parse(s: &str) -> Option<Role> {
        match s.trim() {
            "server" => Some(Role::Server),
            "client" => Some(Role::Client),
            _ => None,
        }
    }
}

fn main() -> anyhow::Result<()> {
    let role = resolve_role();
    info!(role = ?role, "starting parasanoid-worker");

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("building tokio runtime")?;

    match role {
        Role::Server => runtime.block_on(server::run()),
        Role::Client => runtime.block_on(client::run(VERSION)),
    }
}

/// Determine the running role from (in priority order): subcommand, `--flag`,
/// then the `PARASANOID_ROLE` environment variable, defaulting to `server`.
fn resolve_role() -> Role {
    let args: Vec<String> = std::env::args().skip(1).collect();

    // `parasanoid-worker client` / `... server`
    if let Some(pos) = args.iter().position(|a| a == "server" || a == "client") {
        if let Some(role) = Role::parse(&args[pos]) {
            return role;
        }
    }

    // `parasanoid-worker --client` / `--server`
    if args.iter().any(|a| a == "--client") {
        return Role::Client;
    }
    if args.iter().any(|a| a == "--server") {
        return Role::Server;
    }

    // PARASANOID_ROLE=client|server
    if let Ok(v) = std::env::var("PARASANOID_ROLE") {
        if let Some(role) = Role::parse(&v) {
            return role;
        }
    }

    Role::Server
}
