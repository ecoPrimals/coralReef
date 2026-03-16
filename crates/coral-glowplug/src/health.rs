// SPDX-License-Identifier: AGPL-3.0-only
//! Periodic health monitor for managed devices.
//!
//! Runs a background loop that checks each device's VRAM, power state,
//! and PRI bus health at a configurable interval.

use std::sync::Arc;
use tokio::sync::Mutex;

pub async fn health_loop(
    devices: Arc<Mutex<Vec<crate::device::DeviceSlot>>>,
    interval_ms: u64,
) {
    let interval = std::time::Duration::from_millis(interval_ms);

    loop {
        tokio::time::sleep(interval).await;

        let mut devs = devices.lock().await;
        for slot in devs.iter_mut() {
            let prev_vram = slot.health.vram_alive;
            let prev_power = slot.health.power;

            slot.check_health();

            // Log state changes
            if prev_vram && !slot.health.vram_alive {
                tracing::warn!(
                    bdf = %slot.bdf,
                    "VRAM went dead! power={} domains={}/{}",
                    slot.health.power,
                    slot.health.domains_alive,
                    slot.health.domains_alive + slot.health.domains_faulted,
                );
            }

            if prev_power != slot.health.power {
                tracing::info!(
                    bdf = %slot.bdf,
                    from = %prev_power,
                    to = %slot.health.power,
                    "power state changed"
                );

                // Auto-recovery: if device went to D3hot, force D0
                if slot.health.power == crate::device::PowerState::D3Hot
                    && slot.config.power_policy == "always_on"
                {
                    tracing::info!(bdf = %slot.bdf, "auto-recovering D0 (policy=always_on)");
                    crate::device::sysfs_write(
                        &format!("/sys/bus/pci/devices/{}/power/control", slot.bdf),
                        "on",
                    );
                }
            }
        }
    }
}
