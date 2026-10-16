# syntax=docker/dockerfile:1
#
# parasanoid server: the storage-box brain. One small static binary + runtime
# deps only (certs). This binary keeps everything in memory — there is no
# database in the worker, so no sqlite is needed at build or run time.

FROM rust:1-bookworm-slim AS build
WORKDIR /app

# Toolchain metadata + manifests first, for better layer caching.
COPY rust-toolchain.toml Cargo.toml Cargo.lock ./
COPY src ./src

# --locked mirrors the committed Cargo.lock.
RUN cargo build --release --locked

# --- runtime stage ---------------------------------------------------------
FROM debian:trixie-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=build /app/target/release/parasanoid-worker /usr/local/bin/parasanoid-worker

EXPOSE 8080
CMD ["parasanoid-worker", "server"]
