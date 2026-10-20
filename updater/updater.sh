#!/bin/sh
# shellcheck shell=busybox
# Copyright © 2026 Jalapeno Labs
#
# The deployment's updater: applies the releases the API tells it to, and rolls back the
# ones that do not come up healthy. It runs as the `updater` compose service, in the
# `auto-update` profile, with the Docker socket and the deployment directory mounted, on the
# docker CLI image's BusyBox shell. See docs/infrastructure.md.
#
# It decides nothing. Every minute it asks the API what to do, and the API answers one
# line: `wait`, or `apply <target> from <running>`. An update:
#
#   1. pulls the target's API image, unless Docker has it already (release tags never move),
#      and reads the configuration it carries: compose.yml, nginx/, kratos/, hydra/, updater/;
#   2. pulls every other Elysium image that configuration names, at the target's version;
#   3. backs up Elysium's, Kratos's, and Hydra's databases;
#   4. installs the configuration, and points ELYSIUM_VERSION in .env at the target;
#   5. pulls the remaining images and brings the stack up, waiting for every health check;
#   6. reports `succeeded <target>` once the API answering is the target, then recreates
#      itself last if its own definition changed.
#
# When step 5 or 6 fails, it restores the running release's configuration and version,
# restores the databases from the backup, brings the stack back up, and reports
# `rolled-back <target> <reason>`. A failure before step 4 changes nothing but the report.
#
# Both calls carry the hex SHA-256 of `elysium/updater/key:<ELYSIUM_ENCRYPTION_KEY>`. The
# API derives the same key (`api/src/updates/mod.rs`); both pin one test vector.
#
# Functions that can fail return non-zero explicitly: their callers test them, which turns
# `set -e` off inside them.

set -eu
# A failure anywhere in a pipeline fails it: an image that cannot be read must not unpack as
# an empty configuration.
set -o pipefail

: "${ELYSIUM_ENCRYPTION_KEY:?ELYSIUM_ENCRYPTION_KEY is not set}"

# Where the API answers on the compose network. A test may point the updater at a stand-in.
api_url="${UPDATER_API_URL:-http://api:8080}"
readonly api_image="jalapenolabs/elysium-api"
# The deployment directory, mounted from the host.
readonly deployment="/deployment"
# Where each API image carries the deployment's configuration.
readonly bundle_path="/usr/share/elysium/deployment"
# What an update replaces in the deployment directory. Nothing else there is touched: not
# .env beyond its ELYSIUM_VERSION line, not compose.override.yml, not backups/.
readonly bundle_entries="compose.yml nginx kratos hydra updater"
readonly poll_seconds=60
# Pulling, migrating, and starting every service, on a slow host.
readonly health_timeout_seconds=600
readonly backups_kept=5

key="$(printf '%s' "elysium/updater/key:$ELYSIUM_ENCRYPTION_KEY" | sha256sum | cut -c1-64)"

# Logs go to stderr, so a function's printed result, such as a staging directory, stays clean.
log() {
  printf '%s updater: %s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$*" >&2
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
  log "could not report \"$1\"; the API closes the update when it times out"
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

is_release() {
  printf '%s' "$1" | grep -Eqx '[0-9]+\.[0-9]+\.[0-9]+'
}

# Every service but this one: bringing the stack up must not recreate the updater mid-update.
stack_services() {
  docker compose config --services | grep -vx updater
}

image_present_or_pulled() {
  docker image inspect "$1" > /dev/null 2>&1 || docker pull --quiet "$1" > /dev/null
}

# Unpacks the configuration `version`'s API image carries into a new directory, and prints
# where. Every entry an update installs must be there.
stage_bundle() {
  staging="$(mktemp -d)"
  if ! docker run --rm --entrypoint tar "$api_image:$1" -C "$bundle_path" -cf - . \
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
  printf '%s' "$staging"
}

# Pulls every Elysium image the staged configuration names, such as the workspace image
# agents run in, at `version`. They are not all compose services, so compose would not.
pull_release_images() {
  images="$(grep -o 'jalapenolabs/elysium-[a-z-]*' "$1/compose.yml" | sort -u)" || return 1
  while read -r image; do
    image_present_or_pulled "$image:$2" || return 1
  done <<EOF
$images
EOF
}

# Replaces the deployment's configuration with a staged one, and removes the staging.
install_bundle() {
  for entry in $bundle_entries; do
    rm -rf "${deployment:?}/$entry"
    cp -a "$1/$entry" "$deployment/$entry" || return 1
  done
  rm -rf "$1"
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

# Brings every service but the updater up, pulling only images Docker does not have: every
# image the stack names is a fixed version, so one already present is the one named.
bring_up() {
  services="$(stack_services)" || return 1
  # shellcheck disable=SC2086 # one word per service, by design
  docker compose up --detach --pull missing --remove-orphans \
    --wait --wait-timeout "$health_timeout_seconds" $services
}

# Whether the API that answers now is `version`.
api_is() {
  wget -q -T 20 -O - "http://api:8080/api/version" | grep -q "\"release\":\"$1\""
}

# Puts the running release back. `restore` is the backup to restore the databases from, or
# empty when the stack was never touched.
rollback() {
  target="$1"
  running="$2"
  restore="$3"
  reason="$4"
  log "rolling back to $running: $reason"

  if staging="$(stage_bundle "$running")" && install_bundle "$staging" && set_version "$running" \
    && { [ -z "$restore" ] || restore_databases "$restore"; } \
    && bring_up; then
    report "rolled-back $target $reason"
    return
  fi
  report "rolled-back $target $reason; restoring $running failed too, so the stack needs attention"
}

# Reports a failure that changed nothing: the running release never stopped.
abandon() {
  log "not applying $1: $2"
  report "rolled-back $1 $2"
}

apply() {
  target="$1"
  running="$2"
  backup="$deployment/backups/$(date -u +%Y%m%dT%H%M%SZ)-$running"
  log "applying $target over $running"

  if ! image_present_or_pulled "$api_image:$target"; then
    abandon "$target" "the image for $target could not be pulled"
    return
  fi
  if ! staging="$(stage_bundle "$target")"; then
    abandon "$target" "the configuration $target carries could not be read"
    return
  fi
  if ! pull_release_images "$staging" "$target"; then
    rm -rf "$staging"
    abandon "$target" "an image $target names could not be pulled"
    return
  fi
  if ! backup_databases "$backup"; then
    rm -rf "$staging"
    abandon "$target" "the databases could not be backed up"
    return
  fi

  # From here on the stack changes, so every failure rolls back.
  if ! install_bundle "$staging" || ! set_version "$target"; then
    rollback "$target" "$running" "" "the configuration for $target could not be installed"
    return
  fi
  if ! bring_up; then
    rollback "$target" "$running" "$backup" \
      "the stack did not come up healthy within $health_timeout_seconds seconds"
    return
  fi
  if ! api_is "$target"; then
    rollback "$target" "$running" "$backup" "the API that came up is not $target"
    return
  fi

  log "applied $target"
  report "succeeded $target"
  # Last, because recreating the updater ends this script when its definition changed.
  docker compose up --detach updater
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
