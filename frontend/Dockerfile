# syntax=docker/dockerfile:1

# ---------------------------------------------------------------------------
# Development image for the Elysium Vite + React frontend.
#
# Dependencies install into a real node_modules (Yarn Berry, node-modules
# linker) in a cached layer. At runtime Compose bind-mounts the project for
# hot reload and shadows node_modules with an anonymous volume so the
# container keeps its own Linux-built dependencies.
# ---------------------------------------------------------------------------

# Pinned to the exact Node used in CI (never a floating tag).
FROM node:24.13.0-slim AS dev
WORKDIR /app

# git is required to install the Jalapeno Labs packages (`@jalapenolabs/cli`,
# `@jalapenolabs/uikit`), which Yarn fetches straight from their public git repos
# rather than npm; ca-certificates lets git verify the GitHub TLS cert. The slim
# base ships without either.
RUN apt-get update \
  && apt-get install --yes --no-install-recommends git ca-certificates \
  && rm -rf /var/lib/apt/lists/*

RUN corepack enable

# Install dependencies first so this layer is cached until the manifest,
# lockfile, or the pinned Yarn release change.
COPY package.json yarn.lock .yarnrc.yml ./
COPY .yarn/ ./.yarn/
RUN yarn install --immutable

# Bring in the application. Sources are also bind-mounted at runtime, but
# copying them keeps the image runnable on its own. node_modules is never
# copied (see .dockerignore).
COPY index.html vite.config.ts tsconfig.json postcss.config.js tailwind.config.js eslint.config.ts ./
COPY public/ ./public/
COPY src/ ./src/

EXPOSE 5173

# --host publishes the dev server on all interfaces inside the container.
CMD ["yarn", "dev", "--host"]
