#!/bin/sh
# shellcheck shell=busybox
# Copyright © 2026 Jalapeno Labs
#
# Runs on the docker CLI image's BusyBox shell, which has `pipefail`.
#
# The deployment's updater: applies the releases the API tells it to, and rolls back the
# ones that do not come up healthy. It runs as the `updater` compose service, in the
# `auto-update` profile, with the Docker socket and the deployment directory mounted. See
# docs/infrastructure.md.
#
# It decides nothing. Every minute it asks the API what to do, and the API answers one
# line: `wait`, or `apply <target> from <running>`. An update:
#
#   1. pulls the target's API image, unless Docker already has it (release tags never move);
#   2. backs up Elysium's, Kratos's, and Hydra's databases;
#   3. installs the configuration the target's image carries (compose.yml, nginx/, kratos/,
#      hydra/, updater/), and points ELYSIUM_VERSION in .env at the target;
#   4. pulls the other images and brings the stack up, waiting for every health check;
#   5. reports `succeeded <target>`, then recreates itself last if its own definition changed.
#
# When step 4 fails, or the API that comes up is not the target, it restores the running
# release's configuration and version, restores the databases from the backup when the
# stack had been touched, brings the stack back up, and reports
# `rolled-back <target> <reason>`.
#
# Both calls carry the hex SHA-256 of `elysium/updater/key:<ELYSIUM_ENCRYPTION_KEY>`. The
# API derives the same key (`api/src/updates/mod.rs`); both pin one test vector.

set -eu
# A failure anywhere in a pipeline fails it: an image that cannot be read must not extract
# as an empty configuration.
set -o pipefail

: "${ELYSIUM_ENCRYPTION_KEY:?ELYSIUM_ENCRYPTION_KEY is not set}"

# Where the API answers on the compose network. A test may point the updater at a stand-in.
api_url="${UPDATER_API_URL:-http://api:8080}"
readonly image_repository="jalapenolabs/elysium-api"
# The deployment directory, mounted from the host.
readonly deployment="/deployment"
# Where each image carries the deployment's configuration.
readonly bundle_path="/usr/share/elysium/deployment"
# What an update replaces in the deployment directory. Nothing else there is touched: not
# .env beyond its ELYSIUM_VERSION line, not compose.override.yml, not backups/.
readonly bundle_entries="compose.yml nginx kratos hydra updater"
readonly poll_seconds=60
# Pulling, migrating, and starting every service, on a slow host.
readonly health_timeout_seconds=600
readonly backups_kept=5

key="$(printf '%s' "elysium/updater/key:$ELYSIUM_ENCRYPTION_KEY" | sha256sum | cut -c1-64)"

log() {
  printf '%s updater: %s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$*"
}

ask() {
  wget -q -T 20 -O - --header "x-elysium-updater-key: $key" "$api_url/internal/updater/instruction"
}

# Reports an outcome, retrying while the API restarts. A report that never lands is logged;
# the API closes the update itself after 45 minutes.
report() {
  attempt=0
  while [ "$attempt" -lt 60 ]; do
    if wget -q -T 20 -O /dev/null \
      --header "x-elysium-updater-key: $key" \
      --header "Content-Type: text/plain" \
      --post-data "$1" \
      "$api_url/internal/updater/outcome"; then
      return 0
    fi
    attempt=$((attempt + 1))
    sleep 10
  done
  log "could not report \"$1\"; the API will close the update when it times out"
}

# Compose resolves the stack's bind mounts against the deployment directory and hands the
# paths to the Docker daemon, which reads them on the host. So compose must run at the
# directory's host path: a link from that path to the mount makes both views agree. Docker
# names a container's host after its id, so the updater finds its own mounts by it.
enter_deployment() {
  host_path="$(docker inspect --format \
    '{{range .Mounts}}{{if eq .Destination "/deployment"}}{{.Source}}{{end}}{{end}}' "$(hostname)")"
  if [ -z "$host_path" ]; then
    log "cannot find where the deployment directory is mounted from; is this the updater service?"
    exit 1
  fi
  if [ "$host_path" != "$deployment" ]; then
    mkdir -p "$(dirname "$host_path")"
    ln -sfn "$deployment" "$host_path"
  fi
  cd "$host_path"
}

# Every service but this one: bringing the stack up must not recreate the updater mid-update.
stack_services() {
  docker compose config --services | grep -vx updater
}

image_present_or_pulled() {
  docker image inspect "$1" > /dev/null 2>&1 || docker pull --quiet "$1" > /dev/null
}

# Replaces the deployment's configuration with what `version`'s image carries. Functions
# that can fail return non-zero explicitly: the callers test them, which turns `set -e` off
# inside them.
install_bundle() {
  staging="$(mktemp -d)"
  if ! docker run --rm --entrypoint tar "$image_repository:$1" -C "$bundle_path" -cf - . \
    | tar -xf - -C "$staging"; then
    rm -rf "$staging"
    log "could not read the configuration the image for $1 carries"
    return 1
  fi
  for entry in $bundle_entries; do
    if [ ! -e "$staging/$entry" ]; then
      rm -rf "$staging"
      log "the image for $1 carries no $entry"
      return 1
    fi
  done
  for entry in $bundle_entries; do
    rm -rf "${deployment:?}/$entry"
    cp -a "$staging/$entry" "$deployment/$entry" || return 1
  done
  rm -rf "$staging"
}

