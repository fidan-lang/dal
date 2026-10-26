# syntax=docker/dockerfile:1
# Digests verified from official multi-platform images on 2026-10-18.
ARG RUST_IMAGE=rust:1.99.0-slim-trixie@sha256:2752b332db73fdbb7dc576f06c82ed1f312005784ef913d7e04a28f5f55dc581
ARG RUNTIME_IMAGE=debian:trixie-slim@sha256:a29215f6a35e51e22adffa17f89e9d2ef06214e64a2bad10d765c46aea49f11f
FROM ${RUST_IMAGE} AS builder
RUN apt-get update && apt-get upgrade -y && apt-get install -y --no-install-recommends pkg-config libssl-dev ca-certificates && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY migrations ./migrations
RUN --mount=type=cache,target=/usr/local/cargo/registry --mount=type=cache,target=/app/target \
    cargo build --release --locked -p dal-server -p dal-worker && \
    install -D -m 0755 target/release/dal-server /out/dal-server && \
    install -D -m 0755 target/release/dal-healthcheck /out/dal-healthcheck && \
    install -D -m 0755 target/release/dal-worker /out/dal-worker
FROM ${RUNTIME_IMAGE} AS runtime
COPY LICENSE NOTICE /usr/share/doc/dal/
COPY --from=builder /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/ca-certificates.crt
# Both Rust executables use Rustls/AWS-LC; ldd confirms no OpenSSL runtime dependency.
RUN apt-get update && apt-get upgrade -y && rm -rf /var/lib/apt/lists/* && \
    groupadd --system --gid 10001 dal && useradd --system --uid 10001 --gid dal --no-create-home dal
USER dal:dal
WORKDIR /tmp
FROM runtime AS server
COPY --from=builder /out/dal-server /usr/local/bin/dal-server
COPY --from=builder /out/dal-healthcheck /usr/local/bin/dal-healthcheck
EXPOSE 8080
HEALTHCHECK --interval=30s --timeout=5s --start-period=30s --retries=3 CMD ["/usr/local/bin/dal-healthcheck"]
ENTRYPOINT ["/usr/local/bin/dal-server"]
FROM runtime AS worker
COPY --from=builder /out/dal-worker /usr/local/bin/dal-worker
ENTRYPOINT ["/usr/local/bin/dal-worker"]
