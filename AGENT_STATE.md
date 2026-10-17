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
| `src/client/plan.rs` | Client side of **M2 hardlink retention** (`GET /api/plan/latest?dest=…`, `POST /api/plan/prune?dest=…&keep=N`) **+ M3 ZFS retention** (`GET /api/plan/zfs/list?dataset=…`, `POST /api/plan/zfs/prune?dataset=…&keep=N`). M3 in progress — see below. | 🟢 |
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
`POST /api/scan`, `POST /api/tools/:tool`, `GET /api/plan/latest`, `POST /api/plan/prune`,
`GET /api/plan/zfs/list` (M3), `POST /api/plan/zfs/prune` (M3, **uncommitted**).

---

## Current state — M2 (server-owned policy + plan builder + executor)

**Done & verified.** `cargo test` → **12 passed, 0 failed** (was 9; added
`ensure_dest_parent_*` x2 and the prune-`keep` regression test). Release build is
clean with **3** warnings (down from 4: the clamp-style lint is now gone); the
3 remaining are pre-existing dead-code (`one_shot_inventory`, `router`,
`json_error`). Published (on `origin/master`): `d5d4500` (M2), `a5fee39` (docs/examples),
`9a59b10` (this session's bug fixes), plus `9e859d3` (this doc pass). All four
were pushed on request 2026-10-17.

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

**E2E bug fixes landed 2026-10-17 (commit `9a59b10`, pushed):

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

## M3 (ZFS `Snapshot` strategy) — UNTESTED WIP, deliberately left uncommitted

The session after M2 implemented **M3 (ROADMAP: the `BackupStrategy::Snapshot`
strategy)**. The production logic compiles and **all 12 existing tests still pass**
(`cargo test` green); the old "duplicate-route panic / cargo test fails" framing in
this file is **stale** — that was an intermediate half-edit that was already cleaned
up. It is deliberately **left uncommitted** (not interrupt debris, just unfinished):
no ZFS host exists on the dev box, so it cannot be E2E-verified here, and the UI focus
is elsewhere until one does.

Committed baseline: `5125125` (docs: record M2) — this is `origin/master`. The 3 commits
ahead (`ed54db6` / `876d60d` / `6d1850e`) are the Bootstrap-6 UI + `start-dev.sh` and are
also **unpushed**. Everything below (the `src/` M3 files) is **uncommitted** — do not
`git add src/` for the UI commit.

**What the M3 edit got done (the production logic in these files is correct):**

- `src/inventory.rs` — `BackupStrategy` gains `#[derive(Default)]` + `#[default]` on
  `Copy`. A policy written before ZFS existed omits `strategy` ⇒ **rsync**, not "take a
  snapshot".
- `src/client/toolapi.rs` — `ToolCall` gains `tool: String` (default `"rsync"`), so the
  executor knows which `/api/tools/:tool` route each plan call targets (a ZFS run mixes
  `zfs` + `syncoid` calls). The in-file test helper was already updated to build a
  `ToolCall`.
- `src/client/plan.rs` — `DatasetParam { dataset, keep }` query struct; `zfs_list`
  (`GET /api/plan/zfs/list?dataset=…`) and `zfs_prune` (`POST /api/plan/zfs/prune?
  dataset=…&keep=…`) that enumerate/`destroy` the `parasanoid-*` snapshots `syncoid`
  leaves on the target dataset; `zfs_destroy` helper.
- `src/server/plan.rs` — `PolicyEntry` gains `strategy: BackupStrategy` (+ `Default` impl);
  `build_snapshot_plan()` emits per source the 3-call sequence
  `[zfs snapshot {src}@tag -u, syncoid {src}@tag {dest}, zfs destroy {src}@tag]` (tag
  `parasanoid-<ts>`); `tools_for()`/`ready_rec()` gate execution on the strategy's tools
  (`Copy`→`rsync`, `Snapshot`→`zfs`,`syncoid`); `run_policy()` dispatches Copy ⇒
  `build_sync_plan` + `prune_snapshots`, Snapshot ⇒ `build_snapshot_plan` +
  `prune_zfs_snapshots`; `execute_plan()` uses `call.tool` as the route segment; `AppState`
  gains `zfs_snapshot_list()` + `prune_zfs_snapshots()`.

**What's left unfinished (fix these first to get a clean build):**

1. **`src/client/mod.rs` duplicate routes (runtime panic).** The ZFS routes were added but
   the edit duplicated `/api/plan/latest`, `/api/plan/prune`, and `/api/plan/zfs/list` —
   each registered twice (see lines 117-118 / 146-147 / 150-151) alongside the new
   `/api/plan/zfs/prune` (line 156). **Dedupe to one of each** plus the one new route:
   `GET /api/plan/latest`, `POST /api/plan/prune`, `GET /api/plan/zfs/list`,
   `POST /api/plan/zfs/prune`.
2. **`src/server/backup.rs` lines ~147 & ~174** — the two test `ToolCall { … }` initializers
   are missing the new `tool:` field (`missing field 'tool'`). Add `tool: "rsync"` (the
   route segment the test exercises).
3. **`src/server/plan.rs` line ~634** — the `entry()` test helper's `PolicyEntry { … }`
   is missing `strategy:` (`missing field 'strategy'`). Add `strategy: BackupStrategy::Copy`.
4. **`AppState::zfs_snapshot_list()`** (server/plan.rs ~550) is defined but never called →
   dead-code warning. Either wire it into a client-detail/plan endpoint or remove it. The
   build passes; fix this to keep the warning count flat.

**Verify the fix:** `cargo test` (expect all green; was 12 before the M3 test inits),
`cargo build` (clean, no new warnings).

**Remaining M3 work after a clean build (NOT yet started):**

- Add M3 tests: a unit test that `build_snapshot_plan` emits the 3-call
  snapshot/syncoid/destroy sequence per source tagged `parasanoid-<ts>`, and that
  `run_policy` routes `Snapshot` → `zfs`+`syncoid` and gates on those tools being in
  inventory; router/integration tests for `GET /api/plan/zfs/list` and
  `POST /api/plan/zfs/prune` on the client.
- **E2E caveat:** this box is **ext4/xfs, not ZFS** — real `zfs snapshot`/`syncoid` send-
  receive cannot run here. Verify M3 via the built plan + forwarded tool-call sequence and
  the client list/prune endpoints (the client handlers shell out to `zfs`, which is
  absent here → expect them to log "zfs list failed" and return empty, which is fine for a
  plan-shape test). A real ZFS send/receive E2E needs a ZFS host.
- Then commit locally (**no push** — Niklas pushes), and update `ROADMAP.md` (M3 ⏸️→✅)
  and this file.

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
