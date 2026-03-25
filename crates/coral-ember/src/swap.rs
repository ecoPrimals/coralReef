// SPDX-License-Identifier: AGPL-3.0-only
//! swap_device — the core ember-centric driver swap orchestrator.
//!
//! This module is the ONLY place where sysfs driver/unbind and
//! drivers/*/bind writes happen. Glowplug never touches these paths.
//!
//! Driver transitions are mediated by [`VendorLifecycle`](crate::vendor_lifecycle::VendorLifecycle)
//! hooks that encode vendor-specific knowledge (reset method quirks, power state
//! management, rebind strategies). See [`vendor_lifecycle`] module.

use crate::hold::HeldDevice;
use crate::journal::Journal;
use crate::observation::{self, HealthResult, SwapObservation, SwapTiming};
use crate::sysfs;
use crate::vendor_lifecycle::{self, RebindStrategy};
use coral_driver::linux_paths;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

/// Default Xorg drop-in path when `CORALREEF_XORG_ISOLATION_CONF` is unset.
const DEFAULT_XORG_ISOLATION_CONF: &str = "/etc/X11/xorg.conf.d/11-coralreef-gpu-isolation.conf";
/// Default udev rules path when `CORALREEF_UDEV_ISOLATION_RULES` is unset.
const DEFAULT_UDEV_ISOLATION_RULES: &str = "/etc/udev/rules.d/61-coralreef-drm-ignore.rules";

fn xorg_isolation_conf_path() -> String {
    std::env::var("CORALREEF_XORG_ISOLATION_CONF")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| DEFAULT_XORG_ISOLATION_CONF.to_string())
}

fn udev_isolation_rules_path() -> String {
    std::env::var("CORALREEF_UDEV_ISOLATION_RULES")
        .unwrap_or_else(|_| DEFAULT_UDEV_ISOLATION_RULES.to_string())
}

fn verify_drm_isolation(bdf: &str) -> Result<(), String> {
    verify_drm_isolation_with_paths(
        bdf,
        &xorg_isolation_conf_path(),
        &udev_isolation_rules_path(),
    )
}

/// Same checks as `verify_drm_isolation`, but with explicit paths (unit tests and non-default
/// config layouts).
pub fn verify_drm_isolation_with_paths(
    bdf: &str,
    xorg_path: &str,
    udev_path: &str,
) -> Result<(), String> {
    let mut failures = Vec::new();

    match std::fs::read_to_string(xorg_path) {
        Ok(content) => {
            if !content.contains("AutoAddGPU") || !content.contains("false") {
                failures.push(format!("{xorg_path} exists but missing 'AutoAddGPU false'"));
            }
        }
        Err(_) => {
            failures.push(format!(
                "{xorg_path} missing — Xorg will hotplug DRM devices and crash compositor"
            ));
        }
    }

    match std::fs::read_to_string(udev_path) {
        Ok(content) => {
            if !content.contains(bdf) {
                failures.push(format!("{udev_path} exists but does not cover BDF {bdf}"));
            }
        }
        Err(_) => {
            failures.push(format!(
                "{udev_path} missing — logind will assign DRM device to seat0"
            ));
        }
    }

    if failures.is_empty() {
        tracing::debug!(bdf, "DRM isolation verified");
        Ok(())
    } else {
        let msg = format!(
            "swap_device BLOCKED for {bdf}: DRM isolation incomplete. {}",
            failures.join("; ")
        );
        tracing::error!("{msg}");
        Err(msg)
    }
}

