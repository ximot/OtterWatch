# OtterWatch Agent Container
# For load testing with Podman/Docker
# Build: podman build -t otterwatch-agent -f Containerfile .
#
# Minimum Rust version: 1.83.0 (see rust-version in Cargo.toml)

# ============================================
# Build stage
# ============================================
FROM rust:1.83-slim-bookworm AS builder

WORKDIR /usr/src/otterwatch

# Install build dependencies
RUN apt-get update && apt-get install -y \
    protobuf-compiler \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Copy all source files
COPY Cargo.toml Cargo.lock build.rs ./
COPY proto/ proto/
COPY src/ src/

# Build release binary
RUN cargo build --release

# ============================================
# Runtime stage - minimal image
# ============================================
FROM debian:bookworm-slim

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    procps \
    curl \
    && rm -rf /var/lib/apt/lists/* \
    && mkdir -p /var/lib/otterwatch \
    && chmod 755 /var/lib/otterwatch

# Copy binary from builder
COPY --from=builder /usr/src/otterwatch/target/release/otterwatch /usr/bin/otterwatch

# Copy minimal settings file (values overridden by ENV vars)
COPY container-settings.toml /var/lib/otterwatch/settings.toml

# Set working directory
WORKDIR /var/lib/otterwatch

# ============================================
# Environment variables (override at runtime)
# ============================================

# Collection intervals
ENV APP_INTERVAL_SECS=1
ENV APP_PROCESS_LIST_INTERVAL_SECS=10
ENV APP_PROCESS_TOP_N=10

# Local storage (disabled for containers)
ENV APP_DB_SAVE=false
ENV APP_DB_FILE_NAME=/var/lib/otterwatch

# Local HTTP API
ENV APP_LISTEN_ADDR=0.0.0.0:8080

# MQTT configuration
ENV APP_MQTT_ENABLED=true
ENV APP_MQTT_BROKER_ADDR=tcp://mqtt:1883
ENV APP_MQTT_API_KEY=load-test-key
ENV APP_MQTT_TOPIC_PREFIX=otterwatch/metrics
ENV APP_MQTT_KEEPALIVE_SECS=30
ENV APP_MQTT_RETRY_INTERVAL_SECS=5
ENV APP_MQTT_QUEUE_PATH=/var/lib/otterwatch/mqtt_queue
ENV APP_MQTT_QUEUE_MAX_SIZE_MB=10

# Agent grouping
ENV APP_AGENT_GROUP=load-test

# Network interfaces to exclude
ENV APP_EXCLUDE_INTERFACES=lo

# Logging
ENV RUST_LOG=warn

# ============================================
# Health check and startup
# ============================================

HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
    CMD curl -f http://localhost:8080/system-stats || exit 1

# Expose local API port (optional)
EXPOSE 8080

# Run as root to access all /proc data
# For limited visibility, create a non-root user
CMD ["/usr/bin/otterwatch"]
