#!/bin/sh
# Copyright © 2026 Jalapeno Labs
#
# Installs Blender and its MCP integration into the workspace image, so every agent turn can model,
# render, and inspect .blend files. Runs once, as root, while the image builds (workspace/Dockerfile).
#
#   /opt/blender         Blender, from the official release tarball.
#   /opt/blender-mcp     Blender Lab's MCP server, in its own venv, dependencies hash-locked.
#   the `mcp` extension  Blender Lab's bridge add-on, enabled in the agent account's profile.
#   /opt/elysium         Studio's exporter, which turns every .blend an agent delivers into a .glb.
#
# Nothing runs here. Each turn starts its own Blender and MCP server (elysium-blender-mcp), so no two
# turns ever share a scene. See docs/coding.md, Blender.

set -eu

BLENDER_VERSION=5.2.2
BLENDER_SHA256=84098912789dc450e95697c4184fb8a90acbe5111c2ba4aede3fecb57806a168
# The MCP server's release archives sit behind a bot check that refuses scripted downloads, so the
# commit is the pin: git verifies the content it names.
BLENDER_MCP_COMMIT=2cea8d566dde07fbac28a61d698909d69724e853
BLENDER_MCP_EXTENSION_VERSION=1.0.3
BLENDER_MCP_EXTENSION_SHA256=a7a9da816192502e5a0a202a396444e266b47d8fc4f74ad4698048bd43040707

AGENT_ACCOUNT=agent
AGENT_HOME=/home/agent
SOURCE=$(dirname "$0")

run_as_agent() {
    setpriv --reuid="$AGENT_ACCOUNT" --regid="$AGENT_ACCOUNT" --init-groups \
        env -i HOME="$AGENT_HOME" PATH=/usr/local/bin:/usr/bin:/bin LANG=C.UTF-8 "$@"
}

# Blender's Linux build links the X11 and GL client libraries even in background mode. Nothing
# draws; these only satisfy the loader.
install_libraries() {
    apt-get update
    apt-get install --no-install-recommends --yes \
        libx11-6 libxi6 libxxf86vm1 libxfixes3 libxrender1 libxkbcommon0 libsm6 libice6 libgl1 libegl1
    rm -rf /var/lib/apt/lists/*
}

install_blender() {
    series=$(echo "$BLENDER_VERSION" | cut -d. -f1,2)
    curl -fsSLo /tmp/blender.tar.xz \
        "https://download.blender.org/release/Blender${series}/blender-${BLENDER_VERSION}-linux-x64.tar.xz"
    echo "${BLENDER_SHA256}  /tmp/blender.tar.xz" | sha256sum --check --quiet
    mkdir -p /opt/blender
    tar -xJf /tmp/blender.tar.xz -C /opt/blender --strip-components=1 --no-same-owner
    rm /tmp/blender.tar.xz
    ln -sf /opt/blender/blender /usr/local/bin/blender
    blender --version
}

install_mcp_server() {
    git init --quiet /tmp/blender-mcp-source
    git -C /tmp/blender-mcp-source fetch --quiet --depth 1 \
        https://projects.blender.org/lab/blender_mcp.git "$BLENDER_MCP_COMMIT"
    git -C /tmp/blender-mcp-source checkout --quiet FETCH_HEAD

    # Dependencies from the hash lock first, then the server with `--no-deps` and no build isolation,
    # so pip installs nothing the lock does not name (the lock carries the build backend too).
    python3 -m venv /opt/blender-mcp
    /opt/blender-mcp/bin/pip install --quiet --no-cache-dir --require-hashes \
        --requirement "$SOURCE/requirements.txt"
    /opt/blender-mcp/bin/pip install --quiet --no-cache-dir --no-deps --no-build-isolation \
        /tmp/blender-mcp-source/mcp
    rm -rf /tmp/blender-mcp-source
    ln -sf /opt/blender-mcp/bin/blender-mcp /usr/local/bin/blender-mcp
}

# The extension lives in the agent account's Blender profile, because that is the account every
# turn's Blender runs as.
install_extension() {
    curl -fsSLo /tmp/blender-mcp-extension.zip \
        "https://projects.blender.org/lab/blender_mcp/releases/download/v${BLENDER_MCP_EXTENSION_VERSION}/mcp-${BLENDER_MCP_EXTENSION_VERSION}.zip"
    echo "${BLENDER_MCP_EXTENSION_SHA256}  /tmp/blender-mcp-extension.zip" | sha256sum --check --quiet
    chmod 0644 /tmp/blender-mcp-extension.zip

    run_as_agent blender --background --factory-startup --online-mode \
        --command extension install-file --repo user_default --enable /tmp/blender-mcp-extension.zip

    # Preferences persist only when saved. The extension declares the network permission and will
    # not listen without online access, so that is saved on.
    run_as_agent blender --background --online-mode --python-expr "$(cat <<'PYTHON'
import sys
import bpy

addon = bpy.context.preferences.addons.get("bl_ext.user_default.mcp")
if addon is None:
    print("[elysium setup] the mcp extension is not enabled")
    sys.exit(1)
bpy.context.preferences.system.use_online_access = True
addon.preferences.use_autostart = True
bpy.ops.wm.save_userpref()
PYTHON
)"

    rm /tmp/blender-mcp-extension.zip
}

# Studio's exporter (docs/studio.md, Model export), and the per-turn MCP launcher.
install_tools() {
    mkdir -p /opt/elysium/bin
    install -m 0644 "$SOURCE/export_glb.py" /opt/elysium/export_glb.py
    install -m 0755 "$SOURCE/export-glb" /opt/elysium/bin/export-glb
    install -m 0755 "$SOURCE/elysium-blender-mcp" /usr/local/bin/elysium-blender-mcp
}

install_libraries
install_blender
install_mcp_server
install_extension
install_tools
echo "blender install complete"
