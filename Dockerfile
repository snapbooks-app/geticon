# Base images are pinned to builds at least 14 days old (see CLAUDE.md)
FROM rust:1.98.1-slim-trixie@sha256:4cd829461bd5c4d511c32e269da9cb8929223b666519d8004e35fc8d1d771ab7 AS build

# OpenSSL headers for sentry's native-tls transport
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkgconf \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /usr/src/app

# Build dependencies in their own layer, so code changes don't recompile them
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo "fn main() {}" > src/main.rs && touch src/lib.rs \
    && cargo build --release --locked \
    && rm -rf src

# Build the application with release optimizations
COPY src ./src
RUN touch src/main.rs src/lib.rs && cargo build --release --locked

# Runtime image: just the binary on a slim Debian, without the Rust toolchain
FROM debian:trixie-20260918-slim@sha256:a99cfc517144bc59b1978475ec53b46ecabec7e43635402ee5b77cc54cd1b20a

# OpenSSL and CA certificates for sentry's native-tls transport, and curl for
# the deploy's container health check (curl -f http://localhost:8080/health)
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    curl \
    libssl3t64 \
    && rm -rf /var/lib/apt/lists/*

COPY --from=build /usr/src/app/target/release/geticon /usr/local/bin/geticon

# Expose the port the app runs on
EXPOSE 8080

# Set production environment
ENV RUST_LOG=info
ENV RUST_BACKTRACE=0

# Sentry configuration (DSN must be provided at runtime)
ENV SENTRY_DSN=""
ENV SENTRY_ENVIRONMENT="production"
# Release is automatically set from Cargo.toml version

# Run the binary
CMD ["geticon"]
