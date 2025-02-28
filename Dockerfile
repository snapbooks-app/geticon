FROM rust:1-slim-bullseye

# Install dependencies required for building
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /usr/src/app
COPY . .

# Build the application with release optimizations
RUN cargo build --release

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
CMD ["./target/release/geticon"]
