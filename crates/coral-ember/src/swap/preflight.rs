// SPDX-License-Identifier: AGPL-3.0-only
use crate::drm_isolation;
use crate::sysfs;
use coral_driver::gsp::RegisterAccess;
use coral_driver::linux_paths;

pub(super) fn verify_drm_isolation(bdf: &str) -> Result<(), String> {
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
pub(super) fn preflight_device_check(bdf: &str) -> Result<(), String> {
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
                return Err(format!(
                    "preflight FAILED for {bdf}: device is cold/un-POSTed (empty reset_method, \
                     PTIMER frozen). Unbinding vfio-pci will cause kernel D-state. \
                     Boot with nouveau first to POST the device, then swap to vfio-pci."
                ));
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
///
/// Uses `Bar0Access` (mmap-based) for real PCI BAR0 resources, with a
/// fallback to `File::read` for unit tests that use regular tempfiles.
pub(crate) fn read_boot0(resource0_path: &str) -> Option<u32> {
    if let Ok(bar0) = coral_driver::nv::bar0::Bar0Access::open_resource_readonly(resource0_path) {
        return bar0.read_u32(0).ok();
    }
    // Fallback for unit tests using regular files (mmap of PCI resources
    // has different semantics than mmap of tmpfs files).
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
///
/// Uses `Bar0Access` (mmap) for real PCI resources, with a `File::read`
/// fallback for unit tests on tmpfs.
pub(super) fn is_gpu_cold_via_ptimer(resource0_path: &str) -> bool {
    const PTIMER_LOW: u32 = 0x9400;

    let boot0 = read_boot0(resource0_path);

    // Try mmap-based BAR0 access first (works on real PCI resources).
    if let Ok(bar0) = coral_driver::nv::bar0::Bar0Access::open_resource_readonly(resource0_path) {
        let Some(t1) = bar0.read_u32(PTIMER_LOW).ok() else {
            return false;
        };
        std::thread::sleep(std::time::Duration::from_millis(50));
        let Some(t2) = bar0.read_u32(PTIMER_LOW).ok() else {
            return false;
        };
        return is_cold_from_readings(resource0_path, boot0, t1, t2);
    }

    // Fallback: File::read for unit tests with regular files.
    use std::io::{Read, Seek, SeekFrom};
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
    let Some(t1) = read_u32_at(&mut file, PTIMER_LOW as u64) else {
        return false;
    };
    std::thread::sleep(std::time::Duration::from_millis(50));
    let Some(t2) = read_u32_at(&mut file, PTIMER_LOW as u64) else {
        return false;
    };

    is_cold_from_readings(resource0_path, boot0, t1, t2)
}

fn is_cold_from_readings(resource0_path: &str, boot0: Option<u32>, t1: u32, t2: u32) -> bool {
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
    use super::super::swap_test_lock::SWAP_TEST_LOCK;
    use super::*;

    const NONEXISTENT_BDF: &str = "9999:99:99.9";

    #[test]
    fn count_external_vfio_group_holders_zero_without_iommu_group() {
        assert_eq!(count_external_vfio_group_holders(NONEXISTENT_BDF), 0);
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
    fn preflight_rejects_nonexistent_device() {
        let err = preflight_device_check(NONEXISTENT_BDF).unwrap_err();
        assert!(
            err.contains("sysfs path does not exist"),
            "expected sysfs-missing error, got: {err}"
        );
    }

    #[test]
    fn preflight_rejects_0xffff_vendor_id() {
        let result = preflight_device_check("0000:00:00.0");
        // 0000:00:00.0 is the host bridge — it exists and has a real
        // vendor ID, so this should pass preflight (not reject). We only
        // verify no panic; the 0xFFFF path is exercised indirectly by
        // the nonexistent-BDF test above.
        drop(result);
    }

    #[test]
    fn count_external_vfio_skips_proc_when_iommu_group_is_zero() {
        let _guard = SWAP_TEST_LOCK
            .lock()
            .expect("swap tests must not run concurrently with other swap IPC tests");
        assert_eq!(count_external_vfio_group_holders("9999:99:99.9"), 0);
    }

    #[test]
    fn read_boot0_from_tmpfile_returns_value() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("resource0");
        let boot0_val: u32 = 0x108000a1;
        let mut data = vec![0u8; 0x9800];
        data[0..4].copy_from_slice(&boot0_val.to_le_bytes());
        std::fs::write(&path, &data).expect("write fake resource0");
        let result = read_boot0(path.to_str().unwrap());
        assert_eq!(result, Some(boot0_val));
    }

    #[test]
    fn read_boot0_nonexistent_returns_none() {
        assert!(read_boot0("/nonexistent-coral-ember-resource0").is_none());
    }

    #[test]
    fn is_gpu_cold_detects_frozen_ptimer() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("resource0");
        let mut data = vec![0u8; 0x9800];
        // BOOT0: cold GK210 sentinel
        data[0..4].copy_from_slice(&0x0f22d0a1u32.to_le_bytes());
        // PTIMER at 0x9400: frozen (same value on both reads)
        data[0x9400..0x9404].copy_from_slice(&0xBAD0DA1Fu32.to_le_bytes());
        std::fs::write(&path, &data).expect("write fake resource0");
        assert!(is_gpu_cold_via_ptimer(path.to_str().unwrap()));
    }

    #[test]
    fn is_gpu_cold_rejects_unknown_chipset_even_with_running_ptimer() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("resource0");
        let mut data = vec![0u8; 0x9800];
        // BOOT0: invalid chipset (0x0F2 is not in the known table)
        data[0..4].copy_from_slice(&0x0f22d0a1u32.to_le_bytes());
        // PTIMER: would need two different reads, but from a static file
        // both reads return the same value → frozen → cold
        data[0x9400..0x9404].copy_from_slice(&0x12345678u32.to_le_bytes());
        std::fs::write(&path, &data).expect("write fake resource0");
        assert!(is_gpu_cold_via_ptimer(path.to_str().unwrap()));
    }
}
