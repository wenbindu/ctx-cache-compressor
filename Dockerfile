FROM rust:1-bookworm AS builder

WORKDIR /app

COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY static ./static

RUN cargo build --release --locked \
    --bin ctx-cache-compressor-api \
    --bin ctx-cache-compressor-demo \
    --bin ctx-cache-compressor

FROM debian:bookworm-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates tzdata \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --create-home --uid 10001 --shell /usr/sbin/nologin ctx

WORKDIR /app

COPY --from=builder /app/target/release/ctx-cache-compressor-api /usr/local/bin/ctx-cache-compressor-api
COPY --from=builder /app/target/release/ctx-cache-compressor-demo /usr/local/bin/ctx-cache-compressor-demo
COPY --from=builder /app/target/release/ctx-cache-compressor /usr/local/bin/ctx-cache-compressor
COPY config.example.toml /app/config.example.toml

ENV RUST_LOG=info
ENV CTX_CACHE_COMPRESSOR__SERVER__PERMISSIVE_CORS=false
EXPOSE 8080

USER ctx

CMD ["ctx-cache-compressor-api"]
