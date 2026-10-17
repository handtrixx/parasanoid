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
`/api/discovery/clients/{id}`, `/api/discovery/clients/{id}/inventory`,
`POST /api/discovery/register` (self-registration). Client endpoints:
`GET /api/health`, `/api/inventory`, `/api/status`, `POST /api/scan`, and
`POST /api/tools/{tool}` — the data plane: run a detected host tool with a
plain argv vector (never through a shell).

## UI

- **`ui-rust/`** — the **canonical UI**. Rust server-rendered with axum +
  `askama` templates + a SQLite cache, Bootstrap 6 alpha components. This is the
  UI direction; see `ui-rust/README.md` (how to run it in dev is there).
- **`ui/`** — the initial Next.js + MUI pass. **Removed 2026-10-16** once the Rust
  UI scaffold was confirmed working (`docs/PROJECT-NOTES.md` → Decisions).
  Not wired into `compose.yaml`.

### In dev

```sh
# worker server (the "brain"), default :8080 — see "The worker" above
cargo run -- server

# the Rust UI (separate crate; not part of the repo workspace)
cd ui-rust
PARASANOID_SERVER_URL=http://localhost:8080 \
PARASANOID_UI_LISTEN=127.0.0.1:3100 \
PARASANOID_DB_PATH=./data.db \
cargo run
# open http://127.0.0.1:3100   (the list is empty until the server above is running)
```

## Data model

`src/inventory.rs` is the single source of truth shared by both roles:
`ClientInventory`, `HostInfo`, `DetectedFs` (+ `BackupStrategy`), `DetectedTool`,
`ClientStatus`, `WorkerStatus`.

## Current status

- **Worker:** builds (debug binary present). Two roles wired; discovery loop polls
  clients over plain LAN HTTP.
- **Data plane:** implemented — `POST /api/tools/{tool}` runs a detected host tool
  (validated against `KNOWN_TOOLS`, JSON envelope result, per-call timeout); the
  server forwards calls to the client that can run them.
- **Tests:** 4/4 green (data-plane router integration). Coverage gaps remain
  (`slug`, `host_tools_ready`, server control-plane routes — see `ROADMAP.md`).
- **UI:** the Rust UI (`ui-rust`) is the **canonical UI**; the Next.js UI (`ui/`)
  was removed 2026-10-16. The Rust UI is functional as a read-only
  discovery/inventory view but is still **scaffolding** — no control plane and not
  yet wired into `compose.yaml`.

## Working on this project

When developing `parasanoid`, read **`AGENT_STATE.md`** first — it holds the
`src/` architecture map plus the current session's focus, last changes, and next
steps, so orientation doesn't require re-reading the source. Update it at each
checkpoint; it is agent continuity state (committed to the repo, secrets kept
out).

## Roadmap

See `ROADMAP.md` for the live, prioritized plan (M0 foundations → M1 data plane →
M2 policy/plan builder → M3–M4 strategies → M5 UI control plane). M0 and M1 are
**done**; the rest is planned there.
