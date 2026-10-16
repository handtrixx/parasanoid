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
├── ui/               # Next.js 16 + MUI 9 UI (initial attempt)
├── ui-rust/          # migration of the UI to Rust server-rendered + Bootstrap 6 alpha
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

- **`ui/`** — Next.js + MUI, dark "console" theme. Initial pass.
- **`ui-rust/`** — the same UI, **being migrated to Rust** (axum server-rendered
  + `askama` templates + SQLite cache, Bootstrap 6 alpha components). See
  `ui-rust/README.md`.

## Data model

`src/inventory.rs` is the single source of truth shared by both roles:
`ClientInventory`, `HostInfo`, `DetectedFs` (+ `BackupStrategy`), `DetectedTool`,
`ClientStatus`, `WorkerStatus`.

## Current status

- **Worker:** builds (debug binary present). Two roles wired; discovery loop polls
  clients over plain LAN HTTP.
- **Data plane:** not implemented — `/api/backup/streams` and `/api/backup/snapshots`
  return `501`. The server is currently purely observatory.
- **UI:** the Next.js UI exists as an initial pass; the real UI direction is the
  Rust migration in `ui-rust`, which is **still in scaffolding**.

## Next steps

See `docs/PROJECT-NOTES.md` for the full list. In priority order: (1) implement the
data plane (execute snapshots / replication), (2) finalize the client model
(self-registration vs. static list — the compose file and code currently disagree),
(3) version-control the project, (4) add tests, (5) make the UI build reproducible.
