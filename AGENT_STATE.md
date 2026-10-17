# AGENT_STATE.md — continuity file for working on parasanoid

**Read this first.** When picking up work in the repo, start here instead of
re-browsing `src/`. It captures the architecture map and the current session's
state (done / in progress / next / open questions / pitfalls). Update it at each
checkpoint so a resumed session (compacted/failed context) can resume from this
single file.

_Continuity state, not shipped project code. Committed to the public repo per
Niklas's decision; keep secrets out (record where a secret lives + its env-key
name, never the value)._

---

## Architecture map (src/)

`src/inventory.rs` is the **single source of truth** shared by both roles:
`ClientInventory`, `HostInfo`, `DetectedFs` (+ `BackupStrategy`), `DetectedTool`,
`ClientStatus`, `WorkerStatus`.

| File | Purpose | Confirmed |
|------|---------|-----------|
| `src/inventory.rs` | Shared data model (also: `host_tools_ready` lives in `scan.rs`). | ✅ |
| `src/main.rs` | Binary entrypoint. | ⚠️ verify |
| `src/server/mod.rs` | Server role; builds router + spawns discovery loop. | ✅ |
| `src/server/api.rs` | Server JSON API. `AppState { reg, http, exec, policy }`. Handlers: `health`, `register`, `clients`, `client_detail`, `inventory` (serves stored raw blob). | ✅ |
| `src/server/discovery.rs` | Discovery loop; probes `reg.list()` + static `PARASANOID_CLIENTS` (dedup by URL); stores raw inventory `Value` verbatim via `resp.json::<Value>()`. | ✅ |
| `src/server/state.rs` | `Registry`, `ClientRecord` (id/name/url/up_since/created_at/last_seen/last_inventory), `slug`. | ✅ |
| `src/server/plan.rs` | **M2**: `PolicySet`/`PolicyEntry` (`load_from_file`), `build_sync_plan` (rsync `--link-dest` per source), `run_policy` (execute via `forward_tool`, record per-step), `RunStore` history, control-plane routes. | ✅ |
| `src/server/backup.rs` | `forward_tool_call`, `forward_tool` (shared behind data-plane route). | ✅ |
| `src/server/config.rs` | `Config { listen, poll_interval_secs, stale_after_secs, clients, policy_file }`. `PARASANOID_POLICY_FILE` unset ⇒ scheduler disabled. | ✅ |
| `src/client/mod.rs` | Client role; builds router + spawns heartbeat. | ✅ |
| `src/client/api.rs` | Client API: `health`, `inventory` (serializes `&snap.inventory`), `status`, `refresh` (`POST /api/scan`), `json_error`. | ✅ |
| `src/client/scan.rs` | `collect()` → `ClientInventory` (sets `tools: unwrap_or_default()`), `Snapshot`, `host_tools_ready`. | ✅ |
| `src/client/tools.rs` | `KNOWN_TOOLS` = rsync, sanoid, syncoid, zfs, zpool, btrfs, restic; `collect()` returns `Ok(Vec<DetectedTool>)`. | ✅ |
| `src/client/toolapi.rs` | `POST /api/tools/:tool` — runs a detected tool, never via shell. | ✅ |
| `src/client/plan.rs` | Client side of **M2 hardlink retention**: `GET /api/plan/latest?dest=…`, `POST /api/plan/prune?dest=…&keep=N`. | ✅ |
| `src/client/host.rs` | Host/OS scan. | ⚠️ verify |
| `src/client/heartbeat.rs` | Self-registration pings server. | ⚠️ verify |
| `src/client/fs.rs` | Filesystem scan. | ⚠️ verify |

---

## Server endpoints (authoritative)

Registered in `src/server/mod.rs::run()`:

- `GET  /api/health` — liveness
- `POST /api/discovery/register` — client self-registration (upsert keyed by `client_id`, uses `published_url`)
- `GET  /api/discovery/clients` — list (`ClientSummary`)
- `GET  /api/discovery/clients/:id` — detail
- `GET  /api/discovery/clients/:id/inventory` — raw stored inventory blob (204 if none)
- `POST /api/discovery/clients/:id/tools/:tool` — `backup::forward_tool_call` (data plane)
- `GET  /api/backup/run` — `plan::backup_run` (trigger a run)
- `GET  /api/backup/runs` — `plan::backup_runs` (list runs)
- `GET  /api/backup/run/:id` — `plan::backup_run_detail` (inspect one run)

