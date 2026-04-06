// SPDX-License-Identifier: AGPL-3.0-or-later
//! Pre-flight sysfs sanity checks, DRM isolation verification, and cold-GPU heuristics for
//! [`super::handle_swap_device`].

use crate::drm_isolation;
use crate::error::SwapError;
use crate::sysfs;
use coral_driver::linux_paths;

pub(in crate::swap) fn verify_drm_isolation(bdf: &str) -> Result<(), SwapError> {
    verify_drm_isolation_with_paths(
        bdf,
        &drm_isolation::default_xorg_path(),
        &drm_isolation::default_udev_path(),
    )
}

/// Same checks as `verify_drm_isolation`, but with explicit paths (unit tests and non-default
/// config layouts).
pub fn verify_drm_isolation_with_paths(
    bdf: &str,
    xorg_path: &str,
    udev_path: &str,
) -> Result<(), SwapError> {
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
        Err(SwapError::DrmIsolation(msg))
    }
}

/// Check whether any EXTERNAL process still holds the VFIO group fd.
pub(super) fn count_external_vfio_group_holders(bdf: &str) -> usize {
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
pub(super) fn is_active_display_gpu(bdf: &str) -> bool {
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
pub(super) fn preflight_device_check(bdf: &str) -> Result<(), SwapError> {
    let sysfs_path = linux_paths::sysfs_pci_device_path(bdf);
    if !std::path::Path::new(&sysfs_path).exists() {
        tracing::debug!(
            bdf,
            sysfs_path,
            "preflight: sysfs path absent — no device to validate (no blockers)"
        );
        return Ok(());
    }

    if let Some(power) = sysfs::read_power_state(bdf) {
        match power.as_str() {
            "D3cold" => {
                return Err(SwapError::Preflight {
                    bdf: bdf.to_string(),
                    reason:
                        "device in D3cold — platform powered it off; sysfs operations will hang"
                            .to_string(),
                });
            }
            "D3hot" => {
                tracing::warn!(
                    bdf,
                    "preflight: device in D3hot — attempting power pin before swap"
                );
                sysfs::pin_power(bdf);
                std::thread::sleep(std::time::Duration::from_millis(200));
                if sysfs::read_power_state(bdf).as_deref() != Some("D0") {
                    return Err(SwapError::Preflight {
                        bdf: bdf.to_string(),
                        reason: "device stuck in D3hot after pin_power".to_string(),
                    });
                }
            }
            _ => {}
        }
    }

    let vendor_id = sysfs::read_pci_id(bdf, "vendor");
    if vendor_id == 0xFFFF {
        return Err(SwapError::Preflight {
            bdf: bdf.to_string(),
            reason: "vendor ID is 0xFFFF — device not responding on PCIe bus".to_string(),
        });
    }

    let config_path = linux_paths::sysfs_pci_device_file(bdf, "config");
    match std::fs::read(&config_path) {
        Ok(buf) if buf.len() >= 4 => {
            let vendor = u16::from_le_bytes([buf[0], buf[1]]);
            let device = u16::from_le_bytes([buf[2], buf[3]]);
            if vendor == 0xFFFF {
                return Err(SwapError::Preflight {
                    bdf: bdf.to_string(),
                    reason: "raw config space returns 0xFFFF — device not responding on PCIe bus"
                        .to_string(),
                });
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

    // Cold-hardware detection: NVIDIA GPUs claimed by vfio-pci at boot
    // without a prior VBIOS POST have empty reset_method files. Unbinding
    // vfio-pci from such devices triggers PCI config-space writes that the
    // cold hardware doesn't complete, causing PCIe completion timeouts and
    // kernel D-state. Detect this and reject early with an actionable error.
    let current_driver = sysfs::read_current_driver(bdf);
    if current_driver.as_deref() == Some("vfio-pci") {
        let reset_path = linux_paths::sysfs_pci_device_file(bdf, "reset_method");
        let reset_methods = std::fs::read_to_string(&reset_path).unwrap_or_default();
        if reset_methods.trim().is_empty() {
            let resource0_path = linux_paths::sysfs_pci_device_file(bdf, "resource0");
            let is_cold = is_gpu_cold_via_ptimer(&resource0_path);
            if is_cold {
                return Err(SwapError::Preflight {
                    bdf: bdf.to_string(),
                    reason: "device is cold/un-POSTed (empty reset_method, PTIMER frozen). \
                     Unbinding vfio-pci will cause kernel D-state. Boot with nouveau first to POST \
                     the device, then swap to vfio-pci."
                        .to_string(),
                });
            }
            tracing::warn!(
                bdf,
                "preflight: empty reset_method but PTIMER appears alive — \
                 proceeding cautiously"
            );
        }
    }

    tracing::info!(bdf, "preflight: device state OK");
    Ok(())
}

/// Read BOOT0 (PMC_BOOT_0, BAR0 offset 0x000) from a GPU.
///
/// Returns the raw 32-bit register value, or `None` if BAR0 is not readable.
/// On an initialized GPU this returns the chipset ID (e.g. `0x108000a1` for
/// GK210). On cold/un-POSTed hardware it returns a hardware-default sentinel
/// (e.g. `0x0f22d0a1` for cold GK210).
pub(crate) fn read_boot0(resource0_path: &str) -> Option<u32> {
    use std::io::{Read, Seek, SeekFrom};

    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .open(resource0_path)
        .ok()?;
    file.seek(SeekFrom::Start(0)).ok()?;
    let mut buf = [0u8; 4];
    file.read_exact(&mut buf).ok()?;
    Some(u32::from_le_bytes(buf))
}

/// Combined cold-GPU heuristic: read BOOT0 and PTIMER from BAR0.
///
/// A GPU is considered cold/un-POSTed when:
/// - PTIMER (offset 0x9400) is frozen (two reads return the same value), OR
/// - BOOT0 doesn't match known initialized chipset patterns
///
/// Logs diagnostic details (BOOT0 value, PTIMER readings) for debugging.
pub(super) fn is_gpu_cold_via_ptimer(resource0_path: &str) -> bool {
    use std::io::{Read, Seek, SeekFrom};

    const PTIMER_LOW: u64 = 0x9400;

    let mut file = match std::fs::OpenOptions::new().read(true).open(resource0_path) {
        Ok(f) => f,
        Err(_) => return false,
    };

    let read_u32_at = |f: &mut std::fs::File, offset: u64| -> Option<u32> {
        f.seek(SeekFrom::Start(offset)).ok()?;
        let mut buf = [0u8; 4];
        f.read_exact(&mut buf).ok()?;
        Some(u32::from_le_bytes(buf))
    };

    let boot0 = read_boot0(resource0_path);
    let Some(t1) = read_u32_at(&mut file, PTIMER_LOW) else {
        return false;
    };
    std::thread::sleep(std::time::Duration::from_millis(50));
    let Some(t2) = read_u32_at(&mut file, PTIMER_LOW) else {
        return false;
    };

    let ptimer_frozen = t1 == t2;

    if let Some(b0) = boot0 {
        // Extract chipset from BOOT0: bits [28:20] = 9-bit chipset ID
        let chipset = (b0 & 0x1ff0_0000) >> 20;
        let is_known_chipset = matches!(
            chipset,
            // Kepler
            0x0e0 | 0x0e4 | 0x0e6 | 0x0e7 | 0x0f0 | 0x0f1 | 0x106 | 0x108 |
            // Maxwell
            0x117 | 0x118 | 0x120 | 0x124 | 0x126 | 0x12b |
            // Pascal
            0x130 | 0x132 | 0x134 | 0x136 | 0x137 | 0x138 |
            // Volta
            0x140 | 0x142 |
            // Turing
            0x160 | 0x162 | 0x164 | 0x166 | 0x168 |
            // Ampere
            0x170 | 0x172 | 0x174 | 0x176 | 0x177 | 0x178 | 0x179 |
            // Ada / Hopper / Blackwell
            0x190 | 0x192 | 0x193 | 0x194 | 0x196 | 0x197
        );

        if ptimer_frozen || !is_known_chipset {
            tracing::warn!(
                resource0_path,
                boot0 = format!("{b0:#010x}"),
                chipset = format!("{chipset:#05x}"),
                ptimer_t1 = format!("{t1:#010x}"),
                ptimer_t2 = format!("{t2:#010x}"),
                ptimer_frozen,
                known_chipset = is_known_chipset,
                "GPU cold-detection: device is un-POSTed (VBIOS DEVINIT never ran). \
                 POST via VM passthrough or manual DEVINIT before swapping."
            );
            return true;
        }
    } else if ptimer_frozen {
        tracing::warn!(
            resource0_path,
            ptimer_t1 = format!("{t1:#010x}"),
            ptimer_t2 = format!("{t2:#010x}"),
            "PTIMER frozen and BOOT0 unreadable — GPU is cold/un-POSTed"
        );
        return true;
    }

    false
}

#[cfg(test)]
mod tests {
    //! Unit tests for [`verify_drm_isolation_with_paths`], VFIO holder counting,
    //! display-GPU detection, preflight, cold-GPU heuristics, and BAR0 BOOT0 reads.

    use super::*;
    use std::fs;

    const TEST_BDF: &str = "0000:99:00.0";

    #[test]
    fn verify_drm_isolation_with_paths_passes_when_xorg_and_udev_rules_match() {
        let dir = tempfile::tempdir().expect("tempdir");
        let xorg = dir.path().join("xorg.conf");
        let udev = dir.path().join("99-gpu.rules");
        fs::write(
            &xorg,
            r#"
Section "ServerFlags"
    Option "AutoAddGPU" "false"
EndSection
"#,
        )
        .expect("write xorg");
        fs::write(
            &udev,
            format!(
                r#"KERNEL=="{TEST_BDF}", DRIVER=="vfio-pci", TAG+="seat0"
"#
            ),
        )
        .expect("write udev");

        let result = verify_drm_isolation_with_paths(
            TEST_BDF,
            xorg.to_str().expect("utf8 path"),
            udev.to_str().expect("utf8 path"),
        );
        assert!(result.is_ok(), "expected Ok, got {result:?}");
    }

    #[test]
    fn verify_drm_isolation_with_paths_fails_when_xorg_missing_autoaddgpu() {
        let dir = tempfile::tempdir().expect("tempdir");
        let xorg = dir.path().join("xorg.conf");
        let udev = dir.path().join("99-gpu.rules");
        fs::write(&xorg, "Section \"ServerFlags\"\nEndSection\n").expect("write xorg");
        fs::write(
            &udev,
            format!(r#"KERNEL=="{TEST_BDF}", DRIVER=="vfio-pci""#),
        )
        .expect("write udev");

        let err = verify_drm_isolation_with_paths(
            TEST_BDF,
            xorg.to_str().expect("utf8 path"),
            udev.to_str().expect("utf8 path"),
        )
        .expect_err("expected error when AutoAddGPU false is absent");
        assert!(
            err.to_string().contains("AutoAddGPU") || err.to_string().contains("missing"),
            "unexpected message: {err}"
        );
    }

    #[test]
    fn verify_drm_isolation_with_paths_fails_with_multiple_errors_when_paths_missing() {
        let err = verify_drm_isolation_with_paths(
            TEST_BDF,
            "/nonexistent/xorg.conf",
            "/nonexistent/udev.rules",
        )
        .expect_err("expected error when both config paths are missing");
        let s = err.to_string();
        assert!(
            s.contains("xorg.conf") && s.contains("udev.rules"),
            "expected both paths mentioned: {err}"
        );
    }

    #[test]
    fn count_external_vfio_group_holders_returns_zero_for_nonexistent_bdf() {
        assert_eq!(count_external_vfio_group_holders("0000:ff:ff:ff:ff"), 0);
    }

    #[test]
    fn is_active_display_gpu_returns_false_for_nonexistent_bdf() {
        assert!(!is_active_display_gpu("0000:ff:ff:ff:ff"));
    }

    #[test]
    fn preflight_device_check_ok_when_no_sysfs_for_bdf() {
        assert!(preflight_device_check("0000:ff:ff:ff:ff").is_ok());
    }

    #[test]
    fn is_gpu_cold_via_ptimer_returns_false_when_resource0_missing() {
        assert!(!is_gpu_cold_via_ptimer("/nonexistent/resource0"));
    }

    #[test]
    fn read_boot0_returns_none_for_nonexistent_path() {
        assert_eq!(read_boot0("/nonexistent/resource0"), None);
    }

    #[test]
    fn read_boot0_returns_some_le_u32_for_temp_file_with_four_bytes() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("resource0");
        let expected = 0x1234_5678_u32;
        fs::write(&path, expected.to_le_bytes()).expect("write resource0");

        assert_eq!(
            read_boot0(path.to_str().expect("utf8 path")),
            Some(expected)
        );
    }
}
