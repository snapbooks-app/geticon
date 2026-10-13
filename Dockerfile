FROM rust:1-slim-trixie AS build

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
FROM debian:trixie-slim

# OpenSSL and CA certificates for sentry's native-tls transport
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
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
