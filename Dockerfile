FROM rust:1-slim-bullseye

WORKDIR /usr/src/app
COPY . .

# Build the application with release optimizations
RUN cargo build --release

# Expose the port the app runs on
EXPOSE 8080

# Set production environment
ENV RUST_LOG=info
ENV RUST_BACKTRACE=0

# Run the binary
CMD ["./target/release/geticon"]