Client endpoints (`src/client/mod.rs`): `GET /api/health`, `/api/inventory`, `/api/status`,
`POST /api/scan`, `POST /api/tools/:tool`, `GET /api/plan/latest`, `POST /api/plan/prune`.

---

## Current state — M2 (server-owned policy + plan builder + executor)

**Done & verified.** `cargo test` → **12 passed, 0 failed** (was 9; added
`ensure_dest_parent_*` x2 and the prune-`keep` regression test). Release build is
clean with **3** warnings (down from 4: the clamp-style lint is now gone); the
3 remaining are pre-existing dead-code (`one_shot_inventory`, `router`,
`json_error`). Published: commits `d5d4500` (M2, also `origin/master`) and
`a5fee39` (docs/examples), `9a59b10` (this session's bug fixes — **local, not
pushed**; Niklas pushes).

M2 implementation (`d5d4500`, 747 lines): server owns the policy + executor;
every data-touching tool still runs on the client.

- **Policy** (`server/plan.rs`): `PolicySet { entries: Vec<PolicyEntry> }`,
  `PolicyEntry { client_id, sources, destination, retention, timeout_secs }`,
  `PolicySet::load_from_file` → `Arc<Self>`; loaded from `PARASANOID_POLICY_FILE`.
- **Plan** (`build_sync_plan`): expands each source into an `rsync --link-dest`
  call. Snapshot dir shape: `dest/<YYYYmmdd-HHMMSS>/<basename(source)>/`.
- **Executor** (`run_policy`): walks the plan, forwards each call via
  `forward_tool`, records per-step results in a `RunStore`.
- **Retention (rsnapshot-style hardlinks):** each run lands in `dest/<ts>/` and
  links unchanged files against the previous run (`rsync --link-dest`); retention
  is a **count**, not a time window. `second_run_links_against_previous_snapshot`,
  `retention_is_modelled_as_a_count` tests cover this.
- **Client side** (`client/plan.rs`): the server asks the client to list/prune
  `dest/<ts>/` snapshots so it can model hardlink retention.

**E2E bug fixes landed 2026-10-17 (commit `9a59b10`, local only):**

1. `server/plan.rs::latest_snapshot` returned only the ts name, so `--link-dest`
   was relative and rsync looked in the wrong place → hardlinks silently broken.
   Now returns the full `dest/<ts>` path.
2. `server/plan.rs::build_sync_plan` passed the source without a trailing slash,
   so rsync nested `<base>/<base>` and exit-11'd on first run. Now normalized to
   `src + "/"` (contents copy). See `ensure_dest_parent` in `client/toolapi.rs`.
3. `server/plan.rs::prune_snapshots` sent `keep` in the JSON body, but the
   client reads it from `Query<DestParam>` → defaulted to `0` → clamped to `1` →
   retention collapsed to "keep newest only". Now sent in the query string.
4. `server/plan.rs::run_policy` already fixed the previous session: `succeeded`
   is now set `false` when a step actually fails (was only set on pre-execution
   rejection → run reported `succeeded:true` while all steps failed).

All four verified in a real local dual-role run: 2nd run shares inodes with the
1st (hardlinks work), no nested `<base>/<base>`, and 3 snapshots kept with
retention=3. Regression test: `client/plan.rs::prune_keeps_requested_count_from_query`.

---

## Blocker — live run needs a fresh client (NOT a code bug)

Symptom seen earlier: `GET /api/discovery/clients/<id>/inventory` returned
`tools: null`.

**Root cause (verified): a stale running client binary.** The current code cannot
emit `tools: null`:

