# Copyright © 2026 Jalapeno Labs

# Ory Hydra with Elysium's configuration built in. Compose builds it from this directory
# on every `up`, so a changed file here is a new image and the Hydra containers are
# recreated on it; why it is built rather than mounted is at the top of compose.yml.
FROM oryd/hydra:v26.2.0@sha256:ff67c7fb5f95074fa53374d41151713554960504b340cd3f95b09e65deaea2a9

COPY entrypoint.sh hydra.yml /etc/elysium-hydra/

# Derives the configuration from Elysium's bootstrap variables, then runs `hydra` with the
# service's command. See entrypoint.sh.
ENTRYPOINT ["/bin/sh", "/etc/elysium-hydra/entrypoint.sh"]
