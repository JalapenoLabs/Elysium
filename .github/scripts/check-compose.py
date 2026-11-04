#!/usr/bin/env python3
# Copyright © 2026 Jalapeno Labs

"""Checks the rules that let a plain `docker compose up` recreate exactly what changed (#153).

Compose recreates a container when its definition or its image changes, never when a file
mounted into it does. So, in every stack (production, production from a checkout, and
development):

- no service bind-mounts a file or directory from the checkout, except the few listed in
  ALLOWED_BIND_MOUNTS, which are not configuration;
- each image is built by one service, since Compose labels an image with the service that
  built it and two builders would make a new image on every `up`, and every other service
  that runs it never pulls it;
- every build turns provenance off, since its attestation is stamped with the build's time;
- every configuration image (an image a service builds from a directory beside the compose
  files) is rebuilt on every `up`, with `pull_policy: build`.

See docs/infrastructure.md, Configuration images. Run from the repository root; it needs
only Docker and the Python standard library.
"""

import json
import os
import subprocess
import sys

STACKS = {
    "production": ["compose.yml"],
    "production from a checkout": ["compose.yml", "compose.build.yml"],
    "development": ["compose.yml", "compose.dev.yml"],
}

# Bind mounts from the checkout that are not configuration, by service and target.
ALLOWED_BIND_MOUNTS = {
    # The updater installs releases into the deployment directory itself.
    ("updater", "/deployment"),
    # The Vite dev server's sources: hot reload, which Vite watches for itself.
    ("frontend", "/app/src"),
    ("frontend", "/app/public"),
    ("frontend", "/app/index.html"),
    ("frontend", "/app/vite.config.ts"),
}

# Compose refuses every command while a required variable is unset. Placeholders only:
# nothing is started, and none of them is a secret.
PLACEHOLDER_ENVIRONMENT = {
    "POSTGRES_USER": "ci-placeholder",
    "POSTGRES_PASSWORD": "ci-placeholder",
    "POSTGRES_DB": "ci-placeholder",
    "REDIS_PASSWORD": "ci-placeholder",
    "ELYSIUM_ENCRYPTION_KEY": "ci-placeholder",
}


def render(files: list[str]) -> dict:
    """The stack's model as Compose resolves it, with every profile's services."""
    arguments = ["docker", "compose", "--profile", "*"]
    for file in files:
        arguments += ["--file", file]
    arguments += ["config", "--format", "json"]
    environment = {**PLACEHOLDER_ENVIRONMENT, **os.environ}
    output = subprocess.run(arguments, env=environment, check=True, capture_output=True, text=True)
    return json.loads(output.stdout)


def check_stack(checkout: str, model: dict) -> list[str]:
    problems: list[str] = []
    builders_by_image: dict[str, list[str]] = {}

    for name, service in model["services"].items():
        for volume in service.get("volumes", []):
            source = volume.get("source", "")
            is_from_checkout = source == checkout or source.startswith(checkout + os.sep)
            if volume.get("type") != "bind" or not is_from_checkout:
                continue
            if (name, volume["target"]) in ALLOWED_BIND_MOUNTS:
                continue
            relative = os.path.relpath(source, checkout)
            problems.append(f"{name} bind-mounts ./{relative} at {volume['target']}; build it into its image")

        build = service.get("build")
        if build is None:
            continue
        image = service.get("image", f"{model['name']}-{name}")
        builders_by_image.setdefault(image, []).append(name)

        if str(build.get("provenance", "")).lower() != "false":
            problems.append(f"{name} builds {image} without `provenance: false`")

        # A directory beside the compose files, such as kratos/, rather than the repository
        # root or an application's own directory, holds configuration.
        context = os.path.relpath(build["context"], checkout)
        is_configuration = context in ("nginx", "kratos", "hydra", "updater")
        if is_configuration and service.get("pull_policy") != "build":
            problems.append(f"{name} builds configuration image {image} without `pull_policy: build`")

    for name, service in model["services"].items():
        image = service.get("image")
        builders = builders_by_image.get(image, [])
        if len(builders) > 1 and name == builders[0]:
            problems.append(f"{image} is built by {', '.join(builders)}; one service must build it")
        if builders and "build" not in service and service.get("pull_policy") != "never":
            problems.append(f"{name} runs {image}, which {builders[0]} builds, without `pull_policy: never`")

    return problems


def main() -> int:
    checkout = os.getcwd()
    failed = False
    for stack, files in STACKS.items():
        problems = check_stack(checkout, render(files))
        for problem in problems:
            print(f"::error::{stack}: {problem}")
        if problems:
            failed = True
        else:
            print(f"{stack}: every service's configuration is in its image, and each image has one builder")
    if failed:
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
