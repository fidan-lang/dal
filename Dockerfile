# ─────────────────────────────────────────────────────────────────────────────
# Multi-stage Dockerfile for Dal backend
#
# Stages:
#   chef      – install cargo-chef for dependency caching
#   planner   – compute recipe.json
#   builder   – compile workspace (cached deps layer)
#   server    – minimal runtime image for dal-server
#   worker    – minimal runtime image for dal-worker
# ─────────────────────────────────────────────────────────────────────────────

FROM rust:slim-bookworm AS chef
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config libssl-dev ca-certificates \
    && rm -rf /var/lib/apt/lists/*
RUN cargo install cargo-chef --locked
WORKDIR /app

# ── Planner ──────────────────────────────────────────────────────────────────
FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

# ── Builder ───────────────────────────────────────────────────────────────────
FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
# Build and cache only dependencies
RUN cargo chef cook --release --recipe-path recipe.json

# Build the actual binaries
COPY . .
RUN cargo build --release -p dal-server -p dal-worker

# ── Server runtime image ──────────────────────────────────────────────────────
FROM debian:bookworm-slim AS server
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates libssl3 \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/dal-server /usr/local/bin/dal-server
EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/dal-server"]

# ── Worker runtime image ──────────────────────────────────────────────────────
FROM debian:bookworm-slim AS worker
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates libssl3 \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/target/release/dal-worker /usr/local/bin/dal-worker
ENTRYPOINT ["/usr/local/bin/dal-worker"]
