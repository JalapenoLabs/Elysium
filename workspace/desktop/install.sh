#!/bin/sh
# Copyright © 2026 Jalapeno Labs
#
# Installs the desktop every agent turn gets into the workspace image: a virtual display, a window
# manager, a browser, a VNC server the live view watches through, and the computer control MCP
# server the agent drives it with. Runs once, as root, while the image builds (workspace/Dockerfile).
#
#   Xvfb, openbox, x11vnc, socat   from Ubuntu's archive
#   /opt/firefox                   Firefox ESR, from Mozilla's release tarball, checksum-verified
#   /opt/elysium/desktop           computer-use-mcp and its dependencies, installed from the lockfile
#   /opt/elysium/bin               elysium-desktop-mcp, the per-turn launcher
#
# Nothing runs here. Each turn starts a desktop of its own (elysium-desktop-mcp), so no two turns
# share a screen. See docs/coding.md, The desktop.

set -eu

# Firefox ESR, pinned to a release at least two weeks old; the sum is Mozilla's published SHA256SUMS
# entry for linux-x86_64/en-US.
FIREFOX_VERSION=153.4.0esr
FIREFOX_SHA256=1832c20dd8c5f69cee933debe62372f5c997845413c8eff846317f6463172032

SOURCE=$(dirname "$0")

# The display stack; xdotool, which computer-use-mcp types with so text follows the keyboard layout;
# and what Firefox and computer-use-mcp's screen capture and input link against.
install_packages() {
    apt-get update
    apt-get install --no-install-recommends --yes \
        xvfb openbox x11vnc socat x11-utils xdotool \
        fonts-dejavu-core fonts-liberation \
        libgtk-3-0t64 libasound2t64 libx11-xcb1 libxtst6 libdbus-glib-1-2 libpci3
    rm -rf /var/lib/apt/lists/*
}

install_firefox() {
    curl -fsSLo /tmp/firefox.tar.xz \
        "https://ftp.mozilla.org/pub/firefox/releases/${FIREFOX_VERSION}/linux-x86_64/en-US/firefox-${FIREFOX_VERSION}.tar.xz"
    echo "${FIREFOX_SHA256}  /tmp/firefox.tar.xz" | sha256sum --check --quiet
    tar -xJf /tmp/firefox.tar.xz -C /opt --no-same-owner
    rm /tmp/firefox.tar.xz
    # Policies keep a turn's browser quiet: no updates (the image is the version), no telemetry, no
    # first-run pages or default-browser prompts competing with what the agent is doing.
    mkdir -p /opt/firefox/distribution
    cp "$SOURCE/firefox-policies.json" /opt/firefox/distribution/policies.json
    ln -sf /opt/firefox/firefox /usr/local/bin/firefox
    firefox --version
}

install_mcp_server() {
    mkdir -p /opt/elysium/desktop
    cp "$SOURCE/package.json" "$SOURCE/package-lock.json" /opt/elysium/desktop/
    # The lockfile pins every version and its integrity hash. No package needs an install script:
    # sharp and nut.js ship prebuilt binaries as optional dependencies.
    npm ci --prefix /opt/elysium/desktop --omit=dev --ignore-scripts --no-audit --no-fund
    npm cache clean --force
}

install_launcher() {
    mkdir -p /opt/elysium/bin
    install -m 0755 "$SOURCE/elysium-desktop-mcp" /opt/elysium/bin/elysium-desktop-mcp
}

install_packages
install_firefox
install_mcp_server
install_launcher
