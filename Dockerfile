# Prepare with Chef
FROM rust:1.93-slim AS planner
WORKDIR /app
RUN cargo install cargo-chef
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

# Build
FROM rust:1.93-slim AS builder
WORKDIR /app
RUN cargo install cargo-chef
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json

COPY . .
RUN cargo build --release

# Runtime
FROM debian:trixie-slim AS runtime
WORKDIR /app

# Install dependencies
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates && \
    rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/discord-vr-status /usr/local/bin
ENV AUTH_FILE_PATH=/data/auth.json
ENTRYPOINT [ "/usr/local/bin/discord-vr-status" ]