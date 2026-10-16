# ui-rust — the parasanoid UI, migrated to Rust

This crate is the **production UI**, being migrated from the Next.js app in `../ui`
to a Rust server-rendered UI with **Bootstrap 6 alpha** components. It answers one
question the initial Next.js pass raised — *could the UI process be Rust?
* — and the answer was yes.

It is deliberately kept as a **separate, self-contained crate** so it never
taints the worker's "dumb bridge" boundary (the worker holds no database; SQLite
lives here instead).

> **Status: scaffolding only.** The migration stopped mid-process. The skeleton
> (server render + SQLite cache + client list/detail pages) exists; pages, styling,
> and the control plane (triggering backups) are still to come.

## What it does today
- Owns **SQLite** via `sqlx` (`Send` + async — the reason the worker can't hold a DB).
- Renders HTML **server-side** with `askama` templates (compile-time checked).
- Dials the paranoid **server** JSON API (`/api/discovery/clients` + `/inventory`),
  mirrors clients into SQLite every 10s, and pulls inventory for new clients.
- Serves **Bootstrap 6.0.0-alpha.1** as static assets (`assets/`).

## Run
```sh
# from the parasanoid repo root
PARASANOID_SERVER_URL=http://localhost:8080 \
PARASANOID_UI_LISTEN=:3100 \
PARASANOID_DB_PATH=./ui-rust/data.db \
cargo run -p parasanoid-ui-rust
# open http://localhost:3100  (server defaults to http://localhost:8080)
```

Env:
- `PARASANOID_SERVER_URL` — paranoid server base URL (default `http://localhost:8080`)
- `PARASANOID_UI_LISTEN`  — listen addr (default `:3100`)
- `PARASANOID_DB_PATH`    — sqlite file (default `./ui-rust/data.db`)

## Notes
- Bootstrap 6 is pinned to `6.0.0-alpha.1` in `assets/` (downloaded once; keep it
  committed so the build is reproducible offline).
- `data.db` is scratch — delete it to reset.
- Not wired into `compose.yaml`; this is a DX probe.
