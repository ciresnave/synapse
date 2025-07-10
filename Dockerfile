FROM rust:1.70-slim-bullseye as builder

# Install build dependencies
RUN apt-get update && \
    apt-get install -y --no-install-recommends \
    pkg-config \
    libssl-dev \
    libpq-dev \
    protobuf-compiler \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /usr/src/synapse

# Copy Cargo files first for better layer caching
COPY Cargo.toml Cargo.lock ./

# Create a dummy main.rs to build dependencies
RUN mkdir -p src && \
    echo "fn main() { println!(\"Dummy build\"); }" > src/main.rs && \
    cargo build --release && \
    rm -f target/release/deps/synapse*

# Copy the actual source code
COPY . .

# Build the actual binary
RUN cargo build --release

# Final stage - creates a smaller image
FROM debian:bullseye-slim

RUN apt-get update && \
    apt-get install -y --no-install-recommends \
    ca-certificates \
    libpq5 \
    libssl1.1 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy the binary from the builder stage
COPY --from=builder /usr/src/synapse/target/release/synapse /app/
COPY --from=builder /usr/src/synapse/config /app/config

# Create a non-root user for security
RUN useradd -m synapse && \
    chown -R synapse:synapse /app

USER synapse

# Set environment variables
ENV RUST_LOG="info"
ENV DATABASE_URL=""
ENV PORT="8080"
ENV CONFIG_PATH="/app/config/production.toml"

# Expose ports
EXPOSE 8080
EXPOSE 8081/udp
EXPOSE 8082

# Run the binary
CMD ["./synapse"]
