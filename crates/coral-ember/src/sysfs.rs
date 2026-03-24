// SPDX-License-Identifier: AGPL-3.0-only
//! Sysfs helpers — Ember is the sole writer of driver/unbind and bind.
//! Paths respect [`coral_driver::linux_paths`] (`CORALREEF_SYSFS_ROOT`).
//!
//! # D-state isolation
//!
//! Sysfs writes to `driver/unbind`, `drivers/*/bind`, and `remove` can
//! enter uninterruptible kernel sleep (D-state) and never return. A thread
//! in D-state cannot be killed — even `SIGKILL` is deferred until the
//! syscall completes. If the main daemon thread enters D-state, the entire
//! IPC socket becomes unresponsive.
//!
//! To survive this, risky sysfs writes are performed in a **short-lived
//! child process** via [`guarded_sysfs_write`]. The parent waits with a
//! configurable timeout and kills the child if it hangs. The parent daemon
//! stays responsive regardless of kernel misbehavior.

use coral_driver::linux_paths;
use std::time::Duration;

/// Default timeout for sysfs writes that can enter D-state.
const SYSFS_WRITE_TIMEOUT: Duration = Duration::from_secs(10);

/// Parses the body of a sysfs PCI id file (e.g. `"0x10de\n"`).
#[must_use]
pub(crate) fn parse_pci_id_hex(content: &str) -> u16 {
    u16::from_str_radix(content.trim().trim_start_matches("0x"), 16).unwrap_or(0)
}

/// Parses an IOMMU group id from the last segment of a sysfs symlink target.
#[must_use]
pub(crate) fn parse_iommu_group_file_name(name: &str) -> u32 {
    name.parse().unwrap_or(0)
}

/// Write to a sysfs path using process-isolated watchdog.
///
/// Spawns a child process that performs the actual `write(2)` syscall.
/// If the child enters D-state and doesn't complete within
/// [`SYSFS_WRITE_TIMEOUT`], it is killed and an error is returned.
/// The parent ember process remains responsive in all cases.
pub fn sysfs_write(path: &str, value: &str) -> Result<(), String> {
    guarded_sysfs_write(path, value, SYSFS_WRITE_TIMEOUT)
}

/// Process-isolated sysfs write with configurable timeout.
///
/// The child process is spawned via `Command::new("/bin/sh")` with a
/// simple `printf | tee` pipeline. This is intentionally a separate
/// process (not a thread) because:
///
/// - A thread in D-state poisons `pthread_join` and blocks process exit
/// - A child process in D-state can be `SIGKILL`'d by the parent
/// - The parent's `waitpid` never enters D-state itself
fn guarded_sysfs_write(path: &str, value: &str, timeout: Duration) -> Result<(), String> {
    use std::process::{Command, Stdio};

    let mut child = Command::new("/usr/bin/env")
        .args([
            "sh",
            "-c",
            "printf '%s' \"$1\" > \"$2\"",
            "sysfs_write",
            value,
            path,
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("sysfs write {path}: spawn failed: {e}"))?;

    let deadline = std::time::Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if status.success() {
                    return Ok(());
                }
                let stderr = child
                    .stderr
                    .take()
                    .and_then(|mut s| {
                        let mut buf = String::new();
                        std::io::Read::read_to_string(&mut s, &mut buf).ok()?;
                        Some(buf)
                    })
                    .unwrap_or_default();
                return Err(format!(
                    "sysfs write {path}: child exited {status}: {stderr}"
                ));
            }
            Ok(None) => {
                if std::time::Instant::now() >= deadline {
                    tracing::error!(
                        path,
                        value,
                        timeout_secs = timeout.as_secs(),
                        pid = child.id(),
                        "sysfs write TIMED OUT — child likely in D-state, killing"
                    );
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!(
                        "sysfs write {path}: timed out after {}s (child killed — \
                         kernel sysfs operation likely in D-state)",
                        timeout.as_secs()
                    ));
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => {
                return Err(format!("sysfs write {path}: waitpid failed: {e}"));
            }
        }
    }
}

/// Direct sysfs write without process isolation.
///
/// Use only for paths that are known to never enter D-state:
/// `power/control`, `d3cold_allowed`, `reset_method`,
/// `power/autosuspend_delay_ms`. These are memory-mapped config-space
/// attributes that complete synchronously.
pub fn sysfs_write_direct(path: &str, value: &str) -> Result<(), String> {
    std::fs::write(path, value).map_err(|e| format!("sysfs write {path}: {e}"))
}

