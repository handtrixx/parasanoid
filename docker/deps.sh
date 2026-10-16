#!/usr/bin/env bash
# Shared install steps for the paranoid client image.
#
# Mirrors weboid's approach:
#   * ZFS ships in Debian trixie's `contrib` component (no external repo needed).
#     The CLI is installed; the kernel module is *not* built in-image (it needs
#     DKMS against the host kernel) — the privileged container loads the host's
#     module.
#  "agent_servers": {
  "opencode-server": {
    "type": "custom",
    "command": "sh",
    "args": [
      "-c",
      "if docker inspect opencode >/dev/null 2>&1; then exec docker exec -i opencode python3 /data/docker/code.handtrixxx.com/acp-cwd-fix.py; else exec ssh -T -p 2222 handtrixxx@handtrixxx.com 'docker exec -i opencode python3 /data/docker/code.handtrixxx.com/acp-cwd-fix.py'; fi"
    ]
  }
}   * sanoid + syncoid are not packaged for Debian; install the pinned tarball.
#
# Usage: deps.sh <install|verify>
set -euo pipefail

SANOID_VERSION=2.3.0

stage() { echo "==> $*"; }

install_deps() {
  stage "enabling Debian contrib (ships zfsutils-linux)"
  sed -i 's/^Types: deb$/Types: deb deb-src/; s/^URIs:.*/URIs: http://deb.debian.org\/debian/' /etc/apt/sources.list 2>/dev/null || true
  sed -i 's/#\? *\(.*contrib.*\)/\1/' /etc/apt/sources.list 2>/dev/null || true
  grep -E contrib /etc/apt/sources.list >/dev/null 2>&1 || echo "deb http://deb.debian.org/debian trixie contrib main" >> /etc/apt/sources.list
  stage "apt update"
  apt-get update

  stage "installing base tooling (rsync, btrfs, zfs cli + sanoid runtime deps)"
  DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
      rsync btrfs-progs zfsutils-linux \
      perl libcapture-tiny-perl libconfig-inifiles-perl \
      pv lzop mbuffer ca-certificates curl
}

install_sanoid() {
  stage "installing sanoid/syncoid v${SANOID_VERSION} (single Perl scripts)"
  local url="https://github.com/jimsalterjrs/sanoid/releases/download/v${SANOID_VERSION}/sanoid-v${SANOID_VERSION}.tar.gz"
  cd /tmp
  curl -fL -o sanoid.tar.gz "$url"
  tar xzf sanoid.tar.gz
  install -m 0755 sanoid /usr/local/sbin/sanoid
  install -m 0755 syncoid /usr/local/sbin/syncoid
  install -m 0755 snapshot /usr/local/sbin/snapshot
}

verify() {
  stage "verifying required tools"
  local t
  for t in rsync zfs zpool sanoid syncoid; do
    if ! command -v "$t" >/dev/null 2>&1; then
      echo "ERROR: required tool '$t' missing" >&2
      exit 1
    fi
    echo "ok: $t -> $(command -v $t)"
  done
}

case "${1:-install}" in
  install) install_deps; install_sanoid ;;
  verify)  verify ;;
  *) echo "usage: deps.sh [install|verify]" >&2; exit 1 ;;
esac