- `ClientInventory.tools` is a non-optional `Vec<DetectedTool>` (`inventory.rs`).
- `scan::collect()` sets `tools: tools.unwrap_or_default()` (→ `[]` at worst).
- `client/api.rs` serves `serde_json::to_value(&snap.inventory)` — an array.
- `discovery.rs` stores the fetched raw `Value` **verbatim** (`resp.json::<Value>()`).

So a `null` can only come from the served payload, which is **frozen at client
startup** (`scan::collect` is called once in `client/mod.rs::run()`). A binary
built from older code (where `tools` was `Option`/absent) keeps serving the old
shape even after you rebuild.

**Fix:** kill any stale worker on `:9100`, rebuild, relaunch the **release**
client, let discovery re-poll, then confirm `tools` is an array and `rsync`
shows up. Once `rsync` is in inventory, `client_has_tool` passes and the
executor path is valid.

---

## Next (ordered)

1. ~~Rebuild + relaunch the release client on `:9100`, re-poll, confirm `tools`~~
   ✅ done (blocker resolved).
2. ~~End-to-end local dual-role run: snapshots at
   `/tmp/parasanoid-demo/backups/<ts>/sample-data/`, 2nd run hardlinks (inodes
   unchanged), prune honors retention.~~ ✅ done 2026-10-17 (`9a59b10`).
3. ~~Commit example files (`examples/`) + runtime policy (`/tmp`, not committed).~~
   ✅ done (`a5fee39`).
4. ~~Update `ROADMAP.md` (mark M2 done) + THIS file.~~ ✅ done (this session).
5. Fix stale/contradictory `docs/PROJECT-NOTES.md` (§2 still claims no
   self-registration code and lists it as an open decision — it's implemented).
   ⏭️ **next** — out of scope for the e2e pass. `docs/PROJECT-NOTES.md` is the
   known-stale doc; leave it unless Niklas asks.

## Sandbox networking gotcha (important)

Binding a listener to `:8080` / `:9100` fails with
"failed to lookup address information" in this sandbox. **Bind to `127.0.0.1`
(`127.0.0.1:8080` / `127.0.0.1:9100`) instead** — loopback binds fine. Config is
**env-only** (`PARASANOID_*`); `--server`/`--client` select the role, every other
flag is ignored. No Rust on PATH by default (`export PATH="$HOME/.cargo/bin:$PATH"`).

## Open questions (decisions)
- Policy storage: **server-side** (`PARASANOID_POLICY_FILE`, JSON). ✅ decided.
- First scenario: full suite on the local dev box as **server + client**. ✅ decided.

## Run commands (local dual-role demo)
- Client: `PARASANOID_ROLE=client PARASANOID_CLIENT_ID=local-demo PARASANOID_LISTEN=127.0.0.1:9100 PARASANOID_SERVER_URL=http://127.0.0.1:8080 PARASANOID_PUBLISHED_URL=http://127.0.0.1:9100 PARASANOID_HEARTBEAT_INTERVAL=2 RUST_LOG=parasanoid=debug`
- Server: `PARASANOID_ROLE=server PARASANOID_LISTEN=127.0.0.1:8080 PARASANOID_POLL_INTERVAL=2 PARASANOID_POLICY_FILE=/tmp/parasanoid-local-policy.json RUST_LOG=parasanoid=debug`
- Runtime policy `/tmp/parasanoid-local-policy.json`: `local-demo` → source
  `examples/sample-data`, dest `/tmp/parasanoid-demo/backups`, retention 3. (Throwaway.)

## Pitfalls
- **Inventory is frozen at startup** — a stale binary keeps serving an old shape.
  Rebuild the release binary and relaunch before trusting `inventory`.
- Kill any leftover `parasanoid-worker` before relaunching (stale client on `:9100`
  → "Address already in use").
- Don't use bare `vec!["-a".into()]` in plan tests without a `Vec<String>`
  annotation — `.into()` is ambiguous across axum/bytes/reqwest impls.
- `forward_tool_call` match needs a catch-all arm (`StatusCode` is a newtype).
- `/tmp` policies and demo snapshots are throwaway — do not commit them.
- Update THIS file at each checkpoint so a resumed session doesn't have to
  re-browse `src/`.
