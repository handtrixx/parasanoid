# parasanoid — project inspection notes

> Written 2026-10-16. Inspection only — no code changes made.

## What it is

`parasanoid` is a **centralized ZFS / btrfs / rsync backup manager**. A single
Rust binary, `parasanoid-worker`, runs in one of two roles chosen explicitly:

- **server** — the "brain" (storage box). JSON API for the UI + a discovery loop
  that dials out to clients on the LAN and pulls their host inventory.
- **client** — a "dumb bridge" on each backup target. Scans the host and answers
  the server's poll. Performs **no backup work** and holds **no database** — the
  design deliberately keeps all backup execution server-side.

Role precedence: `server`/`client` subcommand > `--server`/`--client` flag >
`PARASANOID_ROLE` env var > default (`server`). Both roles set up tracing and run
their own tokio runtime; `main.rs` only dispatches.

**Note on naming:** the repo/directory is `parasanoid` and the binary is
`parasanoid-worker`; the crate is `parasanoid-worker` (AGPL-3.0-or-later).

## Architecture

```
┌───────────────── parasanoid-worker server ─────────────────┐
│  JSON API (Next.js UI talks here)                           │
│    GET /api/health                                          │
│    GET /api/discovery/clients                               │
│    GET /api/discovery/clients/{id}                          │
│    GET /api/discovery/clients/{id}/inventory                │
│                                                             │
│  Discovery loop (every PARASANOID_POLL_INTERVAL, default 10s)│
│    → GET {client}/api/health  (liveness)                    │
│    → GET {client}/api/inventory (host inventory)            │
│    → in-memory Registry (HashMap, tokio RwLock)             │
└─────────────────────────────────────────────────────────────┘
            HTTP (same-LAN, plain)   ▲
                                   │
┌───────────────── parasanoid-worker client ─────────────────┐
│  LISTEN :9100 (PARASANOID_LISTEN)                           │
│    GET  /api/health      → ClientStatus                     │
│    GET  /api/inventory   → ClientInventory                  │
│    GET  /api/status      → WorkerStatus                     │
│    POST /api/scan        → force fresh scan                 │
│    GET  /api/backup/streams    → 501 (stub)                 │
│    POST /api/backup/snapshots  → 501 (stub)                 │
└─────────────────────────────────────────────────────────────┘
```

Client scan sources (`src/client/`):
- `host.rs` — hostname, `/etc/os-release`, `uname -r/-m`, `/proc/uptime`,
  `/proc/meminfo` (Linux only; best-effort fallback to `unknown`).
- `fs.rs` — groups `/proc/mounts` fstypes with counts; if ZFS userland is present
  (`zfs list`), overrides the zfs count with the authoritative dataset count.
- `tools.rs` — probes `rsync sanoid syncoid zfs zpool btrfs restic` via
  `tool --version` (fallback `-V`); non-zero exit = unavailable.
- `scan.rs` — assembles `ClientInventory`; `host_tools_ready` = rsync present.

Data model lives in one place, `src/inventory.rs` (shared by both roles), so the
two stay in lock-step: `ClientInventory`, `HostInfo`, `DetectedFs` (+
`BackupStrategy` enum: `snapshot` | `copy` | `unsupported`), `DetectedTool`,
`ClientStatus`, `WorkerStatus`.

## Roles of the code

| Path | What it is | Status |
|------|-----------|--------|
| `src/` (worker) | The dual-role binary (server + client) | Builds (debug binary present) |
| `ui/src/` | **Initial pass** — Next.js 16 + React 19 + MUI 9, dark theme (superseded by the Rust migration) | Compiled source only |
| `ui-rust/src/` | **The real UI — being migrated here to Rust** (server-rendered axum + `askama` + SQLite cache, Bootstrap 6 alpha) | **In progress, stopped mid-scaffolding**; not wired into compose |
| `compose.yaml` | Server + Next.js UI on one compose network | v0 deployment |
| `docker/client.compose.yaml` | Per-target privileged client image | Template (placeholder values) |
| `docker/server.Dockerfile` | Two-stage, static binary, no DB | |
| `docker/client.Dockerfile` | Worker + rsync/btrfs/zfs userland + sanoid/syncoid from pinned tarball | |
| `docker/deps.sh` | `install`/`verify` for the client image tooling | See issues below |

Env vars in use:
- Both: `PARASANOID_ROLE` (role).
- Server: `PARASANOID_LISTEN` (`:8080`), `PARASANOID_POLL_INTERVAL` (`10`),
  `PARASANOID_CLIENTS` (comma-sep static fallback list, air-gapped clients).
- Client: `PARASANOID_CLIENT_ID`, `PARASANOID_LISTEN` (`:9100`).
- ui-rust only: `PARASANOID_SERVER_URL` (`http://localhost:8080`),
  `PARASANOID_UI_LISTEN` (`:3100`), `PARASANOID_DB_PATH` (`./ui-rust/data.db`).

