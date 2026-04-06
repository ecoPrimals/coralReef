#!/bin/bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Rebind a GPU to vfio-pci from its current driver.
#
# Usage: sudo bash scripts/rebind-gpu-vfio.sh [BDF]
#   BDF defaults to $CORALREEF_VFIO_BDF, then 0000:03:00.0
set -euo pipefail

BDF="${1:-${CORALREEF_VFIO_BDF:-0000:03:00.0}}"
AUD="${BDF%.*}.1"

echo "[1/4] Setting driver_override → vfio-pci for $BDF..."
echo vfio-pci > "/sys/bus/pci/devices/$BDF/driver_override"
echo vfio-pci > "/sys/bus/pci/devices/$AUD/driver_override" 2>/dev/null || true

echo "[2/4] Unbinding GPU from current driver..."
CURRENT=$(readlink "/sys/bus/pci/devices/$BDF/driver" 2>/dev/null | xargs basename 2>/dev/null || echo "none")
if [ "$CURRENT" != "none" ] && [ "$CURRENT" != "vfio-pci" ]; then
    echo "$BDF" > "/sys/bus/pci/drivers/$CURRENT/unbind" 2>/dev/null || echo "  (unbind failed, continuing)"
fi

echo "[3/4] Probing for vfio-pci..."
echo "$BDF" > /sys/bus/pci/drivers_probe 2>/dev/null || \
  echo "$BDF" > /sys/bus/pci/drivers/vfio-pci/bind 2>/dev/null || true
echo "$AUD" > /sys/bus/pci/drivers_probe 2>/dev/null || true

echo "[4/4] Verifying..."
sleep 1
DRIVER=$(readlink "/sys/bus/pci/devices/$BDF/driver" 2>/dev/null | xargs basename 2>/dev/null || echo "none")
echo "$BDF driver: $DRIVER"
