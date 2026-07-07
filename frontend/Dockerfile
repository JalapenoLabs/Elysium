# syntax=docker/dockerfile:1

# ---------------------------------------------------------------------------
# Development image for the Elysia Vite + React frontend.
#
# Dependencies install into a real node_modules (Yarn Berry, node-modules
# linker) in a cached layer. At runtime Compose bind-mounts the project for
# hot reload and shadows node_modules with an anonymous volume so the
# container keeps its own Linux-built dependencies.
# ---------------------------------------------------------------------------

FROM node:24-slim AS dev
WORKDIR /app

RUN corepack enable

# Install dependencies first so this layer is cached until the manifest,
# lockfile, or the pinned Yarn release change.
COPY package.json yarn.lock .yarnrc.yml ./
COPY .yarn/ ./.yarn/
RUN yarn install --immutable

# Bring in the application. Sources are also bind-mounted at runtime, but
# copying them keeps the image runnable on its own. node_modules is never
# copied (see .dockerignore).
COPY index.html vite.config.ts tsconfig.json postcss.config.js tailwind.config.js eslint.config.js ./
COPY public/ ./public/
COPY src/ ./src/

EXPOSE 5173

# --host publishes the dev server on all interfaces inside the container.
CMD ["yarn", "dev", "--host"]
