#!/bin/bash
# SPDX-License-Identifier: AGPL-3.0-only
# Restore Titan V (03:00.0) back to vfio-pci from nouveau.
#
# Usage: sudo bash scripts/rebind-titanv-vfio.sh
set -euo pipefail

PCI_GPU="0000:03:00.0"
PCI_AUD="0000:03:00.1"

echo "[1/4] Setting driver_override → vfio-pci..."
echo vfio-pci > /sys/bus/pci/devices/$PCI_GPU/driver_override
echo vfio-pci > /sys/bus/pci/devices/$PCI_AUD/driver_override

echo "[2/4] Unbinding GPU from nouveau..."
echo $PCI_GPU > /sys/bus/pci/drivers/nouveau/unbind 2>/dev/null || echo "  (not bound, skipping)"

echo "[3/4] Probing for vfio-pci..."
echo $PCI_GPU > /sys/bus/pci/drivers_probe 2>/dev/null || \
  echo $PCI_GPU > /sys/bus/pci/drivers/vfio-pci/bind 2>/dev/null || true
echo $PCI_AUD > /sys/bus/pci/drivers_probe 2>/dev/null || true

echo "[4/4] Verifying..."
sleep 1
DRIVER=$(readlink /sys/bus/pci/devices/$PCI_GPU/driver 2>/dev/null | xargs basename 2>/dev/null || echo "none")
echo "Titan V driver: $DRIVER"
