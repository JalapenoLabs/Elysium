# Copyright © 2026 Jalapeno Labs

# nginx with Elysium's configuration built in. Compose builds it from this directory on
# every `up`, so a changed file here is a new image and nginx is recreated on it; why it is
# built rather than mounted is at the top of compose.yml.
#
# Two final stages, differing only in the HTTP routing (see docs/infrastructure.md):
# - `production`, the default: every path goes to the API, which serves the web app.
# - `development`: every path outside /api goes to the Vite dev server.
FROM nginx:1.30.4-alpine AS base

# The image's default plus the `stream` block, which must live in the main context.
COPY nginx.conf /etc/nginx/nginx.conf
# Outside conf.d: both routing files include it inside their server block.
COPY locations.conf /etc/nginx/locations.conf
# Rendered into conf.d by the image's entrypoint from TRUSTED_PROXY_ADDRESSES; see the file.
COPY real-ip.conf.template /etc/nginx/templates/real-ip.conf.template
# The mail ports and the browsers' media ports, as raw TCP and UDP.
COPY mail.conf browser.conf /etc/nginx/stream.d/

FROM base AS development
COPY development.conf /etc/nginx/conf.d/default.conf

FROM base AS production
COPY default.conf /etc/nginx/conf.d/default.conf
