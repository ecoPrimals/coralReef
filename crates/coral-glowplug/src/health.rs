// SPDX-License-Identifier: AGPL-3.0-only
#![expect(
    missing_docs,
    reason = "health_loop signature is covered by module docs; missing_docs kept at crate level."
)]
//! Periodic health monitor for managed devices.
//!
//! Runs a background loop that checks each device's VRAM, power state,
//! and PRI bus health at a configurable interval.

use std::sync::Arc;
use tokio::sync::Mutex;

/// Circuit breaker threshold: after this many consecutive faulted reads,
/// stop probing BAR0 registers entirely to avoid kernel instability.
const CIRCUIT_BREAKER_THRESHOLD: u32 = 6;

/// Ping the systemd watchdog via `NOTIFY_SOCKET` (datagram).
///
/// Called every health tick so systemd knows the daemon is alive.
/// No-op if `NOTIFY_SOCKET` is not set (non-systemd environments).
fn notify_watchdog() {
    static SOCKET_PATH: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    let path = SOCKET_PATH.get_or_init(|| std::env::var("NOTIFY_SOCKET").ok());
    if let Some(p) = path {
        let _ = std::os::unix::net::UnixDatagram::unbound()
            .and_then(|sock| sock.send_to(b"WATCHDOG=1", p));
    }
}

pub async fn health_loop(
    devices: Arc<Mutex<Vec<crate::device::DeviceSlot>>>,
    interval_ms: u64,
    shutdown: &mut tokio::sync::watch::Receiver<bool>,
) {
    let interval = std::time::Duration::from_millis(interval_ms);
    let mut consecutive_dead: std::collections::HashMap<Arc<str>, u32> =
        std::collections::HashMap::new();
    let mut tripped: std::collections::HashMap<Arc<str>, bool> = std::collections::HashMap::new();

    loop {
        tokio::select! {
            () = tokio::time::sleep(interval) => {}
            _ = shutdown.changed() => {
                tracing::info!("health loop: shutdown signal received");
                return;
            }
        }

        notify_watchdog();

        let mut devs = devices.lock().await;
        for slot in devs.iter_mut() {
            let bdf = slot.bdf.clone();
            let is_tripped = tripped.get(&bdf).copied().unwrap_or(false);

            if is_tripped {
                // Circuit breaker open — skip ALL BAR0 reads for this device.
                // Only do safe sysfs-based power state checks.
                slot.refresh_power_state();
                continue;
            }

            let prev_vram = slot.health.vram_alive;
            let prev_power = slot.health.power;

            slot.check_health();

            let dead_count = consecutive_dead.entry(bdf.clone()).or_insert(0);

            if slot.health.vram_alive {
                *dead_count = 0;
            } else if slot.has_vfio() {
                *dead_count += 1;
            }

            if prev_vram && !slot.health.vram_alive {
                tracing::warn!(
                    bdf = %slot.bdf,
                    consecutive_dead = *dead_count,
                    "VRAM went dead! power={} domains={}/{}",
                    slot.health.power,
                    slot.health.domains_alive,
                    slot.health.domains_alive + slot.health.domains_faulted,
                );
            }

            // Circuit breaker: stop reading hardware if it's consistently faulted
            if *dead_count >= CIRCUIT_BREAKER_THRESHOLD {
                tracing::error!(
                    bdf = %slot.bdf,
                    consecutive_dead = *dead_count,
                    "CIRCUIT BREAKER TRIPPED — halting BAR0 register reads for {bdf}. \
                     GPU hardware is persistently faulted. Manual intervention or \
                     reboot required to reset."
                );
                tripped.insert(bdf.clone(), true);
                continue;
            }

            if prev_power != slot.health.power {
                tracing::info!(
                    bdf = %slot.bdf,
                    from = %prev_power,
                    to = %slot.health.power,
                    "power state changed"
                );

                if slot.health.power == crate::device::PowerState::D3Hot
                    && slot.config.power_policy == "always_on"
                {
                    tracing::info!(bdf = %slot.bdf, "auto-recovering D0 (policy=always_on)");
                    let _ = crate::sysfs::sysfs_write(
                        &format!("/sys/bus/pci/devices/{}/power/control", slot.bdf),
                        "on",
                    );
                }
            }

            // Auto-resurrection DISABLED: sysfs driver/unbind from glowplug is
            // unsafe while ember holds VFIO fds. Use `swap_device` RPC via ember
            // for manual resurrection instead.
            if *dead_count >= 3
                && slot.has_vfio()
                && slot.config.power_policy == "always_on"
                && crate::sysfs::read_current_driver(&slot.bdf).as_deref() != Some("nvidia")
            {
                tracing::warn!(
                    bdf = %slot.bdf,
                    consecutive_dead = *dead_count,
                    "VRAM dead for 3+ checks — auto-resurrection DISABLED. \
                     Use ember swap_device RPC to manually resurrect: \
                     swap to nouveau then back to vfio."
                );
                *dead_count = 0;
            } else if *dead_count >= 3
                && crate::sysfs::read_current_driver(&slot.bdf).as_deref() == Some("nvidia")
            {
                tracing::warn!(
                    bdf = %slot.bdf,
                    consecutive_dead = *dead_count,
                    "REFUSING auto-resurrection — nvidia is bound to this device. \
                     Unbind nvidia from this BDF before resurrection."
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::device::{DeviceHealth, PowerState};

    #[test]
    fn test_power_state_display() {
        assert_eq!(PowerState::D0.to_string(), "D0");
        assert_eq!(PowerState::D3Hot.to_string(), "D3hot");
        assert_eq!(PowerState::D3Cold.to_string(), "D3cold");
        assert_eq!(PowerState::Unknown.to_string(), "unknown");
    }

    #[test]
    fn test_device_health_default_values() {
        let health = DeviceHealth {
            vram_alive: false,
            boot0: 0,
            pmc_enable: 0,
            power: PowerState::Unknown,
            pci_link_width: None,
            domains_alive: 0,
            domains_faulted: 0,
        };
        assert!(!health.vram_alive);
        assert_eq!(health.boot0, 0);
        assert_eq!(health.pmc_enable, 0);
        assert_eq!(health.power, PowerState::Unknown);
        assert!(health.pci_link_width.is_none());
        assert_eq!(health.domains_alive, 0);
        assert_eq!(health.domains_faulted, 0);
    }

    #[test]
    fn test_device_health_with_values() {
        let health = DeviceHealth {
            vram_alive: true,
            boot0: 0x1234_5678,
            pmc_enable: 0x9abc_def0,
            power: PowerState::D0,
            pci_link_width: Some(16),
            domains_alive: 8,
            domains_faulted: 1,
        };
        assert_eq!(health.power.to_string(), "D0");
        assert!(health.vram_alive);
        assert_eq!(health.pci_link_width, Some(16));
        assert_eq!(health.domains_alive, 8);
        assert_eq!(health.domains_faulted, 1);
    }
}
