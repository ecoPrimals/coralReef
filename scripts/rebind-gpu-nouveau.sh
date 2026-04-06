#!/bin/bash
# SPDX-License-Identifier: AGPL-3.0-or-later
# Rebind a GPU from vfio-pci to nouveau at runtime.
#
# Usage: sudo bash scripts/rebind-gpu-nouveau.sh [BDF]
#   BDF defaults to $CORALREEF_VFIO_BDF, then 0000:03:00.0
set -euo pipefail

BDF="${1:-${CORALREEF_VFIO_BDF:-0000:03:00.0}}"
AUD="${BDF%.*}.1"

echo "[1/5] Setting driver_override → nouveau for $BDF..."
echo nouveau > "/sys/bus/pci/devices/$BDF/driver_override"
echo "" > "/sys/bus/pci/devices/$AUD/driver_override" 2>/dev/null || true

echo "[2/5] Unbinding GPU from vfio-pci..."
echo "$BDF" > /sys/bus/pci/drivers/vfio-pci/unbind 2>/dev/null || echo "  (not bound, skipping)"

echo "[3/5] Unbinding audio from vfio-pci..."
echo "$AUD" > /sys/bus/pci/drivers/vfio-pci/unbind 2>/dev/null || echo "  (not bound, skipping)"

echo "[4/5] Probing GPU for nouveau..."
echo "$BDF" > /sys/bus/pci/drivers_probe 2>/dev/null || \
  echo "$BDF" > /sys/bus/pci/drivers/nouveau/bind 2>/dev/null || true

echo "[5/5] Waiting for nouveau init (3s)..."
sleep 3

DRIVER=$(readlink "/sys/bus/pci/devices/$BDF/driver" 2>/dev/null | xargs basename 2>/dev/null || echo "none")
echo ""
echo "$BDF driver: $DRIVER"
echo "DRI devices:"
ls -la /dev/dri/

if [ "$DRIVER" = "nouveau" ]; then
    echo ""
    echo "SUCCESS — $BDF is on nouveau."
else
    echo ""
    echo "WARNING — $BDF did not bind to nouveau (driver=$DRIVER)"
    echo "Check dmesg: dmesg | tail -30 | grep -i nouveau"
fi
