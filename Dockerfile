# syntax=docker/dockerfile:1.7

# -------------- Build stage --------------
FROM rust:1.94-bookworm AS builder
WORKDIR /build

# Cache deps separately from app code: copy manifests first, build dummy.
COPY Cargo.toml Cargo.lock* ./
RUN mkdir -p src src/bin migrations \
    && echo "fn main(){}" > src/main.rs \
    && echo "fn main(){}" > src/bin/smoke_client.rs \
    && touch migrations/.keep \
    && cargo fetch

# Now copy real source and build for real.
COPY migrations ./migrations
COPY src ./src

RUN cargo build --release --bin broker --bin smoke-client

# -------------- Runtime stage --------------
FROM debian:bookworm-slim AS runtime
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl tini \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /build/target/release/broker /usr/local/bin/broker
COPY --from=builder /build/target/release/smoke-client /usr/local/bin/smoke-client
COPY migrations ./migrations

ENV BIND_ADDR=0.0.0.0:8080
EXPOSE 8080

HEALTHCHECK --interval=15s --timeout=3s --start-period=5s --retries=3 \
    CMD curl -fsS http://127.0.0.1:8080/health >/dev/null || exit 1

ENTRYPOINT ["/usr/bin/tini", "--"]
CMD ["/usr/local/bin/broker"]
