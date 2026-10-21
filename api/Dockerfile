# Build context is the repository root so the builder can read .git for /api/version
# and the frontend stage can read frontend/. Only api/, frontend/, .git, and the
# deployment's configuration (compose.yml, nginx/, kratos/, hydra/, updater/) are copied in.
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
COPY api/Cargo.toml api/Cargo.lock api/build.rs api/rust-toolchain.toml ./
COPY api/src ./src
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /workspace/api/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json
COPY api/Cargo.toml api/Cargo.lock api/build.rs api/rust-toolchain.toml ./
# embed_migrations! compiles the SQL into the binary, so migrations ship with the code.
COPY api/migrations ./migrations
COPY api/src ./src
# Last, because every commit changes it and nothing above depends on it.
COPY .git /workspace/.git
# The release this image is published as, such as 1.4.2, which build.rs bakes into the
# binary for /api/version and auto-update. Empty for main and local builds, which are not
# releases. A build argument reaches RUN as an environment variable.
ARG ELYSIUM_RELEASE=""
RUN cargo build --release --locked

# The official Bitwarden CLI, which the API runs to reach the password vault agents sign in
# with (docs/passwords.md). The OSS build, GPL-3.0 and run as a separate program, pinned to a
# release at least two weeks old and checked against the digest GitHub publishes for it.
FROM debian:trixie-slim AS bitwarden-cli
ARG BW_VERSION=2026.9.1
ARG BW_SHA256=b90c0fc86bcf11c0b194aa3f0a89344738af0512cb267c39ad3250c5edd71663
RUN apt-get update \
  && apt-get install -y --no-install-recommends ca-certificates curl unzip \
  && rm -rf /var/lib/apt/lists/*
RUN curl -fsSL -o /tmp/bw.zip \
    "https://github.com/bitwarden/clients/releases/download/cli-v${BW_VERSION}/bw-oss-linux-${BW_VERSION}.zip" \
  && echo "${BW_SHA256}  /tmp/bw.zip" | sha256sum --check --strict \
  && unzip -q /tmp/bw.zip -d /tmp/bw \
  && install -m 0755 /tmp/bw/bw /usr/local/bin/bw \
  && rm -rf /tmp/bw.zip /tmp/bw

FROM debian:trixie-slim AS server
# ca-certificates: outbound TLS. curl: the compose healthcheck.
RUN apt-get update \
  && apt-get install -y --no-install-recommends ca-certificates curl \
  && rm -rf /var/lib/apt/lists/* \
  && useradd --system --no-create-home --shell /usr/sbin/nologin elysium \
  && install -d -o elysium -g elysium -m 0700 /var/lib/elysium
COPY --from=bitwarden-cli /usr/local/bin/bw /usr/local/bin/bw
COPY --from=builder /workspace/api/target/release/elysium-api /usr/local/bin/elysium-api
# What the API keeps between restarts: the Bitwarden CLI's state, so the CLI stays signed in
# as the same device. compose.yml mounts a volume here; a new volume copies this directory's
# owner and mode.
VOLUME /var/lib/elysium
USER elysium
EXPOSE 8080
# Exec form keeps the binary as PID 1 so SIGTERM reaches its graceful shutdown handler.
# `app` inherits it.
CMD ["elysium-api"]

FROM server AS app
COPY --from=frontend /workspace/frontend/dist /usr/share/elysium/web
# Everything a deployment runs beside the image, so one release carries its own
# configuration and the updater installs both together. Read with:
#   docker run --rm --entrypoint tar <image> -C /usr/share/elysium/deployment -cf - .
COPY compose.yml /usr/share/elysium/deployment/compose.yml
COPY nginx /usr/share/elysium/deployment/nginx
COPY kratos /usr/share/elysium/deployment/kratos
COPY hydra /usr/share/elysium/deployment/hydra
COPY updater /usr/share/elysium/deployment/updater
# The API serves this build at every path outside /api, and refuses to start when the
# directory has no index.html.
ENV FRONTEND_DIR=/usr/share/elysium/web
