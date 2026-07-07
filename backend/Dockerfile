# syntax=docker/dockerfile:1

# ---------------------------------------------------------------------------
# Development image for the Elysia Rust API.
#
# Dependencies are compiled in their own cached layer (via cargo-chef) so that
# editing `src/` only recompiles our crate, not the whole dependency tree.
# The container runs `cargo-watch`, and Compose mounts `src/` plus a named
# volume for `target/`, giving fast incremental hot-reload rebuilds.
# ---------------------------------------------------------------------------

FROM rust:1.96-slim-bookworm AS base
WORKDIR /app

# curl -> backend healthcheck; protobuf-compiler -> `protoc` for build.rs codegen;
# the rest -> common native build prerequisites.
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        curl \
        pkg-config \
        libssl-dev \
        ca-certificates \
        protobuf-compiler \
    && rm -rf /var/lib/apt/lists/*

# Dev tooling. Cached in its own layer so it is built only once.
#   watchexec -> file watcher with a polling mode (bind mounts from a Windows
#                host don't deliver inotify events, so polling is required)
#   cargo-chef -> dependency layer caching
RUN cargo install watchexec-cli cargo-chef

# ---- Plan: capture the dependency graph as a recipe ------------------------
FROM base AS planner
COPY Cargo.toml Cargo.lock ./
# build.rs must be present so the recipe reflects the build-script step.
COPY build.rs ./build.rs
COPY src ./src
RUN cargo chef prepare --recipe-path recipe.json

# ---- Build: cook dependencies, then bring in real sources ------------------
FROM base AS dev
COPY --from=planner /app/recipe.json recipe.json
# Compiles only dependencies; this layer is reused until Cargo.lock changes.
RUN cargo chef cook --recipe-path recipe.json
COPY Cargo.toml Cargo.lock ./
# build.rs generates the protobuf types at compile time (needs `protoc` above and
# the `../proto` tree mounted at /proto); without it `OUT_DIR` is never set.
COPY build.rs ./build.rs
COPY src ./src

EXPOSE 31416

# Poll `src/` for changes and restart `cargo run` on edit. Polling is what makes
# hot reload work over a Windows bind mount; the interval trades latency for CPU.
CMD ["watchexec", "--restart", "--poll=1s", "--watch", "src", "--exts", "rs", "--", "cargo", "run"]
