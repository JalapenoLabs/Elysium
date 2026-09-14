#!/usr/bin/env bash
# Proves the migrations and the Diesel schema are trustworthy, against a disposable
# Postgres that matches production's version:
#
#   1. src/database/schema.rs is exactly what the migrations produce (--locked-schema)
#   2. every migration's down.sql undoes its up.sql cleanly (redo --all)
#   3. the database-backed tests pass, including the API's own migration runner
#
# Requires docker and diesel_cli 2.3.13:
#   cargo install diesel_cli --version 2.3.13 --no-default-features --features postgres --locked

set -euo pipefail

readonly POSTGRES_IMAGE="postgres:18.6-alpine3.23"
readonly ROLE="migration_auditor"
readonly PASSWORD="disposable-verification-only"
readonly DATABASE="elysium_verify"

api_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
container="elysium-verify-migrations-$$"

cleanup() {
  docker stop "$container" >/dev/null 2>&1 || true
}
trap cleanup EXIT

echo "==> starting $POSTGRES_IMAGE"
docker run --detach --rm --name "$container" \
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
export DATABASE_URL="postgres://$ROLE:$PASSWORD@127.0.0.1:$port/$DATABASE"
export TEST_DATABASE_URL="$DATABASE_URL"

cd "$api_dir"

echo "==> applying every migration and checking schema.rs for drift"
diesel migration run --locked-schema

# `revert` on its own would rewrite schema.rs to the empty schema; `redo` rolls
# every migration back and re-applies it, then checks the final schema.
echo "==> rolling back and re-applying every migration"
diesel migration redo --all --locked-schema

echo "==> running database-backed tests"
cargo test --locked -- --include-ignored

echo "==> migrations verified"