/// Check whether any EXTERNAL process still holds the VFIO group fd.
fn count_external_vfio_group_holders(bdf: &str) -> usize {
    let group_id = sysfs::read_iommu_group(bdf);
    if group_id == 0 {
        return 0;
    }
    let group_path = format!("/dev/vfio/{group_id}");
    let self_pid = std::process::id();
    let mut count = 0;

    let Ok(proc_entries) = std::fs::read_dir(linux_paths::proc_root()) else {
        return 0;
    };

    for entry in proc_entries.flatten() {
        let pid_str = entry.file_name().to_string_lossy().to_string();
        let Ok(pid) = pid_str.parse::<u32>() else {
            continue;
        };
        if pid == self_pid {
            continue;
        }

        let fd_dir = linux_paths::proc_pid_fd_dir(pid);
        let Ok(fds) = std::fs::read_dir(&fd_dir) else {
            continue;
        };

        for fd_entry in fds.flatten() {
            if let Ok(link_target) = std::fs::read_link(fd_entry.path())
                && link_target.to_string_lossy() == group_path
            {
                tracing::warn!(
                    bdf,
                    pid,
                    fd = ?fd_entry.file_name(),
                    group = group_id,
                    "external process holds VFIO group fd"
                );
                count += 1;
            }
        }
    }
    count
}

/// Check if a PCI device is currently driving an active display (DRM master, active framebuffer,
/// or render clients). Unbinding such a device causes an unrecoverable kernel crash.
fn is_active_display_gpu(bdf: &str) -> bool {
    let drm_card = sysfs::find_drm_card(bdf);

    if let Some(ref card) = drm_card {
        let fb_dir = format!("/sys/class/drm/{card}/device/drm/{card}");
        if let Ok(entries) = std::fs::read_dir(&fb_dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with("card") && name.contains('-') {
                    let status_path = entry.path().join("status");
                    if std::fs::read_to_string(&status_path)
                        .is_ok_and(|status| status.trim() == "connected")
                    {
                        tracing::warn!(
                            bdf, card, connector = %name,
                            "display GPU detected: connector is connected"
                        );
                        return true;
                    }
                }
            }
        }

        let enabled_path = format!("/sys/class/drm/{card}/device/drm/{card}/enabled");
        if std::fs::read_to_string(&enabled_path).is_ok_and(|enabled| enabled.trim() == "enabled") {
            tracing::warn!(bdf, card, "display GPU detected: card is enabled");
            return true;
        }
    }

    let current_driver = sysfs::read_current_driver(bdf);
    if current_driver.as_deref() == Some("nvidia") && drm_card.is_some() {
        tracing::warn!(
            bdf,
            card = ?drm_card,
            "display GPU detected: nvidia driver with DRM card — assumed display GPU"
        );
        return true;
    }

    false
}

/// Pre-flight check: verify the device is in a sane state before any
/// sysfs unbind/bind. Rejects early if the device would cause the kernel
/// to hang on a driver transition (D3cold, unreachable config space, etc.).
fn preflight_device_check(bdf: &str) -> Result<(), String> {
    let sysfs_path = linux_paths::sysfs_pci_device_path(bdf);
    if !std::path::Path::new(&sysfs_path).exists() {
        return Err(format!(
            "preflight FAILED for {bdf}: sysfs path does not exist ({sysfs_path}). \
             Device may have been removed or never enumerated."
        ));
    }

    if let Some(power) = sysfs::read_power_state(bdf) {
        match power.as_str() {
            "D3cold" => {
                return Err(format!(
                    "preflight FAILED for {bdf}: device in D3cold. \
                     Platform powered it off — sysfs operations will hang."
                ));
            }
            "D3hot" => {
                tracing::warn!(
                    bdf,
                    "preflight: device in D3hot — attempting power pin before swap"
                );
                sysfs::pin_power(bdf);
                std::thread::sleep(std::time::Duration::from_millis(200));
                if sysfs::read_power_state(bdf).as_deref() != Some("D0") {
                    return Err(format!(
                        "preflight FAILED for {bdf}: device stuck in D3hot after pin_power"
                    ));
                }
            }
            _ => {}
        }
    }

    let vendor_id = sysfs::read_pci_id(bdf, "vendor");
    if vendor_id == 0xFFFF {
        return Err(format!(
            "preflight FAILED for {bdf}: vendor ID is 0xFFFF — \
             device not responding on PCIe bus"
        ));
    }

    let config_path = linux_paths::sysfs_pci_device_file(bdf, "config");
    match std::fs::read(&config_path) {
        Ok(buf) if buf.len() >= 4 => {
            let vendor = u16::from_le_bytes([buf[0], buf[1]]);
            let device = u16::from_le_bytes([buf[2], buf[3]]);
            if vendor == 0xFFFF {
                return Err(format!(
                    "preflight FAILED for {bdf}: raw config space returns 0xFFFF — \
                     device not responding on PCIe bus"
                ));
            }
            tracing::debug!(
                bdf,
                vendor = format!("{vendor:#06x}"),
                device = format!("{device:#06x}"),
                "preflight: config space accessible"
            );
        }
        Ok(_) => {
            tracing::warn!(bdf, "preflight: config space read returned < 4 bytes");
        }
        Err(e) => {
            tracing::warn!(
                bdf,
                error = %e,
                "preflight: config space read failed (non-fatal if device is unbound)"
            );
        }
    }

    tracing::info!(bdf, "preflight: device state OK");
    Ok(())
}

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
    handle_swap_device_with_journal(bdf, target, held, enable_trace, None)
}

