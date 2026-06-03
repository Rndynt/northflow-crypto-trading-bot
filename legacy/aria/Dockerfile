FROM rust:1.86-slim AS builder

RUN apt-get update && apt-get install -y pkg-config libssl-dev libfontconfig1-dev && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src ./src

RUN cargo build --release

FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y ca-certificates libssl3 libfontconfig1 && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/aria /usr/local/bin/aria

WORKDIR /app
COPY config ./config

# Default to aggressive config — override via ARIA_CONFIG_OVERLAY env var in Coolify
ENV ARIA_CONFIG_OVERLAY=config/aggressive.toml

EXPOSE 3000
EXPOSE 8080
EXPOSE 8081

CMD ["aria"]
