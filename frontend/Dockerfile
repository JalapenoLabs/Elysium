# Vite dev server. nginx fronts it in compose; source directories are bind-mounted
# there so edits hot-reload without a rebuild. node_modules stays inside the image.

FROM node:22.23.2-trixie-slim
# git: @jalapenolabs/cli and @jalapenolabs/uikit install from GitHub, and uikit
# builds itself during install.
RUN apt-get update \
  && apt-get install -y --no-install-recommends git ca-certificates \
  && rm -rf /var/lib/apt/lists/*
RUN corepack enable && corepack prepare yarn@4.17.0 --activate
WORKDIR /app

COPY package.json yarn.lock .yarnrc.yml ./
RUN yarn install --immutable

COPY index.html vite.config.ts tsconfig.json tsconfig.app.json tsconfig.node.json eslint.config.ts ./
COPY public ./public
COPY src ./src

EXPOSE 5173
CMD ["yarn", "dev"]
