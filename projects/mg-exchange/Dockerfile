# ============================================
# Multi-stage Rust build for exchange server
# ============================================

# Stage 1: Build
FROM rust:1.87-bookworm AS builder

WORKDIR /app

# Cache dependencies — copy manifests first
COPY Cargo.toml Cargo.lock ./
# Copy ALL crate manifests for dependency caching
COPY crates/common/Cargo.toml crates/common/Cargo.toml
COPY crates/matching-engine/Cargo.toml crates/matching-engine/Cargo.toml
COPY crates/journal/Cargo.toml crates/journal/Cargo.toml
COPY crates/order-gateway/Cargo.toml crates/order-gateway/Cargo.toml
COPY crates/account/Cargo.toml crates/account/Cargo.toml
COPY crates/risk-engine/Cargo.toml crates/risk-engine/Cargo.toml
COPY crates/market-data/Cargo.toml crates/market-data/Cargo.toml
COPY crates/api-gateway/Cargo.toml crates/api-gateway/Cargo.toml
COPY crates/settlement/Cargo.toml crates/settlement/Cargo.toml
COPY crates/surveillance/Cargo.toml crates/surveillance/Cargo.toml
COPY crates/admin/Cargo.toml crates/admin/Cargo.toml
COPY crates/compliance/Cargo.toml crates/compliance/Cargo.toml
COPY crates/custody/Cargo.toml crates/custody/Cargo.toml
COPY crates/db/Cargo.toml crates/db/Cargo.toml
COPY crates/server/Cargo.toml crates/server/Cargo.toml
COPY crates/trigger-service/Cargo.toml crates/trigger-service/Cargo.toml
COPY crates/index-price/Cargo.toml crates/index-price/Cargo.toml
COPY crates/proof-of-reserves/Cargo.toml crates/proof-of-reserves/Cargo.toml
COPY crates/fix-gateway/Cargo.toml crates/fix-gateway/Cargo.toml
COPY crates/copy-trading/Cargo.toml crates/copy-trading/Cargo.toml
COPY crates/trading-bots/Cargo.toml crates/trading-bots/Cargo.toml
COPY crates/algo-execution/Cargo.toml crates/algo-execution/Cargo.toml
COPY crates/market-maker/Cargo.toml crates/market-maker/Cargo.toml
COPY crates/asset-registry/Cargo.toml crates/asset-registry/Cargo.toml
COPY crates/defi-yield/Cargo.toml crates/defi-yield/Cargo.toml
COPY crates/dex-aggregator/Cargo.toml crates/dex-aggregator/Cargo.toml
COPY crates/web3-wallet/Cargo.toml crates/web3-wallet/Cargo.toml
COPY load-tests/Cargo.toml load-tests/Cargo.toml

# Create dummy src files for dependency caching
RUN for dir in common matching-engine journal order-gateway account risk-engine \
    market-data api-gateway settlement surveillance admin compliance custody db server \
    trigger-service index-price proof-of-reserves fix-gateway copy-trading trading-bots \
    algo-execution market-maker asset-registry defi-yield dex-aggregator web3-wallet; do \
    mkdir -p crates/$dir/src && echo "" > crates/$dir/src/lib.rs; \
    done && \
    mkdir -p crates/server/src && echo "fn main() {}" > crates/server/src/main.rs && \
    mkdir -p load-tests/src && echo "fn main() {}" > load-tests/src/main.rs

# Copy migrations for sqlx
COPY migrations/ migrations/

# Build dependencies only (cached layer)
RUN cargo build --release -p exchange-server 2>/dev/null || true

# Copy actual source code
COPY crates/ crates/

# Touch all source files to invalidate cache
RUN find crates -name "*.rs" -exec touch {} +

# Build the real binary
RUN cargo build --release -p exchange-server

# Stage 2: Runtime
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy binary
COPY --from=builder /app/target/release/exchange /app/exchange

# Copy migrations (for runtime migration)
COPY migrations/ /app/migrations/

# Create data directory
RUN mkdir -p /app/data

# Non-root user
RUN groupadd -r exchange && useradd -r -g exchange exchange
RUN chown -R exchange:exchange /app
USER exchange

ENV RUST_LOG=info,exchange=debug
ENV EXCHANGE_DATA_DIR=/app/data

EXPOSE 8080

HEALTHCHECK --interval=10s --timeout=3s --start-period=5s --retries=3 \
    CMD wget -qO- http://localhost:8080/health || exit 1

ENTRYPOINT ["/app/exchange"]
