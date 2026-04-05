#!/bin/bash
# Warm GV100 Titan V via nouveau cycle, rebind vfio-pci without reset
set -e
BDF="${1:-0000:03:00.0}"

echo "=== GPU Warm Cycle: $BDF ==="

sudo systemctl stop coral-glowplug.service 2>/dev/null || true
sleep 1
sudo systemctl stop coral-ember.service 2>/dev/null || true
sleep 1

echo "$BDF" | sudo tee /sys/bus/pci/drivers/vfio-pci/unbind 2>/dev/null || true
sleep 1

echo "nouveau" | sudo tee /sys/bus/pci/devices/$BDF/driver_override > /dev/null
sudo modprobe nouveau 2>/dev/null || true
echo "$BDF" | sudo tee /sys/bus/pci/drivers_probe > /dev/null
sleep 5
dmesg | grep "nouveau.*VRAM\|nouveau.*fb:" | tail -2

echo "$BDF" | sudo tee /sys/bus/pci/drivers/nouveau/unbind > /dev/null
sleep 2

echo "" | sudo tee /sys/bus/pci/devices/$BDF/reset_method 2>/dev/null || true
echo "vfio-pci" | sudo tee /sys/bus/pci/devices/$BDF/driver_override > /dev/null
echo "$BDF" | sudo tee /sys/bus/pci/drivers_probe > /dev/null
sleep 1

sudo setpci -s 00:01.3 CAP_EXP+0x28.W=0402
sudo setpci -s $BDF CAP_EXP+0x28.W=0401

sudo systemctl start coral-ember.service
sleep 2
sudo systemctl start coral-glowplug.service
sleep 3

systemctl is-active coral-ember.service coral-glowplug.service
echo "=== WARM CYCLE COMPLETE ==="
