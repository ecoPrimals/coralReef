#!/bin/bash
# SPDX-License-Identifier: AGPL-3.0-only
# Swap Titan V (03:00.0) from vfio-pci to nouveau at runtime.
#
# Usage: sudo bash scripts/rebind-titanv-nouveau.sh
#
# After running, /dev/dri/renderD130 (or similar) should appear
# for nouveau and the nvidia_nouveau_e2e example can run.
set -euo pipefail

PCI_GPU="0000:03:00.0"
PCI_AUD="0000:03:00.1"

echo "[1/5] Setting driver_override → nouveau for GPU, clearing audio..."
echo nouveau > /sys/bus/pci/devices/$PCI_GPU/driver_override
echo "" > /sys/bus/pci/devices/$PCI_AUD/driver_override

echo "[2/5] Unbinding GPU from vfio-pci..."
echo $PCI_GPU > /sys/bus/pci/drivers/vfio-pci/unbind 2>/dev/null || echo "  (not bound, skipping)"

echo "[3/5] Unbinding audio from vfio-pci..."
echo $PCI_AUD > /sys/bus/pci/drivers/vfio-pci/unbind 2>/dev/null || echo "  (not bound, skipping)"

echo "[4/5] Probing GPU for nouveau..."
echo $PCI_GPU > /sys/bus/pci/drivers_probe 2>/dev/null || \
  echo $PCI_GPU > /sys/bus/pci/drivers/nouveau/bind 2>/dev/null || true

echo "[5/5] Waiting for nouveau init (3s)..."
sleep 3

DRIVER=$(readlink /sys/bus/pci/devices/$PCI_GPU/driver 2>/dev/null | xargs basename 2>/dev/null || echo "none")
echo ""
echo "Titan V driver: $DRIVER"
echo "DRI devices:"
ls -la /dev/dri/
echo ""

if [ "$DRIVER" = "nouveau" ]; then
    echo "SUCCESS — Titan V is on nouveau. Run the E2E test:"
    echo "  cd coralReef && cargo run --example nvidia_nouveau_e2e"
else
    echo "WARNING — Titan V did not bind to nouveau (driver=$DRIVER)"
    echo "Check dmesg: dmesg | tail -30 | grep -i nouveau"
fi
