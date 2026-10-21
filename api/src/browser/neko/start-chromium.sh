#!/bin/sh
# Copyright © 2026 Jalapeno Labs
#
# Starts Chromium on the browser's profile, after clearing the profile's singleton lock.
#
# Chromium locks its profile with the hostname and process id of whoever holds it. When the API
# replaces a browser's container, the profile volume moves to a container with another hostname,
# and Chromium refuses to start ("The profile appears to be in use by another Chromium process on
# another computer"). Exactly one container ever runs on a profile, so a lock found at start is
# always left over from the container before, and is safe to clear.

profile=/home/neko/.config/chromium
rm -f "$profile/SingletonLock" "$profile/SingletonSocket" "$profile/SingletonCookie"
exec /usr/bin/chromium "$@"
