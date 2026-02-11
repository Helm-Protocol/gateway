FROM rust:1.75-slim as builder

RUN apt-get update && apt-get install -y \
    pkg-config libssl-dev protobuf-compiler \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /build
COPY Cargo.toml Cargo.lock* ./
COPY src/ src/

RUN cargo build --release

# Runtime
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y \
    ca-certificates tor \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /build/target/release/helm /usr/local/bin/helm
COPY deploy/tor-config/torrc /etc/tor/torrc

EXPOSE 9735
ENTRYPOINT ["helm"]
