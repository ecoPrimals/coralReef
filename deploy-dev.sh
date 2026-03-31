#!/usr/bin/env bash
# deploy-dev.sh — Start coral-ember + coral-glowplug for development testing.
#
# Run after reboot. No sudo needed for daemon startup — daemons bind
# devices via vfio-pci (group fds are 666). Binding unbound devices
# requires pkexec (one-shot).
#
# Usage:
#   ./deploy-dev.sh          # start daemons
#   ./deploy-dev.sh status   # check daemon health
#   ./deploy-dev.sh stop     # stop daemons
#   ./deploy-dev.sh bind     # bind GPUs to vfio-pci (pkexec)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
CONFIG="$SCRIPT_DIR/crates/coral-glowplug/glowplug.toml"
RUNTIME_DIR="${XDG_RUNTIME_DIR:-/run/user/$(id -u)}"
BIOMEOS_DIR="$RUNTIME_DIR/biomeos"
EMBER_SOCK="$BIOMEOS_DIR/coral-ember-default.sock"
GLOWPLUG_SOCK="$BIOMEOS_DIR/coral-glowplug-default.sock"
EMBER_LOG="$BIOMEOS_DIR/coral-ember.log"
GLOWPLUG_LOG="$BIOMEOS_DIR/coral-glowplug.log"

TITAN_BDF="0000:03:00.0"
K80_DIE0="0000:4c:00.0"
K80_DIE1="0000:4d:00.0"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
NC='\033[0m'

ok()   { echo -e "  ${GREEN}✓${NC} $1"; }
warn() { echo -e "  ${YELLOW}!${NC} $1"; }
fail() { echo -e "  ${RED}✗${NC} $1"; }

check_gpu() {
    local bdf="$1" name="$2"
    local driver
    driver=$(readlink "/sys/bus/pci/devices/$bdf/driver" 2>/dev/null | xargs basename 2>/dev/null || echo "none")
    if [ "$driver" = "vfio-pci" ]; then
        ok "$name ($bdf): vfio-pci"
        return 0
    elif [ "$driver" = "none" ]; then
        warn "$name ($bdf): unbound (needs bind)"
        return 1
    else
        warn "$name ($bdf): $driver (needs unbind+rebind)"
        return 1
    fi
}

bind_to_vfio() {
    local bdf="$1" name="$2"
    local driver
    driver=$(readlink "/sys/bus/pci/devices/$bdf/driver" 2>/dev/null | xargs basename 2>/dev/null || echo "none")

    if [ "$driver" = "vfio-pci" ]; then
        ok "$name already on vfio-pci"
        return 0
    fi

    echo "  Binding $name ($bdf) to vfio-pci..."

    if [ "$driver" != "none" ]; then
        echo "$bdf" | pkexec tee "/sys/bus/pci/drivers/$driver/unbind" > /dev/null 2>&1 || true
        sleep 0.5
    fi

    echo "vfio-pci" | pkexec tee "/sys/bus/pci/devices/$bdf/driver_override" > /dev/null
    echo "$bdf" | pkexec tee /sys/bus/pci/drivers/vfio-pci/bind > /dev/null 2>&1 || \
        echo "$bdf" | pkexec tee /sys/bus/pci/drivers_probe > /dev/null

    sleep 1
    local new_driver
    new_driver=$(readlink "/sys/bus/pci/devices/$bdf/driver" 2>/dev/null | xargs basename 2>/dev/null || echo "none")
    if [ "$new_driver" = "vfio-pci" ]; then
        ok "$name bound to vfio-pci"
    else
        fail "$name binding failed (driver=$new_driver)"
        return 1
    fi
}

do_bind() {
    echo "=== Binding GPUs to vfio-pci ==="
    bind_to_vfio "$TITAN_BDF" "Titan V"
    bind_to_vfio "$K80_DIE0"  "K80 die 0"
    bind_to_vfio "$K80_DIE1"  "K80 die 1"
}

