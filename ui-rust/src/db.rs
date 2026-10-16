//! SQLite layer — the one thing that *must* live in the UI process.
//!
//! `rusqlite::Connection` is `!Send`, which is why the worker can't hold a DB.
//! `sqlx`'s `SqlitePool` is `Send` + async, so it fits the axum handler model.
//! This holds the discovered-client list and cached inventory snapshots so the
//! page renders quickly and survives a server blip.

use anyhow::{Context, Result};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePool, SqlitePoolOptions};

use crate::models::{ClientSummary, Inventory};

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS clients (
    id                   TEXT PRIMARY KEY,
    name                 TEXT NOT NULL,
    url                  TEXT NOT NULL,
    up_since             TEXT,
    created_at           TEXT NOT NULL,
    last_seen            TEXT,
    last_inventory_at    TEXT,
    refreshed_at         TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS inventory (
    client_id    TEXT PRIMARY KEY,
    inventory    JSON NOT NULL,
    fetched_at   TEXT NOT NULL
);
"#;

/// Current UTC time as an RFC3339 string (shared by the callers in this crate).
fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339()
}

/// Open (creating if needed) the SQLite pool at `path`.
pub async fn open(path: &str) -> Result<SqlitePool> {
    let opts = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(4)
        .connect_with(opts)
        .await
        .with_context(|| format!("opening sqlite {path}"))?;
    sqlx::query(SCHEMA)
        .execute(&pool)
        .await
        .context("applying schema")?;
    Ok(pool)
}

/// Insert or update a discovered client row.
pub async fn upsert_client(pool: &SqlitePool, c: &ClientSummary) -> Result<()> {
    sqlx::query(
        "INSERT INTO clients (id, name, url, up_since, created_at, last_seen, last_inventory_at, refreshed_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT(id) DO UPDATE SET
           name=excluded.name, url=excluded.url, up_since=excluded.up_since,
           last_seen=excluded.last_seen, last_inventory_at=excluded.last_inventory_at,
           refreshed_at=excluded.refreshed_at",
    )
    .bind(&c.id)
    .bind(&c.name)
    .bind(&c.url)
    .bind(c.up_since.as_deref())
    .bind(&c.created_at)
    .bind(c.last_seen.as_deref())
    .bind(c.last_inventory_taken_at.as_deref())
    .bind(now_rfc3339())
    .execute(pool)
    .await
    .context("upserting client")?;
    Ok(())
}

/// Refresh the `refreshed_at` stamp for clients that are still known but not
/// re-reported this cycle (keeps "last seen" meaningful across polls).
pub async fn touch_stale(pool: &SqlitePool, ids: &[String]) -> Result<()> {
    let now = now_rfc3339();
    let sql = format!(
        "UPDATE clients SET refreshed_at=? WHERE id IN ({placeholders})",
        placeholders = ids.iter().map(|_| "?").collect::<Vec<_>>().join(",")
    );
    let mut q = sqlx::query(&sql);
    q = q.bind(&now);
    for id in ids {
        q = q.bind(id.as_str());
    }
    q.execute(pool).await.context("touching stale clients")?;
    Ok(())
}

/// Cache a client's inventory snapshot (or delete it if gone).
pub async fn set_inventory(pool: &SqlitePool, id: &str, inv: &Inventory) -> Result<()> {
    let json = serde_json::to_string(inv).context("serializing inventory")?;
    sqlx::query(
        "INSERT INTO inventory (client_id, inventory, fetched_at) VALUES (?, ?, ?)
         ON CONFLICT(id) DO UPDATE SET inventory=excluded.inventory, fetched_at=excluded.fetched_at",
    )
    .bind(id)
    .bind(json)
    .bind(now_rfc3339())
    .execute(pool)
    .await
    .context("upserting inventory")?;
    Ok(())
}

/// Drop cached inventory for ids not present in `keep`.
pub async fn prune_inventory(pool: &SqlitePool, keep: &[String]) -> Result<()> {
    if keep.is_empty() {
        // keep nothing cached is wrong; fall back to keeping all
        return Ok(());
    }
    let sql = format!(
        "DELETE FROM inventory WHERE client_id NOT IN ({placeholders})",
        placeholders = keep.iter().map(|_| "?").collect::<Vec<_>>().join(",")
    );
    let mut q = sqlx::query(&sql);
    for id in keep {
        q = q.bind(id.as_str());
    }
    q.execute(pool).await.context("pruning inventory")?;
    Ok(())
}
