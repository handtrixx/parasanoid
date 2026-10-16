# parasanoid

Centralized backup management for ZFS / btrfs / rsync targets. A single Rust
binary runs as a **server** (the "brain") and, in a second role, as a **client**
(the "dumb bridge" on each backup target). A server-rendered Rust UI surfaces the
discovered inventory.

License: **AGPL-3.0-or-later**.

## Roles

`parasanoid-worker` is one binary, two roles, selected by subcommand / flag /
env (in priority order):

| Role | Where it lives | What it does |
|------|----------------|--------------|
| `server` | the storage box | JSON API for the UI + a discovery loop that dials out on the LAN and pulls each client's host inventory. In-memory only — no database, no backup execution. |
| `client` | each backup target | Scans the host (OS, filesystems, tools) and answers the server's poll. Performs no backup work itself. |

Role selection: `server`/`client` subcommand > `--server`/`--client` >
`PARASANOID_ROLE` env var > default `server`.

## Layout

```
.
├── src/              # the dual-role worker (server + client) — builds cleanly
├── ui-rust/          # canonical UI — Rust server-rendered + Bootstrap 6 alpha
├── docker/           # server + client Dockerfiles, deps.sh, per-target compose
├── compose.yaml      # server + UI (v0 deployment)
└── docs/             # project notes (docs/PROJECT-NOTES.md)
```

## The worker

```sh
# server (default)
PARASANOID_LISTEN=:8080 PARASANOID_POLL_INTERVAL=10 \
  PARASANOID_CLIENTS="http://host1:9100,http://host2:9100" \
  cargo run -- server

# client bridge on a target
PARASANOID_CLIENT_ID=host1 PARASANOID_LISTEN=:9100 cargo run -- client
```

Server endpoints: `GET /api/health`, `/api/discovery/clients`,
`/api/discovery/clients/{id}`, `/api/discovery/clients/{id}/inventory`.
Client endpoints: `GET /api/health`, `/api/inventory`, `/api/status`,
`POST /api/scan` (backup endpoints are stubbed with `501` until the data plane
exists).

## UI

- **`ui-rust/`** — the **canonical UI**. Rust server-rendered with axum +
  `askama` templates + a SQLite cache, Bootstrap 6 alpha components. This is the
  UI direction; see `ui-rust/README.md`.
- **`ui/`** — the initial Next.js + MUI pass. **Removed 2026-10-16** once the Rust
  UI scaffold was confirmed working (`docs/PROJECT-NOTES.md` → Decisions).
  Not wired into `compose.yaml`.

## Data model

`src/inventory.rs` is the single source of truth shared by both roles:
`ClientInventory`, `HostInfo`, `DetectedFs` (+ `BackupStrategy`), `DetectedTool`,
`ClientStatus`, `WorkerStatus`.

## Current status

- **Worker:** builds (debug binary present). Two roles wired; discovery loop polls
  clients over plain LAN HTTP.
- **Data plane:** not implemented — `/api/backup/streams` and `/api/backup/snapshots`
  return `501`. The server is currently purely observatory.
- **UI:** the Rust UI (`ui-rust`) is the **canonical UI**; the Next.js UI (`ui/`)
  was removed 2026-10-16. The Rust UI is functional as a read-only
  discovery/inventory view but is still **scaffolding** — no control plane and not
  yet wired into `compose.yaml`.

## Next steps

See `docs/PROJECT-NOTES.md` for the full list. In priority order: (1) implement the
data plane (execute snapshots / replication), (2) finalize the client model
(self-registration vs. static list — the compose file and code currently disagree),
(3) version-control the project, (4) add tests, (5) make the UI build reproducible.
