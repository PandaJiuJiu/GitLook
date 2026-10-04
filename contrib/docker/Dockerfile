# Build stage
FROM rust:1.75-slim AS builder

WORKDIR /build

# Install build dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    git \
    && rm -rf /var/lib/apt/lists/*

# Cache dependencies
COPY Cargo.toml Cargo.lock ./
COPY minisite-server/Cargo.toml ./minisite-server/
COPY minisite-cli/Cargo.toml ./minisite-cli/

RUN mkdir -p minisite-server/src minisite-cli/src \
    && echo "fn main() {}" > minisite-server/src/main.rs \
    && echo "" > minisite-server/src/lib.rs \
    && echo "fn main() {}" > minisite-cli/src/main.rs \
    && cargo build --release \
    && rm -rf minisite-server/src minisite-cli/src

# Build the actual binaries
COPY minisite-server ./minisite-server
COPY minisite-cli ./minisite-cli
RUN cargo build --release

# Runtime stage
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y \
    git \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd -r -u 1000 minisite \
    && mkdir -p /var/lib/minisite/repos /var/lib/minisite/worktrees /var/log/minisite \
    && chown -R minisite:minisite /var/lib/minisite /var/log/minisite

COPY --from=builder /build/target/release/minisite-server /usr/local/bin/
COPY --from=builder /build/target/release/minisite-cli /usr/local/bin/

USER minisite
WORKDIR /var/lib/minisite

EXPOSE 9999

ENTRYPOINT ["/usr/local/bin/minisite-server"]
CMD ["--config", "/etc/minisite/config.toml"]
