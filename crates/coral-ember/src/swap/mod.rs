// SPDX-License-Identifier: AGPL-3.0-only
//! swap_device — the core ember-centric driver swap orchestrator.
//!
//! This module is the ONLY place where sysfs driver/unbind and
//! drivers/*/bind writes happen. Glowplug never touches these paths.
//!
//! Driver transitions are mediated by [`VendorLifecycle`](crate::vendor_lifecycle::VendorLifecycle)
//! hooks that encode vendor-specific knowledge (reset method quirks, power state
//! management, rebind strategies). See [`vendor_lifecycle`] module.

mod bind;
mod preflight;

#[cfg(test)]
pub(crate) mod swap_test_lock {
    use std::sync::Mutex;

    pub static SWAP_TEST_LOCK: Mutex<()> = Mutex::new(());
}

pub use preflight::verify_drm_isolation_with_paths;

use crate::hold::HeldDevice;
use crate::journal::Journal;
use crate::observation::{self, HealthResult, SwapObservation, SwapTiming};
use crate::sysfs;
use crate::vendor_lifecycle::{self, VendorLifecycle};
use bind::{bind_native, bind_vfio};
use coral_driver::linux_paths;
use preflight::{count_external_vfio_group_holders, is_active_display_gpu, preflight_device_check};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

/// Unbind/rebind the device to `target` (e.g. `vfio-pci`, `amdgpu`, `unbound`), updating `held`.
///
/// Returns a [`SwapObservation`] with per-phase timing, trace artifacts, and
/// health results. This observation is also suitable for journal persistence
/// and cross-personality comparison.
///
/// When `trace` is `true`, the kernel mmiotrace facility is enabled around
/// the driver bind, capturing every MMIO write the driver performs during
/// initialization. The trace is saved to the configured data directory.
///
/// # Errors
///
/// Returns an error string when sysfs/VFIO operations fail, external VFIO holders are detected, or
/// DRM isolation checks fail for DRM targets.
pub fn handle_swap_device(
    bdf: &str,
    target: &str,
    held: &mut HashMap<String, HeldDevice>,
    enable_trace: bool,
) -> Result<SwapObservation, String> {
    handle_swap_device_with_journal(bdf, target, held, enable_trace, None, false)
}

