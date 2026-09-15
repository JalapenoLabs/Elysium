#!/bin/sh
# Copyright © 2026 Jalapeno Labs
#
# Starts Stalwart, and restarts it once when its setup completes.
#
# A new Stalwart has no configuration file, so it starts in bootstrap mode. The Email
# settings page finishes the setup through the API, and Stalwart writes its configuration
# file, but keeps running in bootstrap mode until it is restarted. Nothing in Elysium can
# restart a container, so this waits for that file and stops Stalwart; the service's
# `restart: unless-stopped` policy starts it again, now with its configuration.
#
# Once the file exists, every later start skips the watcher.

set -eu

readonly config=/etc/stalwart/config.json

if [ ! -f "$config" ]; then
  (
    while [ ! -f "$config" ]; do
      sleep 1
    done
    # Stalwart runs as PID 1 after the exec below, and shuts down cleanly on TERM.
    kill -TERM 1
  ) &
fi

exec /usr/local/bin/stalwart --config "$config"
