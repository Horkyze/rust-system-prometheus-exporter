FROM rust:1-slim AS builder

WORKDIR /build
COPY Cargo.toml Cargo.lock* ./
COPY src/ src/
COPY tests/ tests/

RUN cargo build --release

FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /build/target/release/sysmetrics-rs /usr/local/bin/sysmetrics-rs

EXPOSE 9101

ENTRYPOINT ["sysmetrics-rs"]
CMD ["--listen", "0.0.0.0:9101"]
