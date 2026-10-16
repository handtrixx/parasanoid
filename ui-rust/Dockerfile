# syntax=docker/dockerfile:1
#
# parasanoid Rust UI (ui-rust): server-rendered axum + askama + SQLite cache.
# Two-stage build (compile once, ship the binary) mirroring docker/server.Dockerfile.
# SQLite is runtime-only, so no database is needed at build time.
#
# Base tags: `rust:slim` (not `rust:1-bookworm-slim`, which the server
# Dockerfile references but which is not pullable from this mirror).
FROM rust:slim AS build
WORKDIR /app

# Toolchain metadata + manifests first, for better layer caching.
# askama compiles templates in at build time, so templates/ must be present.
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY templates ./templates

# --locked mirrors the committed Cargo.lock.
RUN cargo build --release --locked

# --- runtime stage ---------------------------------------------------------
FROM debian:trixie-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=build /app/target/release/parasanoid-ui-rust /usr/local/bin/parasanoid-ui-rust

# Bootstrap assets are served at runtime from ./assets. CARGO_MANIFEST_DIR is a
# build-only env var, so the binary falls back to cwd -> it must run with ./assets
# present next to it.
COPY assets ./assets

WORKDIR /app
EXPOSE 3100
CMD ["parasanoid-ui-rust"]
