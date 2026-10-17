#!/usr/bin/env bash
#
# start-dev.sh — bring up the local parasanoid dev stack (server + Rust UI).
#
# Dev runs two processes:
#   * the server (the "brain": JSON API + discovery loop) on 127.0.0.1:8080
#   * the Rust server-rendered UI on 127.0.0.1:3100 (dials the server for data)
#
# Usage:
#   ./start-dev.sh start     build (debug) then launch both (default)
#   ./start-dev.sh stop      stop both, remove pid files
#   ./start-dev.sh restart   stop, then start
#   ./start-dev.sh status    show what is running
#
# Both processes run in the background; logs + pid files live under the
# gitignored target/dev/ dir. Override any default with the matching
# PARASANOID_* env var.
#
# Why 127.0.0.1 (not bare :PORT)? In this sandbox a bare :8080 / :3100 fails
# to resolve ("failed to lookup address information"). See memory
# sandbox-networking-loopback-bind.

set -euo pipefail

# Locate the repo root (this script lives in the project root) and ensure Rust
# is on PATH (it isn't by default on this box).
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$ROOT"
export PATH="$HOME/.cargo/bin:$PATH"

# --- config (override each with the matching PARASANOID_* env var) -----------
SERVER_BIND="${PARASANOID_LISTEN:-127.0.0.1:8080}"
SERVER_URL="${PARASANOID_SERVER_URL:-http://127.0.0.1:8080}"
UI_BIND="${PARASANOID_UI_LISTEN:-127.0.0.1:3100}"
DB_PATH="${PARASANOID_DB_PATH:-$ROOT/ui-rust/data.db}"
POLL_INTERVAL="${PARASANOID_POLL_INTERVAL:-10}"

# --- paths -------------------------------------------------------------------
DEV_DIR="$ROOT/target/dev"            # gitignored (target/ is ignored)
SERVER_BIN="$ROOT/target/debug/parasanoid-worker"
UI_BIN="$ROOT/ui-rust/target/debug/parasanoid-ui-rust"

log()  { printf '\033[1;34m[dev]\033[0m %s\n' "$*"; }
info() { printf '%s\n' "$*"; }
err()  { printf '\033[1;31m[dev]\033[0m %s\n' "$*" >&2; }

alive() { pgrep -f "$1" >/dev/null 2>&1; }

build() {
  log "building (debug)..."
  cargo build --bin parasanoid-worker
  ( cd ui-rust && cargo build --bin parasanoid-ui-rust )
}

stop() {
  # Stop each by recorded PID, then sweep any stragglers by binary name. Match
  # the name (not the path) so a process started from another working dir —
  # whose command line uses a relative path — is still caught.
  if [[ -f "$DEV_DIR/server.pid" ]]; then
    kill "$(cat "$DEV_DIR/server.pid")" 2>/dev/null || true
    rm -f "$DEV_DIR/server.pid"
  fi
  pkill -f 'parasanoid-worker' 2>/dev/null || true

  # Same for the UI.
  if [[ -f "$DEV_DIR/ui.pid" ]]; then
    kill "$(cat "$DEV_DIR/ui.pid")" 2>/dev/null || true
    rm -f "$DEV_DIR/ui.pid"
  fi
  pkill -f 'parasanoid-ui-rust' 2>/dev/null || true
  sleep 1
  info "stopped (server + UI)."
}

start() {
  # If a previous instance is still up, stop it first so we don't double-bind.
  if alive "parasanoid-worker" || alive "parasanoid-ui-rust"; then
    log "found a running instance; stopping it first"
    stop
  fi

  build
  mkdir -p "$DEV_DIR"

  # Server (background): JSON API + discovery loop.
  PARASANOID_LISTEN="$SERVER_BIND" \
  PARASANOID_POLL_INTERVAL="$POLL_INTERVAL" \
  PARASANOID_STALE_AFTER="${PARASANOID_STALE_AFTER:-30}" \
  PARASANOID_CLIENTS="${PARASANOID_CLIENTS:-}" \
  nohup "$SERVER_BIN" > "$DEV_DIR/server.log" 2>&1 &
  echo $! > "$DEV_DIR/server.pid"

  # UI (background). CARGO_MANIFEST_DIR is read at RUNTIME to locate the vendored
  # Bootstrap assets under ui-rust/assets, so it must be set when launched from
  # the repo root (the Rust crate is not part of the repo workspace).
  PARASANOID_SERVER_URL="$SERVER_URL" \
  PARASANOID_UI_LISTEN="$UI_BIND" \
  PARASANOID_DB_PATH="$DB_PATH" \
  nohup env "CARGO_MANIFEST_DIR=$ROOT/ui-rust" "$UI_BIN" > "$DEV_DIR/ui.log" 2>&1 &
  echo $! > "$DEV_DIR/ui.pid"

  sleep 2
  info "server  -> http://127.0.0.1:8080   (logs: $DEV_DIR/server.log)"
  info "UI      -> http://127.0.0.1:3100    (logs: $DEV_DIR/ui.log)"
  info "data    -> $DB_PATH"
  if ! alive "parasanoid-ui-rust"; then
    err "UI process exited early — see $DEV_DIR/ui.log"
    return 1
  fi
  info "running. stop with: ./start-dev.sh stop"
}

status() {
  alive "parasanoid-worker" && info "server  : running (http://127.0.0.1:8080)" || info "server  : stopped"
  alive "parasanoid-ui-rust"  && info "UI      : running (http://127.0.0.1:3100)"  || info "UI      : stopped"
}

case "${1:-start}" in
  start)   start   ;;
  stop)    stop    ;;
  restart) stop && start ;;
  status)  status  ;;
  *)
    echo "usage: $0 {start|stop|restart|status}"
    exit 2
    ;;
esac
