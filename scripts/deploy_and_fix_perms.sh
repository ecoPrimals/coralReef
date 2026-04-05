#!/bin/bash
set -euo pipefail

echo "=== Setting up passwordless sudo for coralreef GPU operations ==="
cat > /etc/sudoers.d/coralreef-ops << 'SUDOERS'
# Allow biomegate to manage coralreef services and GPU sysfs without password
biomegate ALL=(ALL) NOPASSWD: /usr/bin/systemctl start coral-*, /usr/bin/systemctl stop coral-*, /usr/bin/systemctl restart coral-*, /usr/bin/systemctl status coral-*
biomegate ALL=(ALL) NOPASSWD: /usr/bin/setpci, /usr/sbin/modprobe
biomegate ALL=(ALL) NOPASSWD: /usr/bin/tee /sys/bus/pci/drivers/*, /usr/bin/tee /sys/bus/pci/devices/*
biomegate ALL=(ALL) NOPASSWD: /usr/bin/cp, /usr/bin/install, /usr/bin/chmod, /usr/bin/mkdir, /usr/bin/chown
biomegate ALL=(ALL) NOPASSWD: /usr/bin/dmesg
SUDOERS
chmod 440 /etc/sudoers.d/coralreef-ops
echo "  sudoers installed"

echo "=== Stopping services ==="
systemctl stop coral-glowplug.service coral-ember.service 2>/dev/null || true
sleep 1

echo "=== Deploying binaries ==="
cp /home/biomegate/Development/ecoPrimals/primals/coralReef/target/release/coral-ember /usr/local/bin/coral-ember
cp /home/biomegate/Development/ecoPrimals/primals/coralReef/target/release/coral-glowplug /usr/local/bin/coral-glowplug
echo "  binaries deployed"

echo "=== Creating trace directory ==="
mkdir -p /var/lib/coralreef/traces
chmod 777 /var/lib/coralreef/traces
echo "  trace dir ready"

echo "=== Starting ember ==="
systemctl start coral-ember.service
sleep 2
echo "=== Starting glowplug ==="
systemctl start coral-glowplug.service
sleep 1

echo "=== Service status ==="
systemctl is-active coral-ember.service || echo "ember NOT active"
systemctl is-active coral-glowplug.service || echo "glowplug NOT active"

echo "=== DONE ==="
