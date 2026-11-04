# Copyright © 2026 Jalapeno Labs

# Ory Kratos with Elysium's configuration built in. Compose builds it from this directory
# on every `up`, so a changed file here is a new image and the Kratos containers are
# recreated on it; why it is built rather than mounted is at the top of compose.yml.
FROM oryd/kratos:v26.2.0@sha256:2a13bb8d362c7a7ae33bd7c0f5168aee46921f15c916a06346db91c06dc76643

# kratos.yml names the schemas and webhook bodies by their path under /etc/elysium-kratos.
COPY entrypoint.sh kratos.yml *.schema.json *.jsonnet /etc/elysium-kratos/

# Derives the configuration from Elysium's bootstrap variables, then runs `kratos` with the
# service's command. See entrypoint.sh.
ENTRYPOINT ["/bin/sh", "/etc/elysium-kratos/entrypoint.sh"]