do_status() {
    echo "=== GPU Status ==="
    check_gpu "$TITAN_BDF" "Titan V" || true
    check_gpu "$K80_DIE0"  "K80 die 0" || true
    check_gpu "$K80_DIE1"  "K80 die 1" || true

    echo ""
    echo "=== Daemon Status ==="
    if [ -S "$EMBER_SOCK" ]; then
        ok "ember socket: $EMBER_SOCK"
        # Quick health check
        if echo '{"jsonrpc":"2.0","method":"ember.status","params":{},"id":1}' | \
            timeout 2 socat - UNIX-CONNECT:"$EMBER_SOCK" 2>/dev/null | head -1 | grep -q '"result"'; then
            ok "ember: responding"
        else
            warn "ember: socket exists but not responding"
        fi
    else
        fail "ember socket: not found"
    fi

    if [ -S "$GLOWPLUG_SOCK" ]; then
        ok "glowplug socket: $GLOWPLUG_SOCK"
        if echo '{"jsonrpc":"2.0","method":"daemon.status","params":{},"id":1}' | \
            timeout 2 socat - UNIX-CONNECT:"$GLOWPLUG_SOCK" 2>/dev/null | head -1 | grep -q '"result"'; then
            ok "glowplug: responding"
        else
            warn "glowplug: socket exists but not responding"
        fi
    else
        fail "glowplug socket: not found"
    fi
}

do_stop() {
    echo "=== Stopping daemons ==="
    if pkill -f "coral-glowplug.*--config" 2>/dev/null; then
        ok "glowplug stopped"
    else
        warn "glowplug was not running"
    fi
    sleep 1
    if pkill -f "coral-ember.*server" 2>/dev/null; then
        ok "ember stopped"
    else
        warn "ember was not running"
    fi
}

do_start() {
    echo "=== Deploying coral daemons (dev mode) ==="
    echo ""

    # Ensure socket directory exists
    mkdir -p "$BIOMEOS_DIR"

    # Check GPU bindings
    echo "--- GPU bindings ---"
    local need_bind=0
    check_gpu "$TITAN_BDF" "Titan V" || need_bind=1
    check_gpu "$K80_DIE0"  "K80 die 0" || need_bind=1
    check_gpu "$K80_DIE1"  "K80 die 1" || need_bind=1

    if [ "$need_bind" -eq 1 ]; then
        echo ""
        warn "Some GPUs need binding. Run: $0 bind"
        echo "  (Continuing with what's available...)"
    fi
    echo ""

    # Check if already running
    if [ -S "$EMBER_SOCK" ] && pgrep -f "coral-ember.*server" > /dev/null 2>&1; then
        ok "ember already running"
    else
        echo "--- Starting ember ---"
        rm -f "$EMBER_SOCK"
        nohup coral-ember server "$CONFIG" > "$EMBER_LOG" 2>&1 &
        local ember_pid=$!
        echo "  PID=$ember_pid  log=$EMBER_LOG"

        # Wait for socket
        for i in $(seq 1 20); do
            if [ -S "$EMBER_SOCK" ]; then
                ok "ember socket ready (${i}00ms)"
                break
            fi
            sleep 0.1
        done
        if [ ! -S "$EMBER_SOCK" ]; then
            fail "ember socket not ready after 2s — check $EMBER_LOG"
            return 1
        fi
    fi

    sleep 0.5

    if [ -S "$GLOWPLUG_SOCK" ] && pgrep -f "coral-glowplug.*--config" > /dev/null 2>&1; then
        ok "glowplug already running"
    else
        echo "--- Starting glowplug ---"
        rm -f "$GLOWPLUG_SOCK"
        nohup coral-glowplug --config "$CONFIG" > "$GLOWPLUG_LOG" 2>&1 &
        local gp_pid=$!
        echo "  PID=$gp_pid  log=$GLOWPLUG_LOG"

        for i in $(seq 1 30); do
            if [ -S "$GLOWPLUG_SOCK" ]; then
                ok "glowplug socket ready (${i}00ms)"
                break
            fi
            sleep 0.1
        done
        if [ ! -S "$GLOWPLUG_SOCK" ]; then
            fail "glowplug socket not ready after 3s — check $GLOWPLUG_LOG"
            return 1
        fi
    fi

    echo ""
    echo "=== Ready ==="
    echo "  ember:    $EMBER_SOCK"
    echo "  glowplug: $GLOWPLUG_SOCK"
    echo ""
    echo "  Validate: CORALREEF_VFIO_BDF=$TITAN_BDF cargo test --test hw_nv_vfio -p coral-driver --features vfio -- --ignored --nocapture 2>&1 | head -50"
    echo "  K80:      CORALREEF_VFIO_BDF=$K80_DIE1 CORALREEF_K80_BDF=$K80_DIE1 cargo test --test hw_nv_vfio -p coral-driver --features vfio -- exp123k --ignored --nocapture"
}

case "${1:-start}" in
    start)  do_start ;;
    stop)   do_stop ;;
    status) do_status ;;
    bind)   do_bind ;;
    restart) do_stop; sleep 2; do_start ;;
    *)
        echo "Usage: $0 {start|stop|status|bind|restart}"
        exit 1
        ;;
esac
