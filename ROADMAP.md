# parasanoid — roadmap

Planning doc for `parasanoid`. See `README.md` for what it *is* today and
`docs/PROJECT-NOTES.md` for the detailed history and decisions. This file is the
live plan — keep it current as increments land.

**Status legend:** ✅ done · 🟢 in progress · ⏭️ next · ⏸️ deferred

---

## At a glance

`parasanoid` is a centralized backup manager for ZFS / btrfs / rsync targets. One
Rust binary runs in two roles:

- **server** (the storage box) — JSON API for the UI + a discovery loop that dials
  out on the LAN and pulls each client's host inventory.
- **client** (each backup target) — scans the host (OS, filesystems, tools) and
  exposes generic tool-invocation routes the server can call.

The product value lives in the **data plane**: the server orchestrates a backup by
building a plan and **forwarding tool calls to the client that can run them**.
Execution always stays on the target host; the server never touches the data.

---

## Milestones

### M0 — Foundations  ✅

Dual-role binary, discovery loop, client self-registration, inventory data model
(`src/inventory.rs`), and the Rust UI scaffold (`ui-rust/`).

- Discovery loop polls `GET /api/health` + `/api/inventory` on the LAN
  (`src/server/discovery.rs`).
- Client self-registers via heartbeat; static `PARASANOID_CLIENTS` list is only an
  air-gapped fallback (`src/client/heartbeat.rs`, `src/server/state.rs`).
- Inventory is the single source of truth shared by both roles
  (`src/inventory.rs`): `ClientInventory`, `HostInfo`, `DetectedFs`,
  `DetectedTool`, `ClientStatus`, `WorkerStatus`, and the `BackupStrategy` enum.
- Public repo: <https://github.com/handtrixx/parasanoid>.

### M1 — Data plane  ✅  _(2026-10-17)_

The server can now call **any tool the client reports**, availability-checked.

- **Client** (`src/client/toolapi.rs`): `POST /api/tools/:tool` runs a detected
  tool with a plain argv vector (**never through a shell**), validates the name
  against `KNOWN_TOOLS`, returns a JSON envelope
  (`{tool, exit_code, success, elapsed_ms, stdout, stderr}`) with a per-call
  timeout.
- **Server** (`src/server/backup.rs`): `forward_tool_call` rejects calls to tools
  not present in the client's inventory, forwards to the client's
  `/api/tools/:tool`, returns the result (or 502). Shared `reqwest::Client` on
  `AppState`.
- Router integration tests cover both roles (`cargo test`, 4/4).

### M2 — Backup policy → plan builder  ✅  _(2026-10-17)_

Server owns **what** to back up, **where**, and **with what retention**, and turns
that into an executable sequence. Data still runs on the client.

1. **Policy** (`server/plan.rs`): `PolicySet { entries: Vec<PolicyEntry> }`,
   `PolicyEntry { client_id, sources, destination, retention, timeout_secs }`,
   loaded from `PARASANOID_POLICY_FILE` (server-side JSON — the decided source of
   truth). Empty file ⇒ scheduler disabled.
2. **Plan builder** (`build_sync_plan`): policy → ordered `Vec<ToolCall>`. Each
   source becomes one rsync call under `dest/<ts>/<basename(source)>/`.
3. **Executor** (`run_policy`): walks the plan, forwards each call via
   `forward_tool`, records per-step results (exit code, success, duration) in a
   `RunStore`; marks the run failed if any step fails or is rejected pre-execution.
4. **Hardlink retention** (rsnapshot-style): unchanged files on run N+1 are
   hardlinked against run N via `rsync --link-dest`; retention is a **count**.

**E2E-verified bug fixes (this lands the milestone):**

- `--link-dest` must be the **full** `dest/<ts>` path, not the bare ts name: the
  client returns only the ts, but rsync resolves `--link-dest` relative to the
  **new** snapshot's destination, so a bare name pointed at the wrong place and
  hardlinks silently failed. Fixed in `latest_snapshot` (returns `dest/<ts>`).
- Source args need a **trailing slash**: rsync copies the directory *contents*
  into `dest/<ts>/<base>` instead of nesting `<base>/<base>` (which also broke
  hardlinking by nesting one level too deep).
- `prune_snapshots` sends `keep` in the **query string** — the client reads it
  from `Query<DestParam>`. It used to be in the JSON body, where it parsed to its
  default `0` → clamped to `1`, collapsing retention to "keep newest only".

**Decisions (closed):** policy source = server-side JSON file; first strategy =
rsync for ext4/xfs; hardlink (not bare copy) retention. Closed 2026-10-17.

### M3 — ZFS strategy  ⏸️

`Snapshot` strategy for ZFS: `zfs snapshot` → `syncoid` replication +
`sanoid` retention, forwarded as tool calls on the client.

### M4 — btrfs / rsync / restic strategies  ⏸️

- btrfs: `zfs-like` snapshot/replication via `btrfs send | receive`.
- ext4/xfs: `Copy` strategy via `rsync` (bare copy) or `restic` (encrypted,
  remote-repo, dedup).

### M5 — Control plane in the UI  ⏸️

Today the Rust UI (`ui-rust`) is a read-only discovery/inventory view. Once the
executor exists: trigger backups, inspect results, and see retention/prune status.

---

## Cross-cutting

Independent of milestones — good candidates for small, standalone PRs:

- **Tests.** Coverage gaps: `inventory` serialization, `slug()`,
  `host_tools_ready`, and router integration tests for the server control-plane
  routes. (`src/client/toolapi.rs` and `src/server/backup.rs` already have the
  data-plane integration tests.)
- **Reproducible build / toolchain lock-down.** Pin the Rust toolchain
  (`rust-toolchain.toml`) and the client image's tool versions.
- **Client Docker image.** `docker/client.Dockerfile` + `docker/deps.sh` need
  `sanoid`/`syncoid`/`zfs`/`btrfs` userland from pinned tarballs and a verify step.

---

## How it chains

`M0` (discovery + inventory) → `M1` (call tools on the client) → `M2` (decide what
to back up and sequence the calls) → `M3`–`M4` (the per-filesystem strategies) →
`M5` (let operators drive it from the UI). The cross-cutting items can land
whenever.
