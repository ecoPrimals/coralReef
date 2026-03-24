// SPDX-License-Identifier: AGPL-3.0-only
//! Periodic health monitor for managed devices.
//!
//! Runs a background loop that checks each device's VRAM, power state,
//! and PRI bus health at a configurable interval.

use coral_driver::linux_paths;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Circuit breaker threshold: after this many consecutive faulted reads,
/// stop probing BAR0 registers entirely to avoid kernel instability.
const CIRCUIT_BREAKER_THRESHOLD: u32 = 6;

/// Incremented when the health loop trips the BAR0 circuit breaker (unit tests only).
#[cfg(test)]
pub(crate) static HEALTH_LOOP_TRIP_COUNT_FOR_TESTS: std::sync::atomic::AtomicU32 =
    std::sync::atomic::AtomicU32::new(0);

#[must_use]
fn d3hot_always_on_recovery_applies(power: crate::device::PowerState, power_policy: &str) -> bool {
    power == crate::device::PowerState::D3Hot && power_policy == "always_on"
}

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

/// Invokes [`notify_watchdog`] (for integration tests that adjust `NOTIFY_SOCKET` via `unsafe` env).
#[doc(hidden)]
pub fn test_support_notify_watchdog() {
    notify_watchdog();
}

/// Background health monitor for [`crate::device::DeviceSlot`] instances sharing a [`crate::sysfs_ops::SysfsOps`] backend.
pub async fn health_loop<S: crate::sysfs_ops::SysfsOps>(
    devices: Arc<Mutex<Vec<crate::device::DeviceSlot<S>>>>,
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
                #[cfg(test)]
                HEALTH_LOOP_TRIP_COUNT_FOR_TESTS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
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

                if d3hot_always_on_recovery_applies(
                    slot.health.power,
                    slot.config.power_policy.as_str(),
                ) {
                    tracing::info!(bdf = %slot.bdf, "auto-recovering D0 (policy=always_on)");
                    let _ = crate::sysfs::sysfs_write(
                        &linux_paths::sysfs_pci_device_file(&slot.bdf, "power/control"),
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
    use std::sync::Arc;
    use std::sync::atomic::Ordering;

    use crate::MockSysfs;
    use crate::config::DeviceConfig;
    use crate::device::{DeviceHealth, DeviceSlot, PowerState};

    use crate::sysfs_ops::RealSysfs;

    use super::HEALTH_LOOP_TRIP_COUNT_FOR_TESTS;
    use super::d3hot_always_on_recovery_applies;
    use super::health_loop;

    #[test]
    fn d3hot_always_on_recovery_only_for_matching_policy() {
        assert!(d3hot_always_on_recovery_applies(
            PowerState::D3Hot,
            "always_on"
        ));
        assert!(!d3hot_always_on_recovery_applies(
            PowerState::D3Hot,
            "power_save"
        ));
        assert!(!d3hot_always_on_recovery_applies(
            PowerState::D0,
            "always_on"
        ));
        assert!(!d3hot_always_on_recovery_applies(
            PowerState::D3Cold,
            "always_on"
        ));
        assert!(!d3hot_always_on_recovery_applies(
            PowerState::Unknown,
            "always_on"
        ));
    }

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

    #[tokio::test]
    async fn health_loop_exits_when_shutdown_watch_true() {
        let devices = Arc::new(tokio::sync::Mutex::new(vec![]));
        let (tx, mut rx) = tokio::sync::watch::channel(false);
        let d = devices.clone();
        let j = tokio::spawn(async move {
            health_loop::<RealSysfs>(d, 10_000, &mut rx).await;
        });
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        tx.send(true).expect("signal");
        tokio::time::timeout(std::time::Duration::from_secs(5), j)
            .await
            .expect("health_loop should finish")
            .expect("join");
    }

    #[tokio::test]
    async fn health_loop_trips_circuit_breaker_with_mock_slot() {
        HEALTH_LOOP_TRIP_COUNT_FOR_TESTS.store(0, Ordering::Relaxed);
        let bdf = "0000:aa:00.0";
        let config = DeviceConfig {
            bdf: bdf.into(),
            name: None,
            boot_personality: "vfio".into(),
            power_policy: "power_save".into(),
            role: None,
            oracle_dump: None,
            shared: None,
        };
        let mut mock = MockSysfs::default();
        mock.seed_bdf(bdf);
        let mut slot = DeviceSlot::with_sysfs(config, mock);
        slot.test_set_vfio_override(Some(true));

        let devices = Arc::new(tokio::sync::Mutex::new(vec![slot]));
        let (tx, mut rx) = tokio::sync::watch::channel(false);
        let d = devices.clone();
        let j = tokio::spawn(async move {
            health_loop::<MockSysfs>(d, 5, &mut rx).await;
        });
        tokio::time::sleep(std::time::Duration::from_millis(120)).await;
        assert!(
            HEALTH_LOOP_TRIP_COUNT_FOR_TESTS.load(Ordering::Relaxed) >= 1,
            "expected circuit breaker to trip"
        );
        tx.send(true).expect("shutdown");
        tokio::time::timeout(std::time::Duration::from_secs(5), j)
            .await
            .expect("join timeout")
            .expect("join");
    }

    #[test]
    fn notify_watchdog_test_hook_is_callable() {
        crate::test_support_notify_watchdog();
    }
}
