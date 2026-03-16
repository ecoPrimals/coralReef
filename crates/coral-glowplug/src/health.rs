// SPDX-License-Identifier: AGPL-3.0-only
//! Periodic health monitor for managed devices.
//!
//! Runs a background loop that checks each device's VRAM, power state,
//! and PRI bus health at a configurable interval.

use std::sync::Arc;
use tokio::sync::Mutex;

pub async fn health_loop(devices: Arc<Mutex<Vec<crate::device::DeviceSlot>>>, interval_ms: u64) {
    let interval = std::time::Duration::from_millis(interval_ms);
    let mut consecutive_dead: std::collections::HashMap<String, u32> =
        std::collections::HashMap::new();

    loop {
        tokio::time::sleep(interval).await;

        let mut devs = devices.lock().await;
        for slot in devs.iter_mut() {
            let prev_vram = slot.health.vram_alive;
            let prev_power = slot.health.power;

            slot.check_health();

            // Track consecutive dead readings for auto-resurrection
            let dead_count = consecutive_dead.entry(slot.bdf.clone()).or_insert(0);

            if slot.health.vram_alive {
                *dead_count = 0;
            }

            // Log state changes
            if prev_vram && !slot.health.vram_alive {
                *dead_count += 1;
                tracing::warn!(
                    bdf = %slot.bdf,
                    consecutive_dead = *dead_count,
                    "VRAM went dead! power={} domains={}/{}",
                    slot.health.power,
                    slot.health.domains_alive,
                    slot.health.domains_alive + slot.health.domains_faulted,
                );
            } else if !slot.health.vram_alive && slot.has_vfio() {
                *dead_count += 1;
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

            // Auto-resurrect: if VRAM has been dead for 3+ consecutive checks
            // and we have VFIO access, attempt nouveau resurrection
            if *dead_count >= 3 && slot.has_vfio() && slot.config.power_policy == "always_on" {
                tracing::warn!(
                    bdf = %slot.bdf,
                    consecutive_dead = *dead_count,
                    "VRAM dead for 3+ checks — attempting auto-resurrection via nouveau"
                );
                match slot.resurrect_hbm2() {
                    Ok(true) => {
                        *dead_count = 0;
                        tracing::info!(
                            bdf = %slot.bdf,
                            domains = slot.health.domains_alive,
                            "AUTO-RESURRECTION SUCCEEDED — VRAM alive"
                        );
                    }
                    Ok(false) => {
                        tracing::error!(
                            bdf = %slot.bdf,
                            "auto-resurrection completed but VRAM still dead"
                        );
                        // Reset counter to avoid hammering
                        *dead_count = 0;
                    }
                    Err(e) => {
                        tracing::error!(
                            bdf = %slot.bdf,
                            error = %e,
                            "auto-resurrection failed"
                        );
                        *dead_count = 0;
                    }
                }
            }
        }
    }
}
