#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-only
#
# Deploy coral-ember, coral-glowplug, and coralctl from the release build.
#
# Usage:
#   ./scripts/deploy.sh          # build + deploy (prompts for auth)
#   ./scripts/deploy.sh --skip-build   # deploy already-built binaries
#
# The script uses pkexec for privilege escalation. To avoid the prompt,
# install the polkit rule:
#   sudo cp scripts/50-coralreef-deploy.pkla /etc/polkit-1/localauthority/50-local.d/
#
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BINARIES=(coral-ember coral-glowplug coralctl)
INSTALL_DIR="/usr/local/bin"

skip_build=false
for arg in "$@"; do
    case "$arg" in
        --skip-build) skip_build=true ;;
        *) echo "Unknown arg: $arg"; exit 1 ;;
    esac
done

if [ "$skip_build" = false ]; then
    echo ":: Building release binaries..."
    cargo build --release -p coral-ember -p coral-glowplug --manifest-path "$REPO_ROOT/Cargo.toml"
fi

for bin in "${BINARIES[@]}"; do
    src="$REPO_ROOT/target/release/$bin"
    if [ ! -f "$src" ]; then
        echo "ERROR: $src not found. Run without --skip-build." >&2
        exit 1
    fi
done

echo ":: Stopping services..."
pkexec sh -c "
    systemctl stop coral-glowplug 2>/dev/null || true
    systemctl stop coral-ember 2>/dev/null || true
    for bin in ${BINARIES[*]}; do
        cp \"$REPO_ROOT/target/release/\$bin\" \"$INSTALL_DIR/\$bin\"
        echo \"  installed \$bin\"
    done
    systemctl start coral-ember
    sleep 2
    systemctl start coral-glowplug
    echo ':: Services restarted'
"

# Also update user-local cargo bin if it exists (avoid stale shadow).
CARGO_BIN="$HOME/.cargo/bin"
if [ -d "$CARGO_BIN" ]; then
    for bin in "${BINARIES[@]}"; do
        if [ -f "$CARGO_BIN/$bin" ]; then
            cp "$REPO_ROOT/target/release/$bin" "$CARGO_BIN/$bin"
        fi
    done
fi

echo ":: Verifying..."
for svc in coral-ember coral-glowplug; do
    state=$(systemctl is-active "$svc" 2>/dev/null || echo "inactive")
    if [ "$state" = "active" ]; then
        echo "  $svc: active"
    else
        echo "  $svc: $state (PROBLEM)" >&2
    fi
done

echo ":: Deploy complete."
