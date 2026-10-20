# Copyright © 2026 Jalapeno Labs

# The agent workspace: where every agent turn runs, as `agent`, through `docker exec` from
# the API. The API creates the container itself (api/src/runner/workspace.rs) from the
# image of its own release, with /workspace on a volume. See docs/coding.md.
#
# Ubuntu 24.04, because Blender's MCP server is hash-locked for its Python 3.12.
FROM ubuntu:noble-20260917

ENV DEBIAN_FRONTEND=noninteractive

# What agents reach for on their own: git and gh for repositories, a compiler toolchain and
# Python for builds and scripts, and the small tools Claude Code's own tools shell out to.
# setsid (util-linux) and pkill (procps) are how the API stops a turn's process group.
RUN apt-get update \
  && apt-get install -y --no-install-recommends \
    bash build-essential ca-certificates curl git jq openssh-client procps \
    python3 python3-venv ripgrep unzip util-linux xz-utils \
  && rm -rf /var/lib/apt/lists/*

# Node is the web app's (frontend/.nvmrc), from nodejs.org, checked against its published sum.
# The Workspace image check fails when the two drift.
ARG NODE_VERSION=22.23.2
ARG NODE_SHA256=d60acfe00a2932254bb0ad20e01b0d74397a0875595de719654b214f4b03f307
RUN curl -fsSL -o /tmp/node.tar.xz \
    "https://nodejs.org/dist/v${NODE_VERSION}/node-v${NODE_VERSION}-linux-x64.tar.xz" \
  && echo "${NODE_SHA256}  /tmp/node.tar.xz" | sha256sum --check --strict \
  && tar -xJf /tmp/node.tar.xz -C /usr/local --strip-components=1 --no-same-owner \
  && rm /tmp/node.tar.xz \
  && node --version

# Pinned to a release at least two weeks old, and checked against GitHub's published sum.
ARG GH_VERSION=2.102.0
ARG GH_SHA256=bb766f710eef8ede859c18578c72c327597cd4c8a85b06001b1f3843c6019386
RUN curl -fsSL -o /tmp/gh.tar.gz \
    "https://github.com/cli/cli/releases/download/v${GH_VERSION}/gh_${GH_VERSION}_linux_amd64.tar.gz" \
  && echo "${GH_SHA256}  /tmp/gh.tar.gz" | sha256sum --check --strict \
  && tar -xzf /tmp/gh.tar.gz -C /tmp \
  && install -m 0755 "/tmp/gh_${GH_VERSION}_linux_amd64/bin/gh" /usr/local/bin/gh \
  && rm -rf /tmp/gh.tar.gz "/tmp/gh_${GH_VERSION}_linux_amd64"

# Claude Code refuses to bypass permissions as root, so turns run as this account. The base
# image's `ubuntu` user holds uid 1000; agent takes 1001.
RUN useradd --create-home --uid 1001 --shell /bin/bash agent

# Blender, its MCP server and bridge, and Studio's exporter. Before the CLI, because it changes
# far less often and is the slowest layer to rebuild.
COPY blender/ /tmp/blender/
RUN sh /tmp/blender/install.sh && rm -rf /tmp/blender

# The official Claude Code CLI, pinned, with its self-updater off: the image is the version.
ARG CLAUDE_CODE_VERSION=2.1.292
RUN npm install --global "@anthropic-ai/claude-code@${CLAUDE_CODE_VERSION}" \
  && npm cache clean --force

ENV CLAUDE_CONFIG_DIR=/workspace/.claude \
    DISABLE_AUTOUPDATER=1 \
    TZ=UTC

COPY entrypoint.sh /usr/local/bin/elysium-workspace-entrypoint
RUN chmod 0755 /usr/local/bin/elysium-workspace-entrypoint

WORKDIR /workspace
ENTRYPOINT ["/usr/local/bin/elysium-workspace-entrypoint"]
