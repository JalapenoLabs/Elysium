# Copyright © 2026 Jalapeno Labs

# The updater: the docker CLI image, which carries compose and buildx, with updater.sh built
# in. Compose builds it from this directory on every `up`, so an update that changes the
# script recreates the updater on it at its last step; why it is built rather than mounted
# is at the top of compose.yml.
FROM docker:29.9.1-cli@sha256:6fae2214c3121a28d5aa639d09032f61310b7c9c75ffbad1025f261a1b621800

COPY updater.sh /usr/local/lib/elysium/updater.sh

ENTRYPOINT ["/bin/sh", "/usr/local/lib/elysium/updater.sh"]
