// SPDX-License-Identifier: AGPL-3.0-only
//! swap_device — the core ember-centric driver swap orchestrator.
//!
//! This module is the ONLY place where sysfs driver/unbind and
//! drivers/*/bind writes happen. Glowplug never touches these paths.

use std::collections::HashMap;
use crate::hold::HeldDevice;
use crate::sysfs;

const XORG_ISOLATION_CONF: &str = "/etc/X11/xorg.conf.d/11-coralreef-gpu-isolation.conf";
const UDEV_ISOLATION_RULES: &str = "/etc/udev/rules.d/61-coralreef-drm-ignore.rules";

fn verify_drm_isolation(bdf: &str) -> Result<(), String> {
    let mut failures = Vec::new();

    match std::fs::read_to_string(XORG_ISOLATION_CONF) {
        Ok(content) => {
            if !content.contains("AutoAddGPU") || !content.contains("false") {
                failures.push(format!(
                    "{XORG_ISOLATION_CONF} exists but missing 'AutoAddGPU false'"
                ));
            }
        }
        Err(_) => {
            failures.push(format!(
                "{XORG_ISOLATION_CONF} missing — Xorg will hotplug DRM devices and crash compositor"
            ));
        }
    }

    match std::fs::read_to_string(UDEV_ISOLATION_RULES) {
        Ok(content) => {
            if !content.contains(bdf) {
                failures.push(format!(
                    "{UDEV_ISOLATION_RULES} exists but does not cover BDF {bdf}"
                ));
            }
        }
        Err(_) => {
            failures.push(format!(
                "{UDEV_ISOLATION_RULES} missing — logind will assign DRM device to seat0"
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
            if let Ok(target) = std::fs::read_link(fd_entry.path()) {
                if target.to_string_lossy() == group_path {
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
    }
    count
}

pub fn handle_swap_device(
    bdf: &str,
    target: &str,
    held: &mut HashMap<String, HeldDevice>,
) -> Result<String, String> {
    tracing::info!(bdf, target, "swap_device: starting");

    let external = count_external_vfio_group_holders(bdf);
    if external > 0 {
        tracing::error!(
            bdf, external,
            "swap_device: ABORTING — external process(es) still hold VFIO fds. \
             Glowplug must drop its vfio_holder before calling swap_device."
        );
        return Err(format!(
            "swap_device aborted: {external} external VFIO fd holder(s) detected for {bdf}. \
             Call swap through glowplug RPC (which drops fds first), not directly via ember."
        ));
    }

    // Step 1: release held VFIO fds if we have them.
    if let Some(device) = held.remove(bdf) {
        let dev_fd = device.device.device_fd();
        tracing::info!(bdf, device_fd = dev_fd, "swap_device: dropping VFIO fds");
        drop(device);
        let fd_path = format!("/proc/self/fd/{dev_fd}");
        if std::path::Path::new(&fd_path).exists() {
            tracing::warn!(bdf, fd = dev_fd, "swap_device: fd still in /proc/self/fd after drop!");
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    } else {
        tracing::info!(bdf, "swap_device: no VFIO fds held (device not in ember map)");
    }

    // Step 2: pin power before any driver transitions
    sysfs::pin_power(bdf);

    // Step 3: unbind current driver
    let current = sysfs::read_current_driver(bdf);
    if let Some(ref drv) = current {
        tracing::info!(bdf, driver = %drv, "swap_device: unbinding current driver");
        sysfs::sysfs_write(
            &format!("/sys/bus/pci/devices/{bdf}/driver/unbind"),
            bdf,
        )?;
        std::thread::sleep(std::time::Duration::from_millis(500));
        sysfs::pin_power(bdf);
    }

    // Step 4: bind to target driver
    match target {
        "vfio" | "vfio-pci" => {
            let group_id = sysfs::read_iommu_group(bdf);

            sysfs::sysfs_write(
                &format!("/sys/bus/pci/devices/{bdf}/driver_override"),
                "vfio-pci",
            )?;

            sysfs::bind_iommu_group_to_vfio(bdf, group_id);

            let _ = sysfs::sysfs_write("/sys/bus/pci/drivers/vfio-pci/bind", bdf);
            std::thread::sleep(std::time::Duration::from_millis(500));
            sysfs::pin_power(bdf);

            match coral_driver::vfio::VfioDevice::open(bdf) {
                Ok(device) => {
                    tracing::info!(
                        bdf,
                        container_fd = device.container_fd(),
                        group_fd = device.group_fd(),
                        device_fd = device.device_fd(),
                        "swap_device: VFIO fds reacquired"
                    );
                    held.insert(bdf.to_string(), HeldDevice {
                        bdf: bdf.to_string(),
                        device,
                    });
                }
                Err(e) => {
                    return Err(format!("swap_device: VFIO reacquire failed: {e}"));
                }
            }

            Ok("vfio".to_string())
        }
        "nouveau" | "amdgpu" | "nvidia" => {
            verify_drm_isolation(bdf)?;

            sysfs::sysfs_write(
                &format!("/sys/bus/pci/devices/{bdf}/driver_override"),
                "\n",
            )?;
            std::thread::sleep(std::time::Duration::from_millis(200));

            let _ = sysfs::sysfs_write(
                &format!("/sys/bus/pci/drivers/{target}/bind"),
                bdf,
            );

            let wait_secs: u64 = if target == "nouveau" { 10 } else { 5 };
            for attempt in 0..wait_secs {
                std::thread::sleep(std::time::Duration::from_secs(1));
                let drv = sysfs::read_current_driver(bdf);
                if drv.as_deref() == Some(target) {
                    if sysfs::find_drm_card(bdf).is_some() {
                        tracing::info!(bdf, target, attempt, "swap_device: driver init complete (DRM up)");
                        break;
                    }
                }
                tracing::debug!(bdf, target, attempt, driver = ?drv, "swap_device: waiting for driver init");
            }

            sysfs::pin_power(bdf);

            let actual = sysfs::read_current_driver(bdf);
            if actual.as_deref() != Some(target) {
                tracing::warn!(
                    bdf, target,
                    actual = ?actual,
                    "swap_device: target driver did not bind"
                );
            }

            Ok(target.to_string())
        }
        "unbound" => {
            Ok("unbound".to_string())
        }
        _ => Err(format!("swap_device: unknown target driver '{target}'")),
    }
}
