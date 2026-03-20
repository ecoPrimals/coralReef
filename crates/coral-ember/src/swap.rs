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
use crate::sysfs;
use crate::vendor_lifecycle::{self, RebindStrategy};
use std::collections::HashMap;

/// Default Xorg drop-in path when `CORALREEF_XORG_ISOLATION_CONF` is unset.
const DEFAULT_XORG_ISOLATION_CONF: &str = "/etc/X11/xorg.conf.d/11-coralreef-gpu-isolation.conf";
/// Default udev rules path when `CORALREEF_UDEV_ISOLATION_RULES` is unset.
const DEFAULT_UDEV_ISOLATION_RULES: &str = "/etc/udev/rules.d/61-coralreef-drm-ignore.rules";

fn xorg_isolation_conf_path() -> String {
    std::env::var("CORALREEF_XORG_ISOLATION_CONF")
        .unwrap_or_else(|_| DEFAULT_XORG_ISOLATION_CONF.to_string())
}

fn udev_isolation_rules_path() -> String {
    std::env::var("CORALREEF_UDEV_ISOLATION_RULES")
        .unwrap_or_else(|_| DEFAULT_UDEV_ISOLATION_RULES.to_string())
}

fn verify_drm_isolation(bdf: &str) -> Result<(), String> {
    let mut failures = Vec::new();

    let xorg_path = xorg_isolation_conf_path();
    let udev_path = udev_isolation_rules_path();

    match std::fs::read_to_string(&xorg_path) {
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

    match std::fs::read_to_string(&udev_path) {
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

    let Ok(proc_entries) = std::fs::read_dir("/proc") else {
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

        let fd_dir = format!("/proc/{pid}/fd");
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

pub fn handle_swap_device(
    bdf: &str,
    target: &str,
    held: &mut HashMap<String, HeldDevice>,
) -> Result<String, String> {
    tracing::info!(bdf, target, "swap_device: starting");

    let lifecycle = vendor_lifecycle::detect_lifecycle(bdf);
    tracing::info!(
        bdf,
        lifecycle = lifecycle.description(),
        "vendor lifecycle detected"
    );

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

    // Step 1: vendor-specific preparation BEFORE dropping fds.
    // This MUST happen first — vfio-pci triggers a PCI reset when its
    // last fd closes (vfio_pci_core_disable). If we don't clear
    // reset_method before the fd drop, the reset fires and can kill
    // the card (AMD Vega 20 → D3cold).
    let current = sysfs::read_current_driver(bdf);
    if let Some(ref drv) = current {
        lifecycle.prepare_for_unbind(bdf, drv)?;
    } else {
        sysfs::pin_power(bdf);
    }

    // Step 2: release held VFIO fds (reset_method already cleared).
    if let Some(device) = held.remove(bdf) {
        let dev_fd = device.device.device_fd();
        tracing::info!(bdf, device_fd = dev_fd, "swap_device: dropping VFIO fds");
        drop(device);
        let fd_path = format!("/proc/self/fd/{dev_fd}");
        if std::path::Path::new(&fd_path).exists() {
            tracing::warn!(
                bdf,
                fd = dev_fd,
                "swap_device: fd still in /proc/self/fd after drop!"
            );
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    } else {
        tracing::info!(
            bdf,
            "swap_device: no VFIO fds held (device not in ember map)"
        );
    }

    // Step 3: unbind current driver
    if let Some(ref drv) = current {
        tracing::info!(bdf, driver = %drv, "swap_device: unbinding current driver");
        sysfs::sysfs_write(&format!("/sys/bus/pci/devices/{bdf}/driver/unbind"), bdf)?;
        std::thread::sleep(std::time::Duration::from_millis(500));
        sysfs::pin_power(bdf);
    }

    // Step 4: bind to target driver using vendor-appropriate strategy
    match target {
        "vfio" | "vfio-pci" => bind_vfio(bdf, held, &*lifecycle),
        "nouveau" | "amdgpu" | "nvidia" | "xe" | "i915" | "akida-pcie" => {
            bind_native(bdf, target, &*lifecycle)
        }
        "unbound" => Ok("unbound".to_string()),
        _ => Err(format!("swap_device: unknown target driver '{target}'")),
    }
}

fn bind_vfio(
    bdf: &str,
    held: &mut HashMap<String, HeldDevice>,
    lifecycle: &dyn vendor_lifecycle::VendorLifecycle,
) -> Result<String, String> {
    let group_id = sysfs::read_iommu_group(bdf);

    sysfs::sysfs_write(
        &format!("/sys/bus/pci/devices/{bdf}/driver_override"),
        "vfio-pci",
    )?;

    sysfs::bind_iommu_group_to_vfio(bdf, group_id);

    let _ = sysfs::sysfs_write("/sys/bus/pci/drivers/vfio-pci/bind", bdf);
    let settle = lifecycle.settle_secs("vfio-pci");
    std::thread::sleep(std::time::Duration::from_secs(settle.min(2)));

    lifecycle.stabilize_after_bind(bdf, "vfio-pci");
    lifecycle.verify_health(bdf, "vfio-pci")?;

    match coral_driver::vfio::VfioDevice::open(bdf) {
        Ok(device) => {
            tracing::info!(
                bdf,
                container_fd = device.container_fd(),
                group_fd = device.group_fd(),
                device_fd = device.device_fd(),
                "swap_device: VFIO fds reacquired"
            );
            held.insert(
                bdf.to_string(),
                HeldDevice {
                    bdf: bdf.to_string(),
                    device,
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
    sysfs::pin_bridge_power(bdf);
    sysfs::pin_power(bdf);

    let _ = sysfs::sysfs_write(&format!("/sys/bus/pci/devices/{bdf}/reset_method"), "");

    tracing::info!(bdf, "PCI remove + rescan: removing device");
    sysfs::pci_remove(bdf)?;

    for i in 0..6 {
        std::thread::sleep(std::time::Duration::from_secs(1));
        if !std::path::Path::new(&format!("/sys/bus/pci/devices/{bdf}")).exists() {
            tracing::info!(bdf, seconds = i + 1, "device removed from sysfs");
            break;
        }
    }

    std::thread::sleep(std::time::Duration::from_secs(2));

    tracing::info!(bdf, "PCI remove + rescan: rescanning bus");
    sysfs::pci_rescan()?;

    for i in 0..10 {
        std::thread::sleep(std::time::Duration::from_secs(1));
        if std::path::Path::new(&format!("/sys/bus/pci/devices/{bdf}")).exists() {
            tracing::info!(bdf, seconds = i + 1, "device re-appeared after rescan");

            sysfs::pin_power(bdf);
            sysfs::pin_bridge_power(bdf);
            let _ = sysfs::sysfs_write(&format!("/sys/bus/pci/devices/{bdf}/reset_method"), "");
            return Ok(());
        }
    }

    Err(format!("{bdf}: device did not re-appear after PCI rescan"))
}

fn is_drm_driver(target: &str) -> bool {
    matches!(target, "nouveau" | "nvidia" | "amdgpu" | "xe" | "i915")
}

fn bind_native(
    bdf: &str,
    target: &str,
    lifecycle: &dyn vendor_lifecycle::VendorLifecycle,
) -> Result<String, String> {
    if is_drm_driver(target) {
        verify_drm_isolation(bdf)?;
    }

    let strategy = lifecycle.rebind_strategy(target);
    tracing::info!(
        bdf,
        target,
        ?strategy,
        "swap_device: rebind strategy selected"
    );

    sysfs::sysfs_write(&format!("/sys/bus/pci/devices/{bdf}/driver_override"), "\n")?;
    std::thread::sleep(std::time::Duration::from_millis(200));

    match strategy {
        RebindStrategy::SimpleBind => {
            let _ = sysfs::sysfs_write(&format!("/sys/bus/pci/drivers/{target}/bind"), bdf);
        }
        RebindStrategy::SimpleWithRescanFallback => {
            let bind_result =
                sysfs::sysfs_write(&format!("/sys/bus/pci/drivers/{target}/bind"), bdf);

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
                    let _ =
                        sysfs::sysfs_write(&format!("/sys/bus/pci/devices/{bdf}/reset_method"), "");
                }
                Err(e) => {
                    tracing::warn!(bdf, error = %e, "PM power cycle failed, attempting bind anyway");
                }
            }

            let bind_result =
                sysfs::sysfs_write(&format!("/sys/bus/pci/drivers/{target}/bind"), bdf);

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
