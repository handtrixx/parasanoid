# ui-rust — the parasanoid UI, migrated to Rust

This crate is the **production UI**, being migrated from the Next.js app in `../ui`
to a Rust server-rendered UI with **Bootstrap 6 alpha** components. It answers one
question the initial Next.js pass raised — *could the UI process be Rust?
* — and the answer was yes.

It is deliberately kept as a **separate, self-contained crate** so it never
taints the worker's "dumb bridge" boundary (the worker holds no database; SQLite
lives here instead).

> **Status: admin-dashboard shell is done.** Server render + SQLite cache + a
> vertical, collapsible sidebar with four pages — **Dashboard** (`/`),
> **Activities** (`/activities`, every run expandable to its tool invocations),
> **Backups** (`/backups`, run history + the rsync/syncoid/zstd commands each run
> ran), and **Logs** (`/logs`, server-log tail with live follow). The control
> plane (trigger all policies from Activities) is wired; browseable snapshot
> file-contents are not (see the Backups page caveat + `../ROADMAP.md`).

## What it does today
- Owns **SQLite** via `sqlx` (`Send` + async — the reason the worker can't hold a DB).
- Renders HTML **server-side** with `askama` templates (compile-time checked).
- Dials the paranoid **server** JSON API (`/api/discovery/clients` + `/inventory`),
  mirrors clients into SQLite every 10s, and pulls inventory for new clients.
- Serves **Bootstrap 6.0.0-alpha.1** as static assets (`assets/`).

## Bring up the UI in dev

The UI is a **separate crate** (`ui-rust/`) that is **not a member of the repo
workspace**, so you must run it from inside that directory. `cargo run -p
parasanoid-ui-rust` from the repo root will fail with *"package ID specification
did not match any packages"* — that is expected.

```sh
# from the repo root
cd ui-rust

PARASANOID_SERVER_URL=http://localhost:8080 \
PARASANOID_UI_LISTEN=127.0.0.1:3100 \
PARASANOID_DB_PATH=./data.db \
cargo run
```

Then open **http://127.0.0.1:3100** in a browser (server defaults to
`http://localhost:8080`).

**You will see “No clients discovered yet” until the server is running.** The UI
dials the server (`/api/discovery/clients` + `/inventory`) for its data; without
it the table is simply empty (that is normal, not a bug). Start the server first
(see the “The worker” section) and the list populates within a few seconds.

### Notes on running here

- **Bind `127.0.0.1`, not a bare `:3100`.** A bare port (`:3100` / `:8080`) fails
  to resolve here with “failed to lookup address information”; bind an explicit
  address. `localhost` works too.
- `data.db` is scratch (gitignored) — delete it to reset the cache.
- Bootstrap 6.0.0-alpha.1 is vendored in `assets/` (downloaded once); keep it
  committed so the build is reproducible offline. No CDN, no network at dev time.
- Not wired into `compose.yaml`; this is a DX probe.

Env:
- `PARASANOID_SERVER_URL` — paranoid server base URL (default `http://localhost:8080`)
- `PARASANOID_UI_LISTEN`  — listen addr (**bind `127.0.0.1:3100`**, not bare `:3100`)
- `PARASANOID_DB_PATH`    — sqlite file (default `./data.db` inside `ui-rust/`)

## Notes
- Bootstrap 6 is pinned to `6.0.0-alpha.1` in `assets/` (downloaded once; keep it
  committed so the build is reproducible offline).
- `data.db` is scratch — delete it to reset.
- Not wired into `compose.yaml`; this is a DX probe.
