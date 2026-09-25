#!/bin/sh
# Copyright © 2026 Jalapeno Labs
#
# Installs Blender and its MCP integration on an Arsox satellite, so every coding thread can model,
# render, and inspect .blend files. Elysium hands this to each satellite as its setup script
# (`PUT /v1/setup`); the satellite runs it as root at every container start and whenever it changes.
#
# It must be idempotent: a satellite whose /opt survived (a persisted volume, a restart rather than
# a replacement) skips every download it already has. Each piece records the version it installed,
# and a matching record is the whole check.
#
#   /opt/blender         Blender, from the official release tarball.
#   /opt/blender-mcp     Blender Lab's MCP server, in its own venv, dependencies hash-locked.
#   the `mcp` extension  Blender Lab's bridge add-on, enabled in the agent account's profile.
#
# Nothing runs here. Each thread starts its own bridge and server as Arsox thread services, so no two
# threads ever share a Blender session. See docs/coding.md.
#
# Rust appends the hash-locked requirements and the call to `main` (api/src/blender/mod.rs), so this
# file ends inside the heredoc that writes them.

set -eu

BLENDER_VERSION=5.2.2
BLENDER_SHA256=84098912789dc450e95697c4184fb8a90acbe5111c2ba4aede3fecb57806a168
# The MCP server's release archives sit behind a bot check that refuses scripted downloads, so the
# commit is the pin: git verifies the content it names.
BLENDER_MCP_COMMIT=2cea8d566dde07fbac28a61d698909d69724e853
BLENDER_MCP_EXTENSION_VERSION=1.0.3
BLENDER_MCP_EXTENSION_SHA256=a7a9da816192502e5a0a202a396444e266b47d8fc4f74ad4698048bd43040707

AGENT_ACCOUNT=arsox
AGENT_HOME=/home/arsox

run_as_agent() {
    setpriv --reuid="$AGENT_ACCOUNT" --regid="$AGENT_ACCOUNT" --init-groups \
        env -i HOME="$AGENT_HOME" PATH=/usr/local/bin:/usr/bin:/bin LANG=C.UTF-8 "$@"
}

# Blender's Linux build links the X11 and GL client libraries even in background mode. Nothing here
# draws; these only satisfy the loader. Pinned exactly, like the satellite image's own packages.
install_libraries() {
    packages="libx11-6=2:1.8.7-1build1 libxi6=2:1.8.1-1build1 libxxf86vm1=1:1.1.4-1build4
        libxfixes3=1:6.0.0-2build1 libxrender1=1:0.9.10-1.1build1 libxkbcommon0=1.6.0-1build1
        libsm6=2:1.2.3-1build3 libice6=2:1.0.10-1build3 libgl1=1.7.0-1build1 libegl1=1.7.0-1build1"

    missing=""
    for package in $packages; do
        name=${package%%=*}
        version=${package#*=}
        if [ "$(dpkg-query --show --showformat='${Version}' "$name" 2>/dev/null)" != "$version" ]; then
            missing="$missing $package"
        fi
    done
    if [ -z "$missing" ]; then
        echo "libraries: already installed"
        return
    fi

    apt-get update
    # Word splitting is the point: each entry is one `name=version` argument.
    # shellcheck disable=SC2086
    apt-get install --no-install-recommends --yes $missing
    rm -rf /var/lib/apt/lists/*
}

install_blender() {
    if [ "$(cat /opt/blender/.installed-version 2>/dev/null)" = "$BLENDER_VERSION" ]; then
        echo "blender: $BLENDER_VERSION already installed"
        return
    fi

    series=$(echo "$BLENDER_VERSION" | cut -d. -f1,2)
    curl -fsSLo /tmp/blender.tar.xz \
        "https://download.blender.org/release/Blender${series}/blender-${BLENDER_VERSION}-linux-x64.tar.xz"
    echo "${BLENDER_SHA256}  /tmp/blender.tar.xz" | sha256sum --check --quiet
    rm -rf /opt/blender
    mkdir -p /opt/blender
    tar -xJf /tmp/blender.tar.xz -C /opt/blender --strip-components=1 --no-same-owner
    rm /tmp/blender.tar.xz
    ln -sf /opt/blender/blender /usr/local/bin/blender
    echo "$BLENDER_VERSION" > /opt/blender/.installed-version
    blender --version
}

install_mcp_server() {
    if [ "$(cat /opt/blender-mcp/.installed-commit 2>/dev/null)" = "$BLENDER_MCP_COMMIT" ]; then
        echo "blender-mcp: $BLENDER_MCP_COMMIT already installed"
        return
    fi

    rm -rf /opt/blender-mcp /tmp/blender-mcp-source
    git init --quiet /tmp/blender-mcp-source
    git -C /tmp/blender-mcp-source fetch --quiet --depth 1 \
        https://projects.blender.org/lab/blender_mcp.git "$BLENDER_MCP_COMMIT"
    git -C /tmp/blender-mcp-source checkout --quiet FETCH_HEAD

    # Dependencies from the hash lock first, then the server with `--no-deps` and no build isolation,
    # so pip installs nothing the lock does not name (the lock carries the build backend too).
    write_requirements /tmp/blender-mcp-requirements.txt
    python3 -m venv /opt/blender-mcp
    /opt/blender-mcp/bin/pip install --quiet --no-cache-dir --require-hashes \
        --requirement /tmp/blender-mcp-requirements.txt
    /opt/blender-mcp/bin/pip install --quiet --no-cache-dir --no-deps --no-build-isolation \
        /tmp/blender-mcp-source/mcp
    rm -rf /tmp/blender-mcp-source /tmp/blender-mcp-requirements.txt
    ln -sf /opt/blender-mcp/bin/blender-mcp /usr/local/bin/blender-mcp
    echo "$BLENDER_MCP_COMMIT" > /opt/blender-mcp/.installed-commit
}

# The extension lives in the agent account's Blender profile, because that is the account every
# thread's bridge runs as. The home directory is not on a volume, so this usually runs every start.
install_extension() {
    record="$AGENT_HOME/.config/blender/.elysium-mcp-extension"
    if [ "$(cat "$record" 2>/dev/null)" = "$BLENDER_MCP_EXTENSION_VERSION" ]; then
        echo "mcp extension: $BLENDER_MCP_EXTENSION_VERSION already installed"
        return
    fi

    curl -fsSLo /tmp/blender-mcp-extension.zip \
        "https://projects.blender.org/lab/blender_mcp/releases/download/v${BLENDER_MCP_EXTENSION_VERSION}/mcp-${BLENDER_MCP_EXTENSION_VERSION}.zip"
    echo "${BLENDER_MCP_EXTENSION_SHA256}  /tmp/blender-mcp-extension.zip" | sha256sum --check --quiet
    chmod 0644 /tmp/blender-mcp-extension.zip

    run_as_agent blender --background --factory-startup --online-mode \
        --command extension install-file --repo user_default --enable /tmp/blender-mcp-extension.zip

    # Preferences persist only when saved. The extension declares the network permission and will
    # not listen without online access, so that is saved on; auto start covers an interactive session.
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
    echo "$BLENDER_MCP_EXTENSION_VERSION" | run_as_agent tee "$record" > /dev/null
}

main() {
    install_libraries
    install_blender
    install_mcp_server
    install_extension
    echo "blender setup complete"
}

# The MCP server's dependencies, pinned with hashes. The lock is api/src/blender/requirements.txt,
# regenerated from the server's pyproject.toml at BLENDER_MCP_COMMIT as that file's header says.
write_requirements() {
    cat > "$1" <<'REQUIREMENTS'
