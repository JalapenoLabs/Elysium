#!/usr/bin/env bash
# Copyright © 2026 Jalapeno Labs

# Readies the workspace volume for agents, then waits: turns arrive as `docker exec`, and the
# container's init (Docker's, from the API's `init: true`) reaps what they leave behind.
set -euo pipefail

# A new volume is root's. Agents own the workspace itself and what Elysium keeps in it.
mkdir -p /workspace/.claude /workspace/.elysium/turns
chown agent:agent /workspace /workspace/.claude /workspace/.elysium /workspace/.elysium/turns

# Claude Code reads CLAUDE_CONFIG_DIR, but instructions and tools name ~/.claude, so both lead
# to the same directory on the volume.
ln -sfn /workspace/.claude /home/agent/.claude
chown -h agent:agent /home/agent/.claude

# Every clone under /workspace belongs to agent, whoever's exec created it.
git config --system --add safe.directory '*'

exec sleep infinity
