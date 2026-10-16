# syntax=docker/dockerfile:1
#
# parasanoid client: a host-scanning API bridge. Built in two stages:
#   1. compile the worker binary (from the shared Rust sources),
#   2. wrap it in an image that also carries the backup tooling it inspects and
#      (in future) drives — rsync, the ZFS userland, and sanoid/syncoid.

# --- build the worker binary ------------------------------------------------
FROM rust:1-bookworm-slim AS build
WORKDIR /app
COPY rust-toolchain.toml Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --release --locked

# --- runtime image with backup tooling -------------------------------------
FROM debian:trixie-slim

# rsync / zfs userland / sanoid (see docker/deps.sh; ZFS ships in Debian
# trixie's contrib, sanoid is installed from a pinned release tarball).
COPY docker/deps.sh /usr/local/bin/deps.sh
RUN chmod +x /usr/local/bin/deps.sh \
    && /usr/local/bin/deps.sh install

# Bring in the compiled worker.
COPY --from=build /app/target/release/parasanoid-worker /usr/local/bin/parasanoid-worker

EXPOSE 9100
CMD ["parasanoid-worker", "client"]