# Points ELYSIUM_VERSION in .env at `version`. The file is rewritten in place, never
# replaced, so it keeps its owner and its mode: it holds the bootstrap secrets.
set_version() {
  rewritten="$(mktemp)"
  grep -v '^ELYSIUM_VERSION=' "$deployment/.env" > "$rewritten" || true
  printf 'ELYSIUM_VERSION=%s\n' "$1" >> "$rewritten"
  cat "$rewritten" > "$deployment/.env" || return 1
  rm -f "$rewritten"
}

# The databases this stack keeps: Elysium's own, and Kratos's and Hydra's beside it.
databases() {
  printf '%s kratos hydra' "$(docker compose exec -T postgres printenv POSTGRES_DB)"
}

backup_databases() {
  directory="$1"
  names="$(databases)" || return 1
  mkdir -p "$directory" || return 1
  for database in $names; do
    docker compose exec -T postgres sh -c 'pg_dump --username "$POSTGRES_USER" --format custom "$1"' \
      dump "$database" > "$directory/$database.dump" || return 1
  done
  # The newest few backups are kept; an update's backup is only for undoing that update.
  find "$deployment/backups" -mindepth 1 -maxdepth 1 -type d | sort | head -n "-$backups_kept" \
    | xargs -r rm -rf
}

# Recreates each database from the backup, so tables a failed release's migrations added are
# gone too. Everything that holds a connection is stopped first.
restore_databases() {
  directory="$1"
  names="$(databases)" || return 1
  docker compose stop nginx api kratos hydra > /dev/null || return 1
  for database in $names; do
    docker compose exec -T postgres sh -c \
      'psql --username "$POSTGRES_USER" --dbname postgres --quiet \
        --command "DROP DATABASE IF EXISTS \"$1\" WITH (FORCE)" \
        --command "CREATE DATABASE \"$1\"" \
        && pg_restore --username "$POSTGRES_USER" --dbname "$1" --no-owner' \
      restore "$database" < "$directory/$database.dump" || return 1
  done
}

bring_up() {
  # shellcheck disable=SC2046 # one word per service, by design
  docker compose pull --quiet $(stack_services) \
    && docker compose up --detach --wait --wait-timeout "$health_timeout_seconds" --remove-orphans \
      $(stack_services)
}

# Whether the API that answers now is `version`.
api_is() {
  wget -q -T 20 -O - "http://api:8080/api/version" | grep -q "\"release\":\"$1\""
}

rollback() {
  target="$1"
  running="$2"
  backup="$3"
  touched="$4"
  reason="$5"
  log "rolling back to $running: $reason"

  if install_bundle "$running" && set_version "$running" \
    && { [ "$touched" = "no" ] || restore_databases "$backup"; } \
    && bring_up; then
    report "rolled-back $target $reason"
    return
  fi
  report "rolled-back $target $reason; restoring $running failed too, so the stack needs attention"
}

apply() {
  target="$1"
  running="$2"
  backup="$deployment/backups/$(date -u +%Y%m%dT%H%M%SZ)-$running"
  log "applying $target over $running"

  if ! image_present_or_pulled "$image_repository:$target"; then
    rollback "$target" "$running" "$backup" no "the image for $target could not be pulled"
    return
  fi
  if ! backup_databases "$backup"; then
    rollback "$target" "$running" "$backup" no "the databases could not be backed up"
    return
  fi
  if ! install_bundle "$target" || ! set_version "$target"; then
    rollback "$target" "$running" "$backup" no "the configuration for $target could not be installed"
    return
  fi
  if ! bring_up; then
    rollback "$target" "$running" "$backup" yes \
      "the stack did not come up healthy within $health_timeout_seconds seconds"
    return
  fi
  if ! api_is "$target"; then
    rollback "$target" "$running" "$backup" yes "the API that came up is not $target"
    return
  fi

  log "applied $target"
  report "succeeded $target"
  # Last, because recreating the updater ends this script when its definition changed.
  docker compose up --detach updater
}

is_release() {
  printf '%s' "$1" | grep -Eqx '[0-9]+\.[0-9]+\.[0-9]+'
}

enter_deployment
log "watching for releases from $api_url"

while true; do
  if instruction="$(ask)"; then
    # shellcheck disable=SC2086 # the instruction is words by design
    set -- $instruction
    if [ "${1:-}" = "apply" ] && [ "${3:-}" = "from" ] && is_release "${2:-}" && is_release "${4:-}"; then
      apply "$2" "$4"
    elif [ "${1:-}" != "wait" ]; then
      log "ignored an instruction this updater does not understand: $instruction"
    fi
  else
    log "the API did not answer; asking again in $poll_seconds seconds"
  fi
  sleep "$poll_seconds"
done
