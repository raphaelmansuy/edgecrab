# ─── Stage 1: Build ───────────────────────────────────────────────────────────
FROM rust:1-slim-bookworm AS builder

RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /build

# Cache dependency compilation — copy manifests first
COPY Cargo.toml Cargo.lock ./

# Copy all crate manifests for dependency resolution
COPY crates/edgecrab-types/Cargo.toml     crates/edgecrab-types/
COPY crates/edgecrab-security/Cargo.toml  crates/edgecrab-security/
COPY crates/edgecrab-state/Cargo.toml     crates/edgecrab-state/
COPY crates/edgecrab-cron/Cargo.toml      crates/edgecrab-cron/
COPY crates/edgecrab-tools/Cargo.toml     crates/edgecrab-tools/
COPY crates/edgecrab-core/Cargo.toml      crates/edgecrab-core/
COPY crates/edgecrab-cli/Cargo.toml       crates/edgecrab-cli/
COPY crates/edgecrab-gateway/Cargo.toml   crates/edgecrab-gateway/
COPY crates/edgecrab-acp/Cargo.toml       crates/edgecrab-acp/
COPY crates/edgecrab-migrate/Cargo.toml   crates/edgecrab-migrate/

# Dummy build to cache dependencies
RUN for dir in crates/*/; do \
      mkdir -p "$dir/src"; \
      name=$(basename "$dir"); \
      if [ "$name" = "edgecrab-cli" ]; then \
        echo 'fn main() {}' > "$dir/src/main.rs"; \
      else \
        echo '' > "$dir/src/lib.rs"; \
      fi; \
    done \
    && cargo build --release -p edgecrab-cli 2>/dev/null || true \
    && rm -rf crates/*/src

# Copy real source and rebuild
COPY crates/ crates/
RUN find crates/ -name '*.rs' -exec touch {} + \
    && cargo build --release -p edgecrab-cli

# ─── Stage 2: Runtime ─────────────────────────────────────────────────────────
FROM debian:bookworm-slim AS runtime

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd -r -s /bin/false -d /home/edgecrab -m edgecrab

COPY --from=builder /build/target/release/edgecrab /usr/local/bin/edgecrab

# Create data directory for config/state
RUN mkdir -p /home/edgecrab/.edgecrab && chown -R edgecrab:edgecrab /home/edgecrab

USER edgecrab
WORKDIR /home/edgecrab

# Gateway default port
EXPOSE 8642

ENTRYPOINT ["/usr/local/bin/edgecrab"]
CMD ["gateway", "start", "--foreground"]
