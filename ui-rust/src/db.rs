//! SQLite layer — the one thing that *must* live in the UI process.
//!
//! `rusqlite::Connection` is `!Send`, which is why the worker can't hold a DB.
//! `sqlx`'s `SqlitePool` is `Send` + async, so it fits the axum handler model.
//! This holds the discovered-client list and cached inventory snapshots so the
//! page renders quickly and survives a server blip.

use anyhow::{Context, Result};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePool, SqlitePoolOptions};
use sqlx::Row;

use crate::models::{ClientSummary, Inventory, RunModel, StepModel};

/// Upper bound on the number of runs we keep. The server keeps runs in memory
/// only (lost on restart) and caps `recent()` at 500; mirror that ceiling here
/// so the table does not grow without bound across many dev sessions.
pub const RUNS_KEEP: usize = 500;

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

-- Backup runs. Cached from the server's in-memory RunStore, keyed by run_id
-- (a uuid), with the per-step payloads in JSON so the detail page can render
-- what each run actually executed.
CREATE TABLE IF NOT EXISTS backup_runs (
    run_id      TEXT PRIMARY KEY,
    client_id   TEXT NOT NULL,
    started_at  TEXT NOT NULL,
    ended_at    TEXT,
    succeeded   INTEGER NOT NULL,
    step_count  INTEGER NOT NULL DEFAULT 0,
    steps       JSON NOT NULL,
    fetched_at  TEXT NOT NULL
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

/// Upsert a single backup run (dedup by run_id).
pub async fn upsert_run(pool: &SqlitePool, r: &RunModel) -> Result<()> {
    let steps = serde_json::to_string(&r.steps).context("serializing run steps")?;
    let step_count = r.steps.len();
    sqlx::query(
        "INSERT INTO backup_runs (run_id, client_id, started_at, ended_at, succeeded, step_count, steps, fetched_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT(run_id) DO UPDATE SET
           client_id=excluded.client_id,
           started_at=excluded.started_at,
           ended_at=excluded.ended_at,
           succeeded=excluded.succeeded,
           step_count=excluded.step_count,
           steps=excluded.steps",
    )
    .bind(&r.run_id)
    .bind(&r.client_id)
    .bind(&r.started_at)
    .bind(r.ended_at.as_deref())
    .bind(r.succeeded as i64)
    .bind(step_count as i64)
    .bind(steps)
    .bind(now_rfc3339())
    .execute(pool)
    .await
    .context("upserting run")?;
    Ok(())
}

/// Keep only the most-recently-started `RUNS_KEEP` runs (newest by started_at).
pub async fn prune_runs(pool: &SqlitePool) -> Result<()> {
    let sql = format!(
        "DELETE FROM backup_runs WHERE run_id NOT IN (
            SELECT run_id FROM backup_runs ORDER BY started_at DESC LIMIT {n}
         )",
        n = RUNS_KEEP
    );
    sqlx::query(&sql)
        .execute(pool)
        .await
        .context("pruning runs")?;
    Ok(())
}

/// Which known run ids are not in the DB yet — used so refresh only fetches the
/// deltas rather than re-downloading the whole (possibly large) run list.
pub async fn new_run_ids(pool: &SqlitePool, ids: &[String]) -> Vec<String> {
    if ids.is_empty() {
        return Vec::new();
    }
    let placeholders = ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let sql = format!(
        "SELECT run_id FROM backup_runs WHERE run_id IN ({placeholders})",
        placeholders = placeholders
    );
    let mut q = sqlx::query_as::<_, (String,)>(&sql);
    for id in ids {
        q = q.bind(id.as_str());
    }
    let known: Vec<String> = q
        .fetch_all(pool)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|(s,)| s)
        .collect();
    ids.iter()
        .filter(|id| !known.contains(*id))
        .cloned()
        .collect()
}

/// Fetch the most-recent full runs (with steps decoded) from the cache.
/// Newest-first, capped at `limit`.
pub async fn load_recent(pool: &SqlitePool, limit: usize) -> Result<Vec<RunModel>> {
    let rows = sqlx::query(
        "SELECT run_id, client_id, started_at, ended_at, succeeded, steps \
           FROM backup_runs ORDER BY started_at DESC LIMIT ?",
    )
    .bind(limit as i64)
    .fetch_all(pool)
    .await
    .context("loading recent runs")?;
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let run_id: String = row.try_get("run_id")?;
        let client_id: String = row.try_get("client_id")?;
        let started_at: Option<String> = row.try_get("started_at")?;
        let ended_at: Option<String> = row.try_get("ended_at")?;
        let succeeded: i64 = row.try_get("succeeded")?;
        let steps_json: String = row.try_get("steps")?;
        let steps: Vec<StepModel> = serde_json::from_str(&steps_json).unwrap_or_default();
        out.push(RunModel {
            run_id,
            client_id,
            started_at,
            ended_at,
            succeeded: succeeded != 0,
            steps,
        });
    }
    Ok(out)
}

/// Decode and return a single full run (with steps) from the cache.
pub async fn load_run(pool: &SqlitePool, run_id: &str) -> Result<Option<RunModel>> {
    let row = sqlx::query(
        "SELECT run_id, client_id, started_at, ended_at, succeeded, steps \
           FROM backup_runs WHERE run_id=?",
    )
    .bind(run_id)
    .fetch_optional(pool)
    .await
    .context("loading run")?;
    let Some(row) = row else {
        return Ok(None);
    };
    let run_id: String = row.try_get("run_id")?;
    let client_id: String = row.try_get("client_id")?;
    let started_at: Option<String> = row.try_get("started_at")?;
    let ended_at: Option<String> = row.try_get("ended_at")?;
    let succeeded: i64 = row.try_get("succeeded")?;
    let steps_json: String = row.try_get("steps")?;
    let steps: Vec<StepModel> = serde_json::from_str(&steps_json).unwrap_or_default();
    Ok(Some(RunModel {
        run_id,
        client_id,
        started_at,
        ended_at,
        succeeded: succeeded != 0,
        steps,
    }))
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
