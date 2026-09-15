# Build context is oauth-broker/ itself: the broker deploys independently of Elysium.

FROM rust:1.98.1-slim-trixie AS chef
RUN cargo install cargo-chef --version 0.1.78 --locked
WORKDIR /workspace

# Compute the dependency recipe from the manifest alone so the expensive cook
# layer below is reused until Cargo.toml or Cargo.lock changes.
FROM chef AS planner
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY src ./src
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /workspace/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY src ./src
RUN cargo build --release --locked

FROM debian:trixie-slim AS runtime
# ca-certificates: TLS to provider token endpoints. curl: the compose healthcheck.
RUN apt-get update \
  && apt-get install -y --no-install-recommends ca-certificates curl \
  && rm -rf /var/lib/apt/lists/* \
  && useradd --system --no-create-home --shell /usr/sbin/nologin broker
COPY --from=builder /workspace/target/release/elysium-oauth-broker /usr/local/bin/elysium-oauth-broker
USER broker
EXPOSE 8080
# Exec form keeps the binary as PID 1 so SIGTERM reaches its graceful shutdown handler.
CMD ["elysium-oauth-broker"]
