#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# coralReef boot configuration deployer
#
# Installs modprobe.d, udev rules, kernel cmdline, and rebuilds initramfs
# to ensure vfio-pci claims Titan V GPUs before nvidia can touch them.
#
# Usage: sudo ./scripts/boot/deploy-boot.sh

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

info()  { echo -e "${GREEN}[coralReef]${NC} $*"; }
warn()  { echo -e "${YELLOW}[coralReef]${NC} $*"; }
error() { echo -e "${RED}[coralReef]${NC} $*" >&2; }

if [[ $EUID -ne 0 ]]; then
    error "This script must be run as root (sudo)"
    exit 1
fi

info "Deploying coralReef boot configuration..."

# ── modprobe.d ───────────────────────────────────────────────
info "Installing modprobe config → /etc/modprobe.d/coralreef-dual-titanv.conf"
cp "$SCRIPT_DIR/coralreef-dual-titanv.conf" /etc/modprobe.d/coralreef-dual-titanv.conf

# Remove old conflicting configs
if [[ -f /etc/modprobe.d/coralreef-vfio.conf ]]; then
    warn "Removing old /etc/modprobe.d/coralreef-vfio.conf (consolidated into coralreef-dual-titanv.conf)"
    rm /etc/modprobe.d/coralreef-vfio.conf
fi

# ── udev rules ───────────────────────────────────────────────
info "Installing udev rules → /etc/udev/rules.d/99-coralreef-vfio.rules"
cp "$SCRIPT_DIR/99-coralreef-vfio.rules" /etc/udev/rules.d/99-coralreef-vfio.rules

# Also update the initramfs copy
if [[ -d /usr/lib/udev/rules.d ]]; then
    cp "$SCRIPT_DIR/99-coralreef-vfio.rules" /usr/lib/udev/rules.d/99-coralreef-vfio.rules
fi

udevadm control --reload-rules 2>/dev/null || true

# ── Kernel cmdline ───────────────────────────────────────────
CMDLINE_PARAM="vfio-pci.ids=10de:1d81"
CURRENT_CMDLINE=$(cat /proc/cmdline)

if echo "$CURRENT_CMDLINE" | grep -q "$CMDLINE_PARAM"; then
    info "Kernel cmdline already contains $CMDLINE_PARAM"
else
    info "Adding $CMDLINE_PARAM to kernel cmdline"
    if command -v kernelstub &>/dev/null; then
        kernelstub -a "$CMDLINE_PARAM"
        info "kernelstub updated — $CMDLINE_PARAM added"
    else
        warn "kernelstub not found — manually add '$CMDLINE_PARAM' to your bootloader config"
        warn "  GRUB: edit GRUB_CMDLINE_LINUX_DEFAULT in /etc/default/grub"
        warn "  systemd-boot: edit /boot/loader/entries/*.conf"
    fi
fi

# ── Initramfs rebuild ────────────────────────────────────────
info "Rebuilding initramfs to embed updated modprobe.d config..."
if command -v update-initramfs &>/dev/null; then
    update-initramfs -u
elif command -v dracut &>/dev/null; then
    dracut --force
else
    error "Neither update-initramfs nor dracut found — rebuild initramfs manually"
    exit 1
fi

# ── Verification ─────────────────────────────────────────────
info ""
info "╔══════════════════════════════════════════════════════════╗"
info "║  coralReef boot configuration deployed successfully     ║"
info "╠══════════════════════════════════════════════════════════╣"
info "║  modprobe.d:  softdep nvidia pre: vfio-pci             ║"
info "║               options vfio-pci ids=10de:1d81            ║"
info "║  cmdline:     vfio-pci.ids=10de:1d81                    ║"
info "║  initramfs:   rebuilt                                   ║"
info "╠══════════════════════════════════════════════════════════╣"
info "║  IMPORTANT: Cold reboot required (full power off 10s)   ║"
info "║  This resets GV100 hardware state after nvidia damage.  ║"
info "╚══════════════════════════════════════════════════════════╝"
info ""
info "After reboot, verify with:"
info "  sudo dmesg | grep -E 'nvidia.*03:00|nvidia.*4a:00'  # should be EMPTY"
info "  sudo dmesg | grep -E 'vfio-pci|nvidia' | head -10    # vfio first"
