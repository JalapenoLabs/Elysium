# Build context is the repository root so the builder can read .git for /api/version
# and the frontend stage can read frontend/. Only api/, frontend/, and .git are copied in.
#
# Two final stages:
# - `server`, the API alone. The development stack builds this one; there nginx sends
#   every path outside /api to the Vite dev server.
# - `app`, the default and the published image: the server plus the web app's
#   production build, which it serves at every path outside /api.

# The web app's production build. No Rust stage depends on it, so BuildKit builds it
# alongside the API.
FROM node:22.23.2-trixie-slim AS frontend
# git: @jalapenolabs/cli and @jalapenolabs/uikit install from GitHub, and uikit
# builds itself during install. brotli: the assets are precompressed below.
RUN apt-get update \
  && apt-get install -y --no-install-recommends git ca-certificates brotli \
  && rm -rf /var/lib/apt/lists/*
RUN corepack enable && corepack prepare yarn@4.17.0 --activate
WORKDIR /workspace/frontend
COPY frontend/package.json frontend/yarn.lock frontend/.yarnrc.yml ./
RUN yarn install --immutable
COPY frontend/index.html frontend/vite.config.ts frontend/eslint.config.ts ./
COPY frontend/tsconfig.json frontend/tsconfig.app.json frontend/tsconfig.node.json ./
COPY frontend/public ./public
COPY frontend/src ./src
RUN yarn build
# Fingerprinted assets never change under a name, so they are compressed once here at the
# highest settings, and the API sends these files rather than compressing per request.
RUN find dist/assets -type f \( -name '*.js' -o -name '*.css' -o -name '*.svg' -o -name '*.json' \) \
    -exec brotli --best --keep {} + \
    -exec gzip --best --keep {} +

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

FROM debian:trixie-slim AS server
# ca-certificates: outbound TLS. curl: the compose healthcheck.
RUN apt-get update \
  && apt-get install -y --no-install-recommends ca-certificates curl \
  && rm -rf /var/lib/apt/lists/* \
  && useradd --system --no-create-home --shell /usr/sbin/nologin elysium
COPY --from=builder /workspace/api/target/release/elysium-api /usr/local/bin/elysium-api
USER elysium
EXPOSE 8080
# Exec form keeps the binary as PID 1 so SIGTERM reaches its graceful shutdown handler.
# `app` inherits it.
CMD ["elysium-api"]

FROM server AS app
COPY --from=frontend /workspace/frontend/dist /usr/share/elysium/web
# The API serves this build at every path outside /api, and refuses to start when the
# directory has no index.html.
ENV FRONTEND_DIR=/usr/share/elysium/web
