#!/usr/bin/env bash
# Proves the migrations and the Diesel schema are trustworthy, against a disposable
# Postgres that matches production's version:
#
#   1. src/database/schema.rs is exactly what the migrations produce (--locked-schema)
#   2. every migration's down.sql undoes its up.sql cleanly (redo --all)
#   3. the store-backed tests pass, including the API's own migration runner and the router
#      driven end to end, which also needs a disposable Redis. Those are the tests `cargo test`
#      ignores, so only they run here: plain `cargo test` covers the rest.
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

# Only the library's tests are built (see the last step), so an ignored test anywhere else would
# run neither here nor in plain `cargo test`. Every other place cargo finds tests is searched for
# one, and the run stops before it starts anything.
outside_library=()
for path in src/main.rs src/bin tests benches examples; do
  if [ -e "$api_dir/$path" ]; then
    outside_library+=("$api_dir/$path")
  fi
done
if grep --recursive --files-with-matches '#\[ignore' "${outside_library[@]}"; then
  echo "==> the files above hold ignored tests outside the library, which nothing would run" >&2
  exit 1
fi

# Removes each container this run started, with its volumes. They are started without `--rm`, so
# this is their one removal and nothing races it. The Postgres image declares a volume at its
# data directory, and a container removed without `--volumes` leaves it behind.
cleanup() {
  for name in "$container" "$redis_container"; do
    if docker container inspect "$name" >/dev/null 2>&1; then
      docker rm --force --volumes "$name" >/dev/null
      echo "==> removed $name"
    fi
  done
}
trap cleanup EXIT

echo "==> starting $POSTGRES_IMAGE"
# Every store-backed test creates a database of its own on this one server, and Docker's default
# 64 MB of /dev/shm runs out partway through the suite ("could not resize shared memory segment"),
# failing whichever tests come last.
#
# The data directory stays on disk rather than a tmpfs: no test drops its database, so the suite
# ends holding about 12 GB, more than a tmpfs on a 16 GB machine can take. Postgres never waits
# for the disk instead. Every database here is thrown away with the container, so durability buys
# nothing, and on a cloud disk waiting for each commit to land took the suite from 262 to 464
# seconds.
docker run --detach --name "$container" \
  --shm-size=512m \
  --env POSTGRES_USER="$ROLE" \
  --env POSTGRES_PASSWORD="$PASSWORD" \
  --env POSTGRES_DB="$DATABASE" \
  --publish 127.0.0.1::5432 \
  "$POSTGRES_IMAGE" \
  -c fsync=off -c synchronous_commit=off -c full_page_writes=off >/dev/null

# The image's init phase runs a socket-only server first, so a TCP probe inside the
# container only succeeds once the real server is accepting connections.
until docker exec "$container" pg_isready --quiet --host 127.0.0.1 --username "$ROLE"; do
  sleep 1
done

port="$(docker port "$container" 5432/tcp | head -n 1 | cut -d: -f2)"

echo "==> starting $REDIS_IMAGE"
docker run --detach --name "$redis_container" \
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
# Every store-backed test is a unit test of the library, so only the library's tests are built:
# one compile of the crate, about 13 GB at its peak, where building every target compiles the
# library twice at once and needs about 21 GB, more than a 16 GB machine has. The binaries hold
# no tests and the doc examples are never compiled, so nothing is skipped, and the check at the
# top refuses an ignored test anywhere else.
cargo test --locked --lib -- --ignored

echo "==> migrations verified"