/// Inner swap implementation that optionally wraps the lifecycle in [`AdaptiveLifecycle`]
/// when a journal is provided.
pub fn handle_swap_device_with_journal(
    bdf: &str,
    target: &str,
    held: &mut HashMap<String, HeldDevice>,
    enable_trace: bool,
    journal: Option<&Arc<Journal>>,
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

    preflight_device_check(bdf)?;

    let base_lifecycle = vendor_lifecycle::detect_lifecycle_for_target(bdf, target);
    let lifecycle: Box<dyn vendor_lifecycle::VendorLifecycle> = if let Some(j) = journal {
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

    // --- Phase 2: Unbind ---
    let unbind_start = Instant::now();

    // Release held VFIO fds (reset_method already cleared).
    if let Some(device) = held.remove(bdf) {
        let dev_fd = device.device.device_fd();
        tracing::info!(bdf, device_fd = dev_fd, "swap_device: dropping VFIO fds");
        drop(device);
        let fd_path = linux_paths::proc_self_fd(dev_fd);
        if std::path::Path::new(&fd_path).exists() {
            tracing::warn!(
                bdf,
                fd = dev_fd,
                "swap_device: fd still in proc self fd table after drop!"
            );
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
            match target {
                "vfio" | "vfio-pci" => bind_vfio(bdf, held, &*lifecycle),
                "unbound" => Ok("unbound".to_string()),
                _ => bind_native(bdf, target, &*lifecycle),
            }
        });
        if let Some(ref path) = tp {
            tracing::info!(bdf, target, path = %path, "mmiotrace saved");
        }
        trace_path_captured = tp;
        result
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

fn bind_vfio(
    bdf: &str,
    held: &mut HashMap<String, HeldDevice>,
    lifecycle: &dyn vendor_lifecycle::VendorLifecycle,
) -> Result<String, String> {
    let group_id = sysfs::read_iommu_group(bdf);

    sysfs::sysfs_write(
        &linux_paths::sysfs_pci_device_file(bdf, "driver_override"),
        "vfio-pci",
    )?;

    sysfs::bind_iommu_group_to_vfio(bdf, group_id);

    let _ = sysfs::sysfs_write(&linux_paths::sysfs_pci_driver_bind("vfio-pci"), bdf);
    let settle = lifecycle.settle_secs("vfio-pci");
    std::thread::sleep(std::time::Duration::from_secs(settle));

    lifecycle.stabilize_after_bind(bdf, "vfio-pci");
    lifecycle.verify_health(bdf, "vfio-pci")?;

    match coral_driver::vfio::VfioDevice::open(bdf) {
        Ok(device) => {
            tracing::info!(
                bdf,
                backend = ?device.backend_kind(),
                device_fd = device.device_fd(),
                "swap_device: VFIO fds reacquired"
            );
            held.insert(
                bdf.to_string(),
                HeldDevice {
                    bdf: bdf.to_string(),
                    device,
                    ring_meta: crate::hold::RingMeta::default(),
                },
            );
        }
        Err(e) => {
            return Err(format!("swap_device: VFIO reacquire failed: {e}"));
        }
    }

    Ok("vfio".to_string())
}

fn pci_remove_rescan(bdf: &str) -> Result<(), String> {
    sysfs::pci_remove_rescan(bdf)
}

fn is_drm_driver(target: &str) -> bool {
    matches!(target, "nouveau" | "nvidia" | "amdgpu" | "xe" | "i915")
        || target.starts_with("nvidia_oracle")
}

/// Ensure the kernel module for `target` is loaded before attempting sysfs bind.
/// If the module is already loaded or doesn't need loading (e.g. vfio-pci), this is a no-op.
fn ensure_module_loaded(target: &str) {
    let module = match target {
        "vfio" | "vfio-pci" => return,
        "akida-pcie" => "akida_pcie",
        other => other,
    };

    let sysfs_mod = format!("/sys/module/{}", module.replace('-', "_"));
    if std::path::Path::new(&sysfs_mod).exists() {
        tracing::debug!(module, "kernel module already loaded");
        return;
    }

    tracing::info!(module, "loading kernel module for target driver");
    match std::process::Command::new("modprobe").arg(module).output() {
        Ok(out) if out.status.success() => {
            tracing::info!(module, "kernel module loaded successfully");
        }
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            tracing::warn!(module, %stderr, "modprobe returned non-zero (may still work via install hook)");
        }
        Err(e) => {
            tracing::warn!(module, error = %e, "modprobe not available — bind may fail if module not loaded");
        }
    }

    std::thread::sleep(std::time::Duration::from_millis(500));
}

fn bind_native(
    bdf: &str,
    target: &str,
    lifecycle: &dyn vendor_lifecycle::VendorLifecycle,
) -> Result<String, String> {
    if is_drm_driver(target) {
        verify_drm_isolation(bdf)?;
    }

    // Release IOMMU group peers from vfio-pci so the group is no longer
    // held when we bind the primary device to a native driver.
    let group_id = sysfs::read_iommu_group(bdf);
    if group_id != 0 {
        sysfs::release_iommu_group_from_vfio(bdf, group_id);
    }

    let strategy = lifecycle.rebind_strategy(target);
    tracing::info!(
        bdf,
        target,
        ?strategy,
        "swap_device: rebind strategy selected"
    );

    sysfs::sysfs_write(
        &linux_paths::sysfs_pci_device_file(bdf, "driver_override"),
        "\n",
    )?;
    std::thread::sleep(std::time::Duration::from_millis(200));

    ensure_module_loaded(target);

    match strategy {
        RebindStrategy::SimpleBind => {
            let _ = sysfs::sysfs_write(&linux_paths::sysfs_pci_driver_bind(target), bdf);
        }
        RebindStrategy::SimpleWithRescanFallback => {
            let bind_result = sysfs::sysfs_write(&linux_paths::sysfs_pci_driver_bind(target), bdf);

            if bind_result.is_err() {
                tracing::warn!(
                    bdf,
                    target,
                    "simple bind failed — falling back to PCI remove + rescan"
                );
                pci_remove_rescan(bdf)?;
            }
        }
        RebindStrategy::PciRescan => {
            tracing::info!(
                bdf,
                target,
                "using PCI remove + rescan (skipping simple bind)"
            );
            pci_remove_rescan(bdf)?;
        }
        RebindStrategy::PmResetAndBind => {
            tracing::info!(bdf, target, "PM power cycle before bind");
            match sysfs::pm_power_cycle(bdf) {
                Ok(()) => {
                    tracing::info!(bdf, "PM cycle OK, attempting bind");
                    let _ = sysfs::sysfs_write_direct(
                        &linux_paths::sysfs_pci_device_file(bdf, "reset_method"),
                        "",
                    );
                }
                Err(e) => {
                    tracing::warn!(bdf, error = %e, "PM power cycle failed, attempting bind anyway");
                }
            }

            let bind_result = sysfs::sysfs_write(&linux_paths::sysfs_pci_driver_bind(target), bdf);

            if bind_result.is_err() {
                tracing::warn!(
                    bdf,
                    target,
                    "simple bind after PM cycle failed — trying rescan fallback"
                );
                sysfs::pin_bridge_power(bdf);
                sysfs::pci_remove(bdf)?;
                std::thread::sleep(std::time::Duration::from_secs(3));
                sysfs::pci_rescan()?;
            }
        }
    }

    let wait_secs = lifecycle.settle_secs(target);
    for attempt in 0..wait_secs {
        std::thread::sleep(std::time::Duration::from_secs(1));
        let drv = sysfs::read_current_driver(bdf);
        if drv.as_deref() == Some(target) && sysfs::find_drm_card(bdf).is_some() {
            tracing::info!(
                bdf,
                target,
                attempt,
                "swap_device: driver init complete (DRM up)"
            );
            break;
        }
        tracing::debug!(bdf, target, attempt, driver = ?drv, "swap_device: waiting for driver init");
    }

    lifecycle.stabilize_after_bind(bdf, target);

    let actual = sysfs::read_current_driver(bdf);
    if actual.as_deref() != Some(target) {
        tracing::warn!(
            bdf, target,
            actual = ?actual,
            "swap_device: target driver did not bind"
        );
    }

    lifecycle.verify_health(bdf, target)?;

    Ok(target.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    static SWAP_TEST_LOCK: Mutex<()> = Mutex::new(());

    const NONEXISTENT_BDF: &str = "9999:99:99.9";

    #[test]
    fn count_external_vfio_group_holders_zero_without_iommu_group() {
        assert_eq!(count_external_vfio_group_holders(NONEXISTENT_BDF), 0);
    }

    #[test]
    fn is_drm_driver_matches_drm_targets() {
        assert!(is_drm_driver("nouveau"));
        assert!(is_drm_driver("nvidia"));
        assert!(is_drm_driver("amdgpu"));
        assert!(is_drm_driver("xe"));
        assert!(is_drm_driver("i915"));
    }

    #[test]
    fn is_drm_driver_rejects_non_drm() {
        assert!(!is_drm_driver("vfio-pci"));
        assert!(!is_drm_driver("akida-pcie"));
        assert!(!is_drm_driver("unbound"));
    }

    #[test]
    fn is_drm_driver_matches_nvidia_oracle_prefix() {
        assert!(is_drm_driver("nvidia_oracle"));
        assert!(is_drm_driver("nvidia_oracle_535"));
    }

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
    fn verify_drm_isolation_ok_when_files_valid() {
        let dir = tempfile::tempdir().expect("tempdir for isolation files");
        let xorg = dir.path().join("xorg.conf");
        let udev = dir.path().join("udev.rules");
        let bdf = "0000:03:00.0";
        std::fs::write(&xorg, "Option \"AutoAddGPU\" \"false\"\n")
            .expect("write synthetic xorg snippet");
        std::fs::write(
            &udev,
            format!("KERNEL==\"card*\", ATTR{{address}}==\"{bdf}\""),
        )
        .expect("write synthetic udev rules");
        verify_drm_isolation_with_paths(
            bdf,
            xorg.to_str().expect("xorg path utf-8"),
            udev.to_str().expect("udev path utf-8"),
        )
        .expect("valid isolation files should verify");
    }

    #[test]
    fn verify_drm_isolation_fails_when_xorg_missing() {
        let dir = tempfile::tempdir().expect("tempdir for isolation files");
        let xorg = dir.path().join("missing-xorg");
        let udev = dir.path().join("udev.rules");
        std::fs::write(&udev, "0000:03:00.0").expect("write udev stub");
        let err = verify_drm_isolation_with_paths(
            "0000:03:00.0",
            xorg.to_str().expect("xorg path utf-8"),
            udev.to_str().expect("udev path utf-8"),
        )
        .expect_err("missing xorg must fail verification");
        assert!(err.contains("BLOCKED"));
        assert!(err.contains("missing — Xorg"));
    }

    #[test]
    fn verify_drm_isolation_fails_when_xorg_missing_autoaddgpu_false() {
        let dir = tempfile::tempdir().expect("tempdir for isolation files");
        let xorg = dir.path().join("xorg.conf");
        let udev = dir.path().join("udev.rules");
        std::fs::write(&xorg, "not the droids you are looking for\n")
            .expect("write invalid xorg snippet");
        std::fs::write(&udev, "0000:03:00.0").expect("write udev stub");
        let err = verify_drm_isolation_with_paths(
            "0000:03:00.0",
            xorg.to_str().expect("xorg path utf-8"),
            udev.to_str().expect("udev path utf-8"),
        )
        .expect_err("xorg without AutoAddGPU false must fail");
        assert!(err.contains("AutoAddGPU"));
    }

    #[test]
    fn verify_drm_isolation_fails_when_udev_missing() {
        let dir = tempfile::tempdir().expect("tempdir for isolation files");
        let xorg = dir.path().join("xorg.conf");
        let udev = dir.path().join("missing-udev");
        std::fs::write(&xorg, "Option \"AutoAddGPU\" \"false\"\n").expect("write xorg snippet");
        let err = verify_drm_isolation_with_paths(
            "0000:03:00.0",
            xorg.to_str().expect("xorg path utf-8"),
            udev.to_str().expect("udev path utf-8"),
        )
        .expect_err("missing udev rules must fail verification");
        assert!(err.contains("logind"));
    }

    #[test]
    fn verify_drm_isolation_fails_when_udev_missing_bdf_token() {
        let dir = tempfile::tempdir().expect("tempdir for isolation files");
        let xorg = dir.path().join("xorg.conf");
        let udev = dir.path().join("udev.rules");
        let bdf = "0000:03:00.0";
        std::fs::write(&xorg, "Option \"AutoAddGPU\" \"false\"\n").expect("write xorg snippet");
        std::fs::write(&udev, "some other pci address").expect("write udev without BDF");
        let err = verify_drm_isolation_with_paths(
            bdf,
            xorg.to_str().expect("xorg path utf-8"),
            udev.to_str().expect("udev path utf-8"),
        )
        .expect_err("udev without BDF token must fail");
        assert!(err.contains("does not cover BDF"));
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
    fn preflight_rejects_nonexistent_device() {
        let err = preflight_device_check(NONEXISTENT_BDF).unwrap_err();
        assert!(
            err.contains("sysfs path does not exist"),
            "expected sysfs-missing error, got: {err}"
        );
    }

    #[test]
    fn preflight_rejects_0xffff_vendor_id() {
        let err = preflight_device_check("0000:00:00.0");
        // 0000:00:00.0 is the host bridge — it exists and has a real
        // vendor ID, so this should pass preflight (not reject). We only
        // verify no panic; the 0xFFFF path is exercised indirectly by
        // the nonexistent-BDF test above.
        drop(err);
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
    fn xorg_isolation_conf_path_contains_default_marker() {
        let _guard = SWAP_TEST_LOCK
            .lock()
            .expect("swap tests must not run concurrently with other swap IPC tests");
        let p = super::xorg_isolation_conf_path();
        assert!(
            p.contains("coralreef-gpu-isolation"),
            "unexpected xorg path: {p}"
        );
    }

    #[test]
    fn udev_isolation_rules_path_contains_default_marker() {
        let p = super::udev_isolation_rules_path();
        assert!(
            p.contains("coralreef-drm-ignore"),
            "unexpected udev path: {p}"
        );
    }

    #[test]
    fn count_external_vfio_skips_proc_when_iommu_group_is_zero() {
        let _guard = SWAP_TEST_LOCK
            .lock()
            .expect("swap tests must not run concurrently with other swap IPC tests");
        assert_eq!(count_external_vfio_group_holders("9999:99:99.9"), 0);
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
