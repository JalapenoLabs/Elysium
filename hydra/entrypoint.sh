#!/bin/sh
# Copyright © 2026 Jalapeno Labs
#
# Starts Hydra with a configuration derived from Elysium's own bootstrap variables, as
# kratos/entrypoint.sh does for Kratos. Every argument is passed to `hydra`, after the
# configuration: `serve all` for the server, `migrate sql up -e --yes` to migrate.
#
# Secrets: each is SHA-256 over a purpose label and ELYSIUM_ENCRYPTION_KEY, in hex.
#
# Over plain http, which the API allows on localhost alone, Hydra must run in development
# mode: it refuses an http issuer otherwise. Development mode relaxes only the https
# requirements (the issuer, redirect URIs, and the cookie's Secure flag). See docs/mcp.md.

set -eu

: "${ELYSIUM_ENCRYPTION_KEY:?ELYSIUM_ENCRYPTION_KEY is not set}"
: "${ELYSIUM_PUBLIC_URL:?ELYSIUM_PUBLIC_URL is not set}"

derive_secret() {
  printf '%s' "elysium/hydra/$1:$ELYSIUM_ENCRYPTION_KEY" | sha256sum | cut -c1-64
}

public_url="${ELYSIUM_PUBLIC_URL%/}"

sed \
  -e "s|@PUBLIC_URL@|$public_url|g" \
  -e "s|@SYSTEM_SECRET@|$(derive_secret system)|g" \
  -e "s|@COOKIE_SECRET@|$(derive_secret cookie)|g" \
  /etc/elysium-hydra/hydra.yml > /tmp/hydra.yml

if [ "${public_url%%://*}" = "http" ] && [ "${1:-}" = "serve" ]; then
  exec hydra "$@" --dev --config /tmp/hydra.yml
fi
exec hydra "$@" --config /tmp/hydra.yml
