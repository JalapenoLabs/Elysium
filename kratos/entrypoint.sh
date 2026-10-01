#!/bin/sh
# Copyright © 2026 Jalapeno Labs
#
# Starts Kratos with a configuration derived from Elysium's own bootstrap variables, so
# `.env` holds nothing for Kratos. Every argument is passed to `kratos`, after the
# configuration: `serve --watch-courier` for the server, `migrate sql --yes` to migrate.
#
# Secrets: each is SHA-256 over a purpose label and ELYSIUM_ENCRYPTION_KEY, in hex. The
# labels keep them independent of each other and of the key itself. The API derives the
# hook key the same way (`api/src/auth/hook_key.rs`); both sides pin the same test
# vector, so the two cannot drift.
#
# URLs: ELYSIUM_PUBLIC_URL is the origin browsers use, such as https://elysium.example.com
# or http://localhost:4000. The API refuses to start on anything else, so it is trusted
# as a bare origin here.

set -eu

: "${ELYSIUM_ENCRYPTION_KEY:?ELYSIUM_ENCRYPTION_KEY is not set}"
: "${ELYSIUM_PUBLIC_URL:?ELYSIUM_PUBLIC_URL is not set}"

derive_secret() {
  printf '%s' "elysium/kratos/$1:$ELYSIUM_ENCRYPTION_KEY" | sha256sum | cut -c1-64
}

public_url="${ELYSIUM_PUBLIC_URL%/}"
scheme="${public_url%%://*}"
authority="${public_url#*://}"
host="${authority%%:*}"

# `Secure` cookies are only promised over https. Not every browser keeps them from
# http://localhost, so plain http turns the flag off.
cookie_secure=true
if [ "$scheme" = "http" ]; then
  cookie_secure=false
fi

# Kratos's cipher key must be exactly 32 characters: 128 bits of the hex digest.
cipher_secret="$(derive_secret cipher | cut -c1-32)"

sed \
  -e "s|@PUBLIC_URL@|$public_url|g" \
  -e "s|@PUBLIC_ORIGIN@|$scheme://$authority|g" \
  -e "s|@PUBLIC_HOST@|$host|g" \
  -e "s|@COOKIE_SECURE@|$cookie_secure|g" \
  -e "s|@COOKIE_SECRET@|$(derive_secret cookie)|g" \
  -e "s|@CIPHER_SECRET@|$cipher_secret|g" \
  -e "s|@DEFAULT_SECRET@|$(derive_secret default)|g" \
  -e "s|@HOOK_KEY@|$(derive_secret hook)|g" \
  /etc/elysium-kratos/kratos.yml > /tmp/kratos.yml

exec kratos "$@" --config /tmp/kratos.yml
