# parasanoid — session summary & next steps

> Condensed from a bloated session transcript (was ~10,760 lines of repeated
> re-inspection + code dumps). Full detailed inspection lives in
> [`PROJECT-NOTES.md`](./PROJECT-NOTES.md); project overview in
> [`README.md`](../../README.md). Read this one and the linked notes; they cover it.

## Original ask
"Inspect the `parasanoid` project, write notes about it and the next steps
required. Don't continue developing." Later clarification: `ui-rust/` is **not**
a throwaway probe — it's the **in-progress migration of the Next.js UI into Rust
(server-rendered + Bootstrap 6 alpha)** that stopped mid-scaffolding.

## What `parasanoid` is
Centralized **ZFS / btrfs / rsync backup manager**. One Rust binary,
`parasanoid-worker`, runs in two roles:
- **server** ("brain") — JSON API for the UI + a discovery loop that dials out on
  the LAN and pulls each client's host inventory. In-memory only; no DB, no
  backup execution.
- **client** ("dumb bridge") — on each target, scans host/fs/tools and answers
  the server's poll. Performs no backup work, holds no DB.

Scan model is shared by both roles in `src/inventory.rs`.

## The three codebases
| Path | Role | Status |
|------|------|--------|
| `src/` | Dual-role worker (server + client) | Builds (debug binary present) |
| `ui/src/` | Initial Next.js 16 + React 19 + MUI 9 UI (dark "console" theme) | Source only; superseded by the Rust migration |
| `ui-rust/src/` | **Real UI — migrated here to Rust** (axum server-render + `askama` + SQLite cache, Bootstrap 6 alpha) | **In progress, stopped mid-scaffolding**; not wired into compose |

Server is currently **purely observatory**: `/api/backup/streams` and
`/api/backup/snapshots` return **501**.

## Next steps (most important first)
1. **Implement the data plane** — the actual product: execute the matching
   `BackupStrategy` (ZFS via `sanoid`/`syncoid`, btrfs via `send`/`receive`,
   ext4/xfs via `rsync`/`restic`). Big open chunk.
2. **Resolve the client-model contradiction** — compose sets
   `PARASANOID_SERVER_URL`/`PARASANOID_PUBLISHED_URL` and claims self-registration,
   but `src/` never reads them; the server instead dials a static
   `PARASANOID_CLIENTS` list. Pick one, fix docs.
3. **Version-control the project** — not a git repo despite `Cargo.toml`
   referencing a remote.
4. **Add tests** — none exist.
5. **Reproducible UI build** — no lockfile committed.
6. Plus: compose `healthcheck`, `.env.example`, secure plain-LAN HTTP, root README.

## Notable issues (minor, no action taken)
- `docker/deps.sh` has a corrupted comment block with stray JSON (copy-paste
  contamination). Harmless but messy.
- Duplicate `slug()` in `state.rs` + `discovery.rs`; a new `reqwest::Client`
  built per probe; ui-rust silently swallows discovery errors.

## Done this session
- Wrote root `README.md` (correct framing: `ui-rust` is the migration-in-progress).
- Wrote `docs/PROJECT-NOTES.md` (full inspection).
- Updated `ui-rust/README.md` to reflect migration-in-progress status.

## Validation
Inspected every source file, compose, Dockerfile, `deps.sh`, template; confirmed
worker builds and the gaps (no VCS, no tests, no UI lockfile, dead env vars).
Not run: release build, `docker compose config`, `cargo test`.
