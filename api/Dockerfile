# Build context is the repository root so the builder can read .git for /api/version.
# Only the api/ tree and .git are copied in; nothing from the frontend enters this image.

FROM rust:1.98.1-slim-trixie AS chef
RUN apt-get update \
  && apt-get install -y --no-install-recommends git \
  && rm -rf /var/lib/apt/lists/*
RUN cargo install cargo-chef --version 0.1.78 --locked
WORKDIR /workspace/api

# Compute the dependency recipe from the manifest alone so the expensive cook
# layer below is reused until Cargo.toml or Cargo.lock changes.
FROM chef AS planner
COPY api/.cargo ./.cargo
COPY api/Cargo.toml api/Cargo.lock api/build.rs api/rust-toolchain.toml ./
COPY api/src ./src
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /workspace/api/recipe.json recipe.json
# git-fetch-with-cli, for the arsox-sdk git dependency.
COPY api/.cargo ./.cargo
RUN cargo chef cook --release --recipe-path recipe.json
COPY api/Cargo.toml api/Cargo.lock api/build.rs api/rust-toolchain.toml ./
# embed_migrations! compiles the SQL into the binary, so migrations ship with the code.
COPY api/migrations ./migrations
COPY api/src ./src
# Last, because every commit changes it and nothing above depends on it.
COPY .git /workspace/.git
RUN cargo build --release --locked

FROM debian:trixie-slim AS runtime
# ca-certificates: outbound TLS. curl: the compose healthcheck.
RUN apt-get update \
  && apt-get install -y --no-install-recommends ca-certificates curl \
  && rm -rf /var/lib/apt/lists/* \
  && useradd --system --no-create-home --shell /usr/sbin/nologin elysium
COPY --from=builder /workspace/api/target/release/elysium-api /usr/local/bin/elysium-api
USER elysium
EXPOSE 8080
# Exec form keeps the binary as PID 1 so SIGTERM reaches its graceful shutdown handler.
CMD ["elysium-api"]
