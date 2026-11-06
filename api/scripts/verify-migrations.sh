#!/usr/bin/env bash
# Proves the migrations and the Diesel schema are trustworthy, against a disposable
# Postgres that matches production's version:
#
#   1. src/database/schema.rs is exactly what the migrations produce (--locked-schema)
#   2. every migration's down.sql undoes its up.sql cleanly (redo --all)
#   3. the store-backed tests pass, including the API's own migration runner and the router
#      driven end to end, which also needs a disposable Redis
#
# Requires docker and diesel_cli 2.3.13:
#   cargo install diesel_cli --version 2.3.13 --no-default-features --features postgres --locked

set -euo pipefail

# The Postgres compose.yml runs, pgvector's build of it, character for character: the API
# migrations job fails when the two differ.
readonly POSTGRES_IMAGE="pgvector/pgvector:0.8.7-pg18-trixie@sha256:5895949dfcdf308eb0cd31c950c93c14895c77f33fbedfecf1fa511ab1d4bcd2"
# The Redis compose.yml runs.
readonly REDIS_IMAGE="redis:8.10.1-alpine"
readonly ROLE="migration_auditor"
readonly PASSWORD="disposable-verification-only"
readonly DATABASE="elysium_verify"

api_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
container="elysium-verify-migrations-$$"
redis_container="elysium-verify-redis-$$"

cleanup() {
  docker stop "$container" "$redis_container" >/dev/null 2>&1 || true
}
trap cleanup EXIT

echo "==> starting $POSTGRES_IMAGE"
# Every store-backed test creates a database of its own on this one server, and Docker's default
# 64 MB of /dev/shm runs out partway through the suite ("could not resize shared memory segment"),
# failing whichever tests come last.
docker run --detach --rm --name "$container" \
  --shm-size=512m \
  --env POSTGRES_USER="$ROLE" \
  --env POSTGRES_PASSWORD="$PASSWORD" \
  --env POSTGRES_DB="$DATABASE" \
  --publish 127.0.0.1::5432 \
  "$POSTGRES_IMAGE" >/dev/null

# The image's init phase runs a socket-only server first, so a TCP probe inside the
# container only succeeds once the real server is accepting connections.
until docker exec "$container" pg_isready --quiet --host 127.0.0.1 --username "$ROLE"; do
  sleep 1
done

port="$(docker port "$container" 5432/tcp | head -n 1 | cut -d: -f2)"

echo "==> starting $REDIS_IMAGE"
docker run --detach --rm --name "$redis_container" \
  --publish 127.0.0.1::6379 \
  "$REDIS_IMAGE" >/dev/null
until docker exec "$redis_container" redis-cli ping 2>/dev/null | grep -q PONG; do
  sleep 1
done
redis_port="$(docker port "$redis_container" 6379/tcp | head -n 1 | cut -d: -f2)"
export TEST_REDIS_URL="redis://127.0.0.1:$redis_port/0"
export DATABASE_URL="postgres://$ROLE:$PASSWORD@127.0.0.1:$port/$DATABASE"
export TEST_DATABASE_URL="$DATABASE_URL"

cd "$api_dir"

echo "==> applying every migration and checking schema.rs for drift"
diesel migration run --locked-schema

# `revert` on its own would rewrite schema.rs to the empty schema; `redo` rolls
# every migration back and re-applies it, then checks the final schema.
echo "==> rolling back and re-applying every migration"
diesel migration redo --all --locked-schema

echo "==> running store-backed tests"
cargo test --locked -- --include-ignored

echo "==> migrations verified"
