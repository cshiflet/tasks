#!/usr/bin/env bash
# Bare-metal version of Dockerfile.dev for running the desktop-native
# toolchain directly on an Ubuntu 24.04 (noble) host — or a WSL2 /
# Multipass / cloud VM with that distro.
#
# Run:
#   bash desktop-native/docker/setup-ubuntu-24.04.sh
#
# Non-idempotent in the sense that it re-runs `apt-get install` and
# `rustup` every time; each is safe to re-invoke.
set -euo pipefail

if [[ "$(. /etc/os-release && echo "$ID $VERSION_ID")" != "ubuntu 24.04" ]]; then
    echo "This script targets Ubuntu 24.04 (noble). See Dockerfile.dev for other hosts." >&2
fi

sudo apt-get update
sudo apt-get install -y --no-install-recommends \
    build-essential \
    ca-certificates \
    clang \
    cmake \
    curl \
    git \
    libqt6svg6-dev \
    libsqlite3-dev \
    libssl-dev \
    pkg-config \
    qml6-module-qt-labs-platform \
    qml6-module-qtquick \
    qml6-module-qtquick-controls \
    qml6-module-qtquick-dialogs \
    qml6-module-qtquick-layouts \
    qml6-module-qtquick-window \
    qt6-base-dev \
    qt6-declarative-dev \
    qt6-tools-dev \
    qt6-tools-dev-tools

if ! command -v rustup >/dev/null; then
    # `--profile default` already installs clippy + rustfmt; do NOT pass
    # `--component rustfmt` separately — rustup-init rejects it as an
    # unexpected positional argument.
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
        | sh -s -- -y --default-toolchain stable --profile default
    # shellcheck disable=SC1090
    source "$HOME/.cargo/env"
fi

rustup show
qmake6 --version
cmake --version | head -1

echo
echo "Toolchain ready. From the repo root, try:"
echo "  cd desktop-native"
echo "  cargo test --workspace"
echo "  cargo run -p tasks-ui -- --cli path/to/tasks.db      # CLI mode"
echo "  cargo run -p tasks-ui                                 # GUI"