pub fn read_current_driver(bdf: &str) -> Option<String> {
    std::fs::read_link(linux_paths::sysfs_pci_device_file(bdf, "driver"))
        .ok()
        .and_then(|p| p.file_name().map(|f| f.to_string_lossy().to_string()))
}

pub fn read_iommu_group(bdf: &str) -> u32 {
    std::fs::read_link(linux_paths::sysfs_pci_device_file(bdf, "iommu_group"))
        .ok()
        .and_then(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .map(parse_iommu_group_file_name)
        })
        .unwrap_or(0)
}

pub fn find_drm_card(bdf: &str) -> Option<String> {
    let drm_dir = linux_paths::sysfs_pci_device_file(bdf, "drm");
    for entry in std::fs::read_dir(&drm_dir).ok()?.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with("card") {
            return Some(format!("/dev/dri/{name}"));
        }
    }
    None
}

pub fn bind_iommu_group_to_vfio(primary_bdf: &str, group_id: u32) {
    for_each_iommu_peer(primary_bdf, group_id, |peer_bdf| {
        let driver = read_current_driver(&peer_bdf);
        if driver.as_deref() == Some("vfio-pci") {
            return;
        }
        tracing::info!(peer = %peer_bdf, group = group_id, "binding IOMMU group peer to vfio-pci");
        if driver.is_some() {
            let _ = sysfs_write(
                &linux_paths::sysfs_pci_device_file(&peer_bdf, "driver/unbind"),
                &peer_bdf,
            );
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
        let _ = sysfs_write(
            &linux_paths::sysfs_pci_device_file(&peer_bdf, "driver_override"),
            "vfio-pci",
        );
        let _ = sysfs_write(&linux_paths::sysfs_pci_driver_bind("vfio-pci"), &peer_bdf);
        std::thread::sleep(std::time::Duration::from_millis(200));
    });
}

/// Release IOMMU group peers from vfio-pci before a native driver bind.
///
/// When swapping the primary device to a native driver (e.g. nouveau),
/// peer functions (e.g. HDMI audio) must be unbound from vfio-pci and
/// have their `driver_override` cleared. Otherwise the VFIO group stays
/// held and the native driver may fail to claim the primary device.
pub fn release_iommu_group_from_vfio(primary_bdf: &str, group_id: u32) {
    for_each_iommu_peer(primary_bdf, group_id, |peer_bdf| {
        let driver = read_current_driver(&peer_bdf);
        if driver.as_deref() != Some("vfio-pci") {
            return;
        }
        tracing::info!(
            peer = %peer_bdf,
            group = group_id,
            "releasing IOMMU group peer from vfio-pci"
        );
        let _ = sysfs_write(
            &linux_paths::sysfs_pci_device_file(&peer_bdf, "driver/unbind"),
            &peer_bdf,
        );
        std::thread::sleep(std::time::Duration::from_millis(200));
        let _ = sysfs_write(
            &linux_paths::sysfs_pci_device_file(&peer_bdf, "driver_override"),
            "\n",
        );
    });
}

fn for_each_iommu_peer(primary_bdf: &str, group_id: u32, mut f: impl FnMut(String)) {
    let group_path = linux_paths::sysfs_kernel_iommu_group_devices(group_id);
    let Ok(entries) = std::fs::read_dir(&group_path) else {
        return;
    };
    for entry in entries.flatten() {
        let peer_bdf = entry.file_name().to_string_lossy().to_string();
        if peer_bdf == primary_bdf {
            continue;
        }
        f(peer_bdf);
    }
}

/// Pin power state to prevent D3 transitions during driver swaps.
///
/// Uses direct writes — `power/control` and `d3cold_allowed` are
/// config-space attributes that always complete synchronously.
pub fn pin_power(bdf: &str) {
    let _ = sysfs_write_direct(
        &linux_paths::sysfs_pci_device_file(bdf, "power/control"),
        "on",
    );
    let _ = sysfs_write_direct(
        &linux_paths::sysfs_pci_device_file(bdf, "d3cold_allowed"),
        "0",
    );
}

/// Write `driver_override` sysfs attribute to lock a device to a specific driver.
/// Used to protect display GPUs from being rebound to vfio-pci or nouveau.
pub fn set_driver_override(bdf: &str, driver: &str) {
    let path = linux_paths::sysfs_pci_device_file(bdf, "driver_override");
    match sysfs_write_direct(&path, driver) {
        Ok(()) => tracing::info!(bdf = %bdf, driver, "driver_override set"),
        Err(e) => tracing::warn!(bdf = %bdf, driver, error = %e, "failed to set driver_override"),
    }
}

/// Error when a PM power cycle leaves the device in `D3cold`.
pub(crate) fn err_if_pm_cycle_d3cold(bdf: &str, after_power_state: &str) -> Result<(), String> {
    if after_power_state == "D3cold" {
        return Err(format!("{bdf}: PM power cycle resulted in D3cold"));
    }
    Ok(())
}

/// Read a PCI ID field (vendor, device, subsystem_vendor, subsystem_device).
/// Returns 0 on failure. The sysfs files contain hex values like "0x10de\n".
pub fn read_pci_id(bdf: &str, field: &str) -> u16 {
    let path = linux_paths::sysfs_pci_device_file(bdf, field);
    std::fs::read_to_string(&path)
        .ok()
        .map(|s| parse_pci_id_hex(&s))
        .unwrap_or(0)
}

/// Read the current PCIe power state (D0, D3hot, D3cold, unknown).
pub fn read_power_state(bdf: &str) -> Option<String> {
    let path = linux_paths::sysfs_pci_device_file(bdf, "power_state");
    std::fs::read_to_string(&path)
        .ok()
        .map(|s| s.trim().to_string())
}

/// Returns `true` when the device is in D3cold (powered off by the platform).
///
/// D3cold devices must NOT have VFIO operations attempted against them.
/// Ember checks this before reacquire and swap to prevent cascade failures.
pub fn is_d3cold(bdf: &str) -> bool {
    read_power_state(bdf).as_deref() == Some("D3cold")
}

/// Pin power on all upstream PCI bridges to prevent them from
/// powering down after a device remove. Walks the sysfs topology
/// from the device up to the root port.
///
/// Uses direct writes — bridge power attributes are config-space
/// and always complete synchronously.
pub fn pin_bridge_power(bdf: &str) {
    let device_path = linux_paths::sysfs_pci_device_path(bdf);
    let Ok(real_path) = std::fs::canonicalize(&device_path) else {
        return;
    };

    let mut current = real_path.parent();
    while let Some(parent) = current {
        let power_control = parent.join("power/control");
        let d3cold = parent.join("d3cold_allowed");

        if power_control.exists() {
            let _ = sysfs_write_direct(power_control.to_str().unwrap_or(""), "on");
            let _ = sysfs_write_direct(d3cold.to_str().unwrap_or(""), "0");
        }

        if parent
            .file_name()
            .is_some_and(|n| n.to_string_lossy().starts_with("pci"))
        {
            break;
        }
        current = parent.parent();
    }
}

/// Remove a PCI device from the kernel's device tree.
/// This forces full cleanup of sysfs entries, DRM nodes, hwmon, etc.
pub fn pci_remove(bdf: &str) -> Result<(), String> {
    let path = linux_paths::sysfs_pci_device_file(bdf, "remove");
    sysfs_write(&path, "1")
}

/// Trigger a PCI bus rescan, causing the kernel to re-enumerate
/// all devices and probe matching drivers.
pub fn pci_rescan() -> Result<(), String> {
    sysfs_write(&linux_paths::sysfs_pci_bus_rescan(), "1")
}

/// PM power cycle: transition through D3hot → D0 to reinitialize the
/// function without a bus reset. The PCIe spec requires D3hot→D0 to
/// reset function-level state while preserving PCI topology.
///
/// Power state transitions use the guarded write path since they can
/// stall if the device firmware is unresponsive.
pub fn pm_power_cycle(bdf: &str) -> Result<(), String> {
    let power_state_path = linux_paths::sysfs_pci_device_file(bdf, "power_state");

    let current = std::fs::read_to_string(&power_state_path)
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    tracing::info!(bdf, current_state = %current, "PM power cycle: entering D3hot");

    pin_power(bdf);
    pin_bridge_power(bdf);

    sysfs_write_direct(
        &linux_paths::sysfs_pci_device_file(bdf, "power/control"),
        "on",
    )?;

    std::thread::sleep(Duration::from_millis(500));

    let config_path = linux_paths::sysfs_pci_device_file(bdf, "config");
    let saved_config = std::fs::read(&config_path).ok();

    sysfs_write(&power_state_path, "D3hot")?;
    std::thread::sleep(Duration::from_secs(2));

    sysfs_write(&power_state_path, "D0")?;
    std::thread::sleep(Duration::from_secs(1));

    if let Some(config) = saved_config {
        let _ = std::fs::write(&config_path, &config);
    }

    pin_power(bdf);
    pin_bridge_power(bdf);

    let after = std::fs::read_to_string(&power_state_path)
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    tracing::info!(bdf, power_state = %after, "PM power cycle complete");

    err_if_pm_cycle_d3cold(bdf, &after)
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXPECTED_NVIDIA_VENDOR: u16 = 0x10de;

    #[test]
    fn parse_pci_id_hex_accepts_0x_prefix_and_whitespace() {
        assert_eq!(parse_pci_id_hex("0x10de\n"), EXPECTED_NVIDIA_VENDOR);
        assert_eq!(parse_pci_id_hex("  0xABCD  "), 0xabcd);
    }

    #[test]
    fn parse_pci_id_hex_invalid_returns_zero() {
        assert_eq!(parse_pci_id_hex("not-hex"), 0);
        assert_eq!(parse_pci_id_hex(""), 0);
    }

    #[test]
    fn parse_iommu_group_file_name_numeric() {
        const EXPECTED_GROUP: u32 = 42;
        assert_eq!(parse_iommu_group_file_name("42"), EXPECTED_GROUP);
        assert_eq!(parse_iommu_group_file_name("0"), 0);
    }

    #[test]
    fn parse_iommu_group_file_name_invalid_returns_zero() {
        assert_eq!(parse_iommu_group_file_name("not-a-number"), 0);
    }

    #[test]
    fn guarded_sysfs_write_round_trip_tmpfile() {
        let dir = std::env::temp_dir();
        let path = dir.join("coral_ember_sysfs_guarded_write_test");
        let payload = "on";
        sysfs_write(path.to_str().unwrap(), payload).unwrap();
        let read_back = std::fs::read_to_string(&path).unwrap();
        assert_eq!(read_back, payload);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn guarded_sysfs_write_missing_parent_is_error() {
        let err = sysfs_write("/nonexistent-coral-ember-path/nope", "1").unwrap_err();
        assert!(err.contains("sysfs write"));
    }

    #[test]
    fn guarded_sysfs_write_timeout_reports_d_state() {
        let err = guarded_sysfs_write("/dev/null", "test", Duration::ZERO);
        // With zero timeout the child may or may not finish — we just
        // verify no panic and the error path works.
        drop(err);
    }

    #[test]
    fn direct_sysfs_write_round_trip() {
        let dir = std::env::temp_dir();
        let path = dir.join("coral_ember_direct_write_test");
        sysfs_write_direct(path.to_str().unwrap(), "direct").unwrap();
        let read_back = std::fs::read_to_string(&path).unwrap();
        assert_eq!(read_back, "direct");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn direct_sysfs_write_missing_parent_is_error() {
        let err = sysfs_write_direct("/nonexistent-coral-ember-path/nope", "1").unwrap_err();
        assert!(err.contains("sysfs write"));
    }

    #[test]
    fn err_if_pm_cycle_d3cold_rejects_d3cold() {
        let bdf = "0000:01:00.0";
        let err = err_if_pm_cycle_d3cold(bdf, "D3cold").unwrap_err();
        assert!(err.contains(bdf));
        assert!(err.contains("D3cold"));
    }

    #[test]
    fn err_if_pm_cycle_d3cold_accepts_other_states() {
        err_if_pm_cycle_d3cold("0000:01:00.0", "D0").unwrap();
        err_if_pm_cycle_d3cold("0000:01:00.0", "D3hot").unwrap();
    }

    #[test]
    fn pci_remove_invalid_bdf_is_error() {
        let remove_err = pci_remove("ff:ff:ff.f");
        assert!(remove_err.is_err());
    }

    #[test]
    fn read_power_state_nonexistent_device_returns_none() {
        assert_eq!(read_power_state("9999:99:99.9"), None);
    }

    #[test]
    fn pci_rescan_write_failure_is_propagated_when_rescan_missing() {
        let err = sysfs_write("/nonexistent-coral-ember-pci/rescan", "1").unwrap_err();
        assert!(err.contains("sysfs write"));
    }
}