/// Inner swap implementation that optionally wraps the lifecycle in [`AdaptiveLifecycle`](crate::adaptive::AdaptiveLifecycle)
/// when a journal is provided.
///
/// When `allow_cold` is `true`, the preflight check permits swapping a
/// cold/un-POSTed device as long as BAR0 is readable. This enables the
/// cold-POST path where nouveau is bound to a cold Kepler GPU to POST it.
pub fn handle_swap_device_with_journal(
    bdf: &str,
    target: &str,
    held: &mut HashMap<String, HeldDevice>,
    enable_trace: bool,
    journal: Option<&Arc<Journal>>,
    allow_cold: bool,
) -> Result<SwapObservation, String> {
    let swap_start = Instant::now();
    let timestamp = observation::epoch_ms();
    tracing::info!(bdf, target, trace = enable_trace, "swap_device: starting");

    const KNOWN_TARGETS: &[&str] = &[
        "vfio",
        "vfio-pci",
        "nouveau",
        "amdgpu",
        "nvidia",
        "nvidia-open",
        "nvidia_oracle",
        "xe",
        "i915",
        "akida-pcie",
        "unbound",
    ];
    let target_matches = KNOWN_TARGETS.contains(&target) || target.starts_with("nvidia_oracle_");
    if !target_matches {
        return Err(format!("swap_device: unknown target driver '{target}'"));
    }

    if target == "unbound"
        && !std::path::Path::new(&linux_paths::sysfs_pci_device_path(bdf)).exists()
    {
        tracing::info!(
            bdf,
            "device absent from sysfs — already effectively unbound"
        );
        return Ok(SwapObservation {
            bdf: bdf.to_string(),
            from_personality: None,
            to_personality: "unbound".to_string(),
            timestamp_epoch_ms: timestamp,
            timing: SwapTiming {
                prepare_ms: 0,
                unbind_ms: 0,
                bind_ms: 0,
                stabilize_ms: 0,
                total_ms: swap_start.elapsed().as_millis() as u64,
            },
            trace_path: None,
            health: HealthResult::Ok,
            lifecycle_description: "absent device".to_string(),
            reset_method_used: None,
            firmware_pre: None,
            firmware_post: None,
        });
    }

    if is_active_display_gpu(bdf) {
        let msg = format!(
            "swap_device BLOCKED for {bdf}: device is an active display GPU. \
             Unbinding it would crash the system (kernel NULL deref in nvidia_modeset). \
             Refusing to proceed."
        );
        tracing::error!("{msg}");
        return Err(msg);
    }

    preflight_device_check(bdf, allow_cold)?;

    let base_lifecycle = vendor_lifecycle::detect_lifecycle_for_target(bdf, target);
    let lifecycle: Box<dyn VendorLifecycle> = if let Some(j) = journal {
        Box::new(crate::adaptive::AdaptiveLifecycle::new(
            base_lifecycle,
            Arc::clone(j),
            bdf.to_string(),
        ))
    } else {
        base_lifecycle
    };
    let lifecycle_desc = lifecycle.description().to_string();
    tracing::info!(bdf, lifecycle = %lifecycle_desc, "vendor lifecycle detected");

    let external = count_external_vfio_group_holders(bdf);
    if external > 0 {
        tracing::error!(
            bdf,
            external,
            "swap_device: ABORTING — external process(es) still hold VFIO fds. \
             Glowplug must drop its vfio_holder before calling swap_device."
        );
        return Err(format!(
            "swap_device aborted: {external} external VFIO fd holder(s) detected for {bdf}. \
             Call swap through glowplug RPC (which drops fds first), not directly via ember."
        ));
    }

    // --- Phase 1: Prepare ---
    let prepare_start = Instant::now();
    let current = sysfs::read_current_driver(bdf);
    let from_personality = current.clone();

    // Vendor-specific preparation BEFORE dropping fds.
    // vfio-pci triggers a PCI reset when its last fd closes
    // (vfio_pci_core_disable). If we don't clear reset_method before
    // the fd drop, the reset fires and can kill the card.
    if let Some(ref drv) = current {
        lifecycle.prepare_for_unbind(bdf, drv)?;
    } else {
        sysfs::pin_power(bdf);
    }
    let prepare_ms = prepare_start.elapsed().as_millis() as u64;

    // --- Phase 1b: Pre-unbind BAR0 safety (Exp 138) ---
    //
    // Before releasing VFIO fds, restore known-dangerous BAR0 registers
    // that may have been modified by experiments (coralctl mmio write).
    // PRAMIN window (0x1700) is the most critical: if it points to the
    // wrong VRAM address, kernel teardown reads garbage and can hang.
    if let Some(held_dev) = held.get(bdf) {
        if held_dev.experiment_dirty {
            tracing::warn!(
                bdf,
                "pre-unbind: device is experiment-dirty — applying full BAR0 safety sweep"
            );
        }
        if let Ok(bar0) = held_dev.device.map_bar(0) {
            // Restore PRAMIN window to default (disabled)
            let pramin = bar0.read_u32(0x1700).unwrap_or(0);
            if pramin != 0 {
                tracing::warn!(
                    bdf,
                    pramin = format_args!("{pramin:#010x}"),
                    "pre-unbind: PRAMIN window was non-default, restoring to 0x0"
                );
                let _ = bar0.write_u32(0x1700, 0);
            }

            // BAR0 health check: BOOT0 must return a valid chip ID
            let boot0 = bar0.read_u32(0x0).unwrap_or(0xFFFF_FFFF);
            if boot0 == 0xFFFF_FFFF || boot0 == 0xBADF_1100 {
                tracing::error!(
                    bdf,
                    boot0 = format_args!("{boot0:#010x}"),
                    "pre-unbind: BAR0 is NOT responsive — device in bad state. \
                     Proceeding with guarded close but D-state risk is HIGH."
                );
            } else {
                tracing::info!(
                    bdf,
                    boot0 = format_args!("{boot0:#010x}"),
                    "pre-unbind: BAR0 responsive"
                );
            }
        }
    }

    // --- Phase 2: Unbind ---
    let unbind_start = Instant::now();

    // Release held VFIO fds (reset_method already cleared).
    //
    // The fd close is performed in an isolated thread because
    // `vfio_pci_core_disable` (triggered by the last fd close) accesses
    // PCI config space. If the device is in a bad state (e.g. from
    // register experiments), this can cause a PCIe completion timeout
    // that puts the thread into uninterruptible D-state. Thread isolation
    // ensures the main ember thread stays responsive. (Exp 138)
    if let Some(device) = held.remove(bdf) {
        let dev_fd = device.device.device_fd();
        tracing::info!(bdf, device_fd = dev_fd, "swap_device: dropping VFIO fds (guarded)");

        let (tx, rx) = std::sync::mpsc::channel::<()>();
        let close_bdf = bdf.to_string();
        let handle = std::thread::Builder::new()
            .name(format!("vfio-close-{bdf}"))
            .spawn(move || {
                drop(device);
                let _ = tx.send(());
            });

        match handle {
            Ok(h) => {
                const VFIO_CLOSE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);
                match rx.recv_timeout(VFIO_CLOSE_TIMEOUT) {
                    Ok(()) => {
                        let _ = h.join();
                        let fd_path = linux_paths::proc_self_fd(dev_fd);
                        if std::path::Path::new(&fd_path).exists() {
                            tracing::warn!(
                                bdf = %close_bdf,
                                fd = dev_fd,
                                "swap_device: fd still in proc self fd table after drop!"
                            );
                        }
                    }
                    Err(_) => {
                        tracing::error!(
                            bdf = %close_bdf,
                            device_fd = dev_fd,
                            "swap_device: VFIO fd close TIMED OUT after {}s — \
                             thread likely in D-state, leaked. Device may be in bad state \
                             from register experiments (see Exp 138).",
                            VFIO_CLOSE_TIMEOUT.as_secs()
                        );
                        std::mem::forget(h);
                    }
                }
            }
            Err(e) => {
                // spawn failed — the closure (and `device` inside it) was dropped
                // by the runtime when Builder::spawn returned Err.
                tracing::error!(bdf, error = %e, "swap_device: failed to spawn vfio-close thread — device dropped with closure");
            }
        }

        std::thread::sleep(std::time::Duration::from_millis(100));
    } else {
        tracing::info!(
            bdf,
            "swap_device: no VFIO fds held (device not in ember map)"
        );
    }

    // Unbind current driver.
    if let Some(ref drv) = current {
        tracing::info!(bdf, driver = %drv, "swap_device: unbinding current driver");
        sysfs::sysfs_write(
            &linux_paths::sysfs_pci_device_file(bdf, "driver/unbind"),
            bdf,
        )?;
        std::thread::sleep(std::time::Duration::from_millis(500));
        sysfs::pin_power(bdf);
    }
    let unbind_ms = unbind_start.elapsed().as_millis() as u64;

    // --- Phase 3: Bind ---
    let bind_start = Instant::now();
    let mut trace_path_captured: Option<String> = None;

    let bind_result = if enable_trace && crate::trace::is_mmiotrace_available() {
        tracing::info!(bdf, target, "mmiotrace capture enabled for bind");
        let (result, tp) = crate::trace::with_mmiotrace(bdf, target, || {
            let inner = match target {
                "vfio" | "vfio-pci" => bind_vfio(bdf, held, &*lifecycle),
                "unbound" => Ok("unbound".to_string()),
                _ => bind_native(bdf, target, &*lifecycle),
            };
            inner.map_err(crate::error::SwapError::Other)
        });
        if let Some(ref path) = tp {
            tracing::info!(bdf, target, path = %path, "mmiotrace saved");
        }
        trace_path_captured = tp;
        result.map_err(|e| e.to_string())
    } else {
        if enable_trace {
            tracing::warn!(
                bdf,
                target,
                "mmiotrace requested but debugfs tracer not available"
            );
        }
        match target {
            "vfio" | "vfio-pci" => bind_vfio(bdf, held, &*lifecycle),
            "unbound" => Ok("unbound".to_string()),
            _ => bind_native(bdf, target, &*lifecycle),
        }
    };

    let personality = bind_result?;
    let bind_ms = bind_start.elapsed().as_millis() as u64;

    // --- Phase 4: Stabilize (already done inside bind_vfio/bind_native, measure residual) ---
    let stabilize_start = Instant::now();
    // bind_vfio and bind_native already call stabilize_after_bind + verify_health,
    // so this phase captures any additional overhead.
    let stabilize_ms = stabilize_start.elapsed().as_millis() as u64;

    let total_ms = swap_start.elapsed().as_millis() as u64;

    let obs = SwapObservation {
        bdf: bdf.to_string(),
        from_personality,
        to_personality: personality,
        timestamp_epoch_ms: timestamp,
        timing: SwapTiming {
            prepare_ms,
            unbind_ms,
            bind_ms,
            stabilize_ms,
            total_ms,
        },
        trace_path: trace_path_captured,
        health: HealthResult::Ok,
        lifecycle_description: lifecycle_desc,
        reset_method_used: None,
        firmware_pre: None,
        firmware_post: None,
    };

    tracing::info!(
        bdf,
        to = %obs.to_personality,
        total_ms,
        prepare_ms,
        unbind_ms,
        bind_ms,
        "swap_device: complete"
    );

    Ok(obs)
}