## Next steps required

These are ordered roughly from "unblocks a usable product" to "polish."

### 1. Implement the data plane (the actual product)
The two `/api/backup/*` endpoints return **501**. The server is currently purely
observatory (discovers inventory); it does **not** trigger or run backups. The
real value is: given an inventory, execute the matching `BackupStrategy` —
ZFS via `sanoid`/`syncoid` (retention + replication), btrfs via `send`/`receive`,
ext4/xfs via `rsync`/`restic`. This is the big open chunk of work.

### 2. Finish (or drop) the client model — self-registration vs static list
`docker/client.compose.yaml` sets `PARASANOID_SERVER_URL` and
`PARASANOID_PUBLISHED_URL` and its comment says the client "self-registers with
the server." **Neither env var is read by `src/`, and there is no self-registration
code** — the server instead dials a static `PARASANOID_CLIENTS` list and seeds it
as a fallback. The compose doc and the code contradict each other. Decide one:
- self-registration (client calls server on boot), or
- finalize the server-dials-static-list model and remove the dead env vars/docs.

### 3. Version-control the project
Not a git repo yet, despite `Cargo.toml` referencing
`https://git.handtrixxx.com/parasanoid/parasanoid.git`. `docs/` is empty.
Commit (locally, per the KB rules — Niklas pushes). Decide what to track: the
worker builds cleanly, so a first commit is straightforward.

### 4. Add tests
None exist (`#[test]` is absent everywhere). `client/mod.rs` references tests
that don't exist. Worth at least: `inventory` serialization, `slug()`,
`strategy_for`, `host_tools_ready`, and router integration tests for both roles.

### 5. Make the Next.js UI build reproducible
No `pnpm-lock.yaml` and no `package-lock.json` are present. `ui/Dockerfile` copies
`package-lock.json pnpm-lock.yaml*` and runs `pnpm install --frozen-lockfile ||
pnpm install`; without a committed lockfile the build pins runtime but isn't
reproducible. Generate + commit a lockfile. (Also the image uses `node:26-alpine`
/ `.nvmrc` 26 — verify that toolchain actually exists before relying on it.)

### 6. Lock down the UI stack / toolchain
The UI pins bleeding-edge versions (Next 16.3.8, React 19.2.8, MUI 9.4.0,
TypeScript 7, oxlint 1.87). Fine for a probe; verify the toolchain is sane before
production use.

### 7. Security hardening (already flagged in code comments)
Discovery + client bridge is plain same-LAN HTTP. `Cargo.toml` notes future
mutual-TLS / server pinning; the client router also leaves CORS on for
`next dev` (comment says drop it in prod). Plan a concrete lock-down before this
touches any non-LAN network.

### 8. Deployment hygiene
- Add a compose `healthcheck` for server/UI (none currently).
- Add a `.env.example` (`.dockerignore` already references one).
- ui-rust creates `data.db` at runtime — add a `.gitignore`/cleanup so scratch DB
  isn't committed.

## Issues found while inspecting (minor, no action taken)

- **`docker/deps.sh` has a corrupted comment block** (~lines 9–10, 18): it
  contains stray JSON that looks like copy-paste contamination from another config
  (an `agent_servers` block). Harmless (inside a `#` comment) but messy — should
  be cleaned up.
- **Duplicate `slug()`** implemented identically in `src/server/state.rs` and
  `src/server/discovery.rs` — a small refactor candidate.
- **`discovery.rs` builds a new `reqwest::Client` per probe** in `probe_target`;
  it should be constructed once and shared.
- **ui-rust swallows errors** in `refresh()` via `... .await.ok()` /
  `.unwrap_or_default()` — a discovery hiccup is silent; consider surfacing or
  at least tracing it.
- **No root README.** Only `ui-rust/README.md` exists; the worker and UI have none.
- **UI is mid-migration:** the Next.js UI (`ui/`) is the initial pass; the real UI
  direction is `ui-rust` (Rust server-rendered + Bootstrap 6 alpha), which stopped
  during scaffolding. The repo currently has two parallel UIs until the migration
  is finished or the Next.js one is dropped.

## Verification done

- Read all source in `src/`, `ui/src/`, `ui-rust/src/`, compose, Dockerfiles,
  `deps.sh`, and templates.
- Worker compiles: `target/debug/parasanoid-worker` is present (built previously).
- Confirmed: no VCS, no `#[test]`, no root README, empty `docs/`, no UI lockfile,
  and `PARASANOID_SERVER_URL`/`PARASANOID_PUBLISHED_URL` are dead in the client path.

Not run: full release build (`cargo build --release`), `docker compose config`,
or `cargo test` (nothing to run). These are left as suggested next-step checks.