#[cfg(test)]
mod tests {
    use super::swap_test_lock::SWAP_TEST_LOCK;
    use super::*;
    use crate::drm_isolation;
    use crate::hold::HeldDevice;
    use std::collections::HashMap;

    const NONEXISTENT_BDF: &str = "9999:99:99.9";

    #[test]
    fn handle_swap_nvidia_oracle_target_is_recognized_before_preflight() {
        let _guard = SWAP_TEST_LOCK
            .lock()
            .expect("swap tests must not run concurrently with other swap IPC tests");
        let mut held: HashMap<String, HeldDevice> = HashMap::new();
        let err = handle_swap_device(NONEXISTENT_BDF, "nvidia_oracle_535", &mut held, false)
            .expect_err("absent BDF must not complete swap");
        assert!(
            err.contains("preflight") || err.contains("swap_device"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn handle_swap_unbound_without_sysfs_device_succeeds() {
        let _guard = SWAP_TEST_LOCK
            .lock()
            .expect("swap tests must not run concurrently with other swap IPC tests");
        let mut held: HashMap<String, HeldDevice> = HashMap::new();
        let obs = handle_swap_device(NONEXISTENT_BDF, "unbound", &mut held, false)
            .expect("unbound on absent device returns ok");
        assert_eq!(obs.to_personality, "unbound");
    }

    #[test]
    fn handle_swap_unknown_target_errors() {
        let _guard = SWAP_TEST_LOCK
            .lock()
            .expect("swap tests must not run concurrently with other swap IPC tests");
        let mut held: HashMap<String, HeldDevice> = HashMap::new();
        let err = handle_swap_device(NONEXISTENT_BDF, "not-a-real-driver", &mut held, false)
            .expect_err("unknown driver target must error");
        assert!(
            err.contains("unknown target driver") || err.contains("preflight"),
            "expected unknown-target or preflight error, got: {err}"
        );
    }

    #[test]
    fn default_xorg_path_contains_default_marker() {
        let _guard = SWAP_TEST_LOCK
            .lock()
            .expect("swap tests must not run concurrently with other swap IPC tests");
        let p = drm_isolation::default_xorg_path();
        assert!(
            p.contains("coralreef-gpu-isolation"),
            "unexpected xorg path: {p}"
        );
    }

    #[test]
    fn default_udev_path_contains_default_marker() {
        let p = drm_isolation::default_udev_path();
        assert!(
            p.contains("coralreef-drm-ignore"),
            "unexpected udev path: {p}"
        );
    }

    #[test]
    fn handle_swap_vfio_targets_fail_without_sysfs_and_vfio() {
        let _guard = SWAP_TEST_LOCK
            .lock()
            .expect("swap tests must not run concurrently with other swap IPC tests");
        let mut held_vfio = HashMap::new();
        let err_vfio = handle_swap_device(NONEXISTENT_BDF, "vfio", &mut held_vfio, false);
        let mut held_pci = HashMap::new();
        let err_vfio_pci = handle_swap_device(NONEXISTENT_BDF, "vfio-pci", &mut held_pci, false);
        assert!(err_vfio.is_err());
        assert!(err_vfio_pci.is_err());
        let msg = err_vfio.expect_err("vfio swap on absent device must fail");
        assert!(
            msg.contains("VFIO") || msg.contains("sysfs") || msg.contains("swap_device"),
            "unexpected error message: {msg}"
        );
    }
}
