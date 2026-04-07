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

use crate::error::SysfsError;
use coral_driver::linux_paths;
use std::time::Duration;

/// Run `setpci` with a timeout to prevent ember from hanging if ECAM is stuck.
///
/// Returns the child's stdout on success, or an error if the command fails
/// or times out. If the child hangs (ECAM access stuck), it is killed after
/// `timeout` and an error is returned — ember stays alive.
fn setpci_with_timeout(
    args: &[&str],
    timeout: Duration,
) -> Result<String, String> {
    use std::process::{Command, Stdio};

    let mut child = Command::new("setpci")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("setpci spawn: {e}"))?;

    let deadline = std::time::Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if status.success() {
                    let stdout = child.stdout.take().map(|mut s| {
                        let mut buf = String::new();
                        let _ = std::io::Read::read_to_string(&mut s, &mut buf);
                        buf
                    }).unwrap_or_default();
                    return Ok(stdout);
                }
                let stderr = child.stderr.take().map(|mut s| {
                    let mut buf = String::new();
                    let _ = std::io::Read::read_to_string(&mut s, &mut buf);
                    buf
                }).unwrap_or_default();
                return Err(format!("setpci exited {status}: {stderr}"));
            }
            Ok(None) => {
                if std::time::Instant::now() >= deadline {
                    tracing::error!(
                        ?args,
                        timeout_ms = timeout.as_millis(),
                        pid = child.id(),
                        "setpci TIMED OUT — ECAM may be stuck, killing"
                    );
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!(
                        "setpci timed out after {}ms (killed)",
                        timeout.as_millis()
                    ));
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => return Err(format!("setpci waitpid: {e}")),
        }
    }
}

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
pub fn sysfs_write(path: &str, value: &str) -> Result<(), SysfsError> {
    guarded_sysfs_write(path, value, SYSFS_WRITE_TIMEOUT)
}

/// Process-isolated sysfs write with configurable timeout.
///
/// The child process is spawned via `/usr/bin/env sh -c` with a
/// simple `printf | tee` pipeline. This is intentionally a separate
/// process (not a thread) because:
///
/// - A thread in D-state poisons `pthread_join` and blocks process exit
/// - A child process in D-state can be `SIGKILL`'d by the parent
/// - The parent's `waitpid` never enters D-state itself
fn guarded_sysfs_write(path: &str, value: &str, timeout: Duration) -> Result<(), SysfsError> {
    use std::process::{Command, Stdio};

    // `/usr/bin/env` is the conventional FHS location for the `env(1)` utility.
    // It invokes the named program (`sh`) with a controlled argv; using it
    // avoids hardcoding `/bin/sh` vs `/usr/bin/sh` and matches common POSIX usage.
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
        .map_err(|e| SysfsError::Write {
            path: path.to_string(),
            reason: format!("spawn failed: {e}"),
        })?;

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
                return Err(SysfsError::Write {
                    path: path.to_string(),
                    reason: format!("child exited {status}: {stderr}"),
                });
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
                    // D-state processes ignore SIGKILL until the kernel syscall
                    // unblocks. A blocking wait() here would hang this thread
                    // indefinitely. Brief try_wait loop, then abandon the zombie
                    // — it is reaped when the D-state eventually resolves.
                    let reaped = (0..10).any(|_| match child.try_wait() {
                        Ok(Some(_)) | Err(_) => true,
                        Ok(None) => {
                            std::thread::sleep(Duration::from_millis(100));
                            false
                        }
                    });
                    if !reaped {
                        tracing::warn!(
                            path,
                            pid = child.id(),
                            "sysfs write child still in D-state after kill — \
                             abandoning zombie (will be reaped when kernel unblocks)"
                        );
                    }
                    return Err(SysfsError::Write {
                        path: path.to_string(),
                        reason: format!(
                            "timed out after {}s (child killed — kernel sysfs operation likely in D-state)",
                            timeout.as_secs()
                        ),
                    });
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => {
                return Err(SysfsError::Write {
                    path: path.to_string(),
                    reason: format!("waitpid failed: {e}"),
                });
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
///
/// Sysfs caveat: the kernel ignores 0-byte writes (the store function
/// is never invoked). When `value` is empty we write `"\n"` instead,
/// which the kernel's `sysfs_streq` treats as an empty string. Without
/// this, writes like `reset_method = ""` silently do nothing and the
/// device retains its previous reset method.
pub fn sysfs_write_direct(path: &str, value: &str) -> Result<(), SysfsError> {
    let bytes: &[u8] = if value.is_empty() {
        b"\n"
    } else {
        value.as_bytes()
    };
    std::fs::write(path, bytes).map_err(|e| SysfsError::Write {
        path: path.to_string(),
        reason: e.to_string(),
    })
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

/// Set aggressive PCIe completion timeouts for a device and its root port.
///
/// On reboot, PCIe timeouts reset to defaults (50ms–4s). If a BAR0 read
/// hits an unresponsive engine (e.g., PRAMIN on a cold GPU), the CPU blocks
/// for the full timeout. With a 4s default, the NMI watchdog fires and the
/// system locks up. Setting short timeouts (50µs–10ms) ensures the CPU gets
/// 0xFFFFFFFF quickly instead of hanging.
///
/// Uses `setpci` to write Device Control 2 (CAP_EXP+0x28):
///   bits[3:0] = completion timeout value
///   0x1 = 50µs–100µs, 0x2 = 1ms–10ms
pub fn harden_pcie_timeouts(bdf: &str) {
    let set = |target: &str, value: &str| {
        let arg = format!("CAP_EXP+0x28.W={value}");
        match setpci_with_timeout(&["-s", target, &arg], Duration::from_secs(5)) {
            Ok(_) => tracing::info!(bdf = target, value, "PCIe completion timeout hardened"),
            Err(e) => tracing::warn!(bdf = target, value, error = %e, "setpci timeout/failed"),
        }
    };

    // Device: 50µs–100µs timeout (fastest)
    set(bdf, "0001");

    // Parent root port: 1ms–10ms (slightly longer to avoid false positives)
    if let Some(bridge) = find_parent_bridge(bdf) {
        set(&bridge, "0002");

        // Disable DPC (Downstream Port Containment) on the bridge.
        // DPC escalates PCIe completion timeouts into full link teardowns,
        // making all subsequent MMIO hang indefinitely. With DPC disabled,
        // errors are logged via AER but the link stays up — reads return
        // 0xFFFF_FFFF and software can handle the failure gracefully.
        disable_dpc(&bridge);

        // Disable AER error reporting on the bridge. When a GPU MMIO
        // operation triggers a PCIe error, the kernel's AER handler tries
        // to read the device's AER registers through the downstream link.
        // If that link is stuck (flow-control stall), the AER handler
        // enters D-state and cascades to a full system lockup. Disabling
        // AER reporting at the bridge prevents this cascade entirely.
        disable_bridge_aer(&bridge);
    }

    // Disable reset_method to prevent vfio from triggering resets
    let reset_path = linux_paths::sysfs_pci_device_file(bdf, "reset_method");
    let _ = sysfs_write_direct(&reset_path, "");

    // Raise NMI watchdog threshold so our 5s MMIO watchdog + 10s bus reset
    // have room to complete without triggering a hard system lockup.
    harden_nmi_watchdog();
}

/// Disable DPC (Downstream Port Containment) on a PCIe root port.
///
/// DPC is a PCIe feature that takes the link down when a completion timeout or
/// other uncorrectable error is detected. While useful in production, it's
/// catastrophic for GPU experimentation: a single bad MMIO read escalates into
/// a full link teardown, making every subsequent operation hang indefinitely
/// (the CPU blocks waiting for a completion that will never arrive because
/// the link is down).
///
/// With DPC disabled, errors are still logged via AER but the link stays up.
/// MMIO reads to a faulted device return `0xFFFF_FFFF` and writes silently
/// fail — both are detectable and recoverable by software.
///
/// Reads the bridge's PCIe extended capability list to find the DPC capability
/// (ID `0x001D`), then clears the trigger-enable bits in its control register.
pub fn disable_dpc(bridge_bdf: &str) {
    let config_path = linux_paths::sysfs_pci_device_file(bridge_bdf, "config");

    let config = match std::fs::read(&config_path) {
        Ok(data) => data,
        Err(e) => {
            tracing::warn!(bdf = bridge_bdf, error = %e, "cannot read PCI config — skipping DPC disable");
            return;
        }
    };

    if config.len() < 0x104 {
        tracing::debug!(bdf = bridge_bdf, "config space too small for extended caps");
        return;
    }

    const DPC_CAP_ID: u16 = 0x001D;
    let mut offset = 0x100u16;

    loop {
        if offset == 0 || (offset as usize) + 4 > config.len() {
            break;
        }

        let header = u32::from_le_bytes([
            config[offset as usize],
            config[offset as usize + 1],
            config[offset as usize + 2],
            config[offset as usize + 3],
        ]);

        let cap_id = (header & 0xFFFF) as u16;
        let next_offset = ((header >> 20) & 0xFFC) as u16;

        if cap_id == DPC_CAP_ID {
            let ctrl_offset = offset + 6;

            if (ctrl_offset as usize) + 2 > config.len() {
                tracing::warn!(bdf = bridge_bdf, "DPC capability found but control register out of bounds");
                return;
            }

            let current_ctrl = u16::from_le_bytes([
                config[ctrl_offset as usize],
                config[ctrl_offset as usize + 1],
            ]);

            if current_ctrl & 0x03 == 0 {
                tracing::info!(bdf = bridge_bdf, "DPC already disabled on root port");
                return;
            }

            // Clear trigger-enable (bits 0-1) and interrupt-enable (bit 3)
            let new_ctrl = current_ctrl & !0x000B;
            let arg = format!("{ctrl_offset:#x}.W={new_ctrl:04x}");

            match setpci_with_timeout(&["-s", bridge_bdf, &arg], Duration::from_secs(5)) {
                Ok(_) => {
                    tracing::info!(
                        bdf = bridge_bdf,
                        offset = ctrl_offset,
                        old_ctrl = format_args!("{current_ctrl:#06x}"),
                        new_ctrl = format_args!("{new_ctrl:#06x}"),
                        "DPC DISABLED — PCIe errors logged via AER without link teardown"
                    );
                }
                Err(e) => {
                    tracing::warn!(bdf = bridge_bdf, error = %e, "setpci DPC disable failed/timed out");
                }
            }
            return;
        }

        offset = next_offset;
    }

    tracing::debug!(bdf = bridge_bdf, "DPC capability not found on this bridge");
}

/// Disable AER error reporting on a bridge permanently (at startup).
///
/// Clears bits 0-2 (CERptEn, NFERptEn, FERptEn) in the AER Root Command
/// register. This prevents the kernel's AER handler from chasing errors on
/// downstream devices — if the device link is stuck, the AER handler would
/// enter D-state trying to read AER registers through the dead link.
fn disable_bridge_aer(bridge_bdf: &str) {
    let config_path = linux_paths::sysfs_pci_device_file(bridge_bdf, "config");
    let config = match std::fs::read(&config_path) {
        Ok(d) => d,
        Err(e) => {
            tracing::warn!(bdf = bridge_bdf, error = %e, "cannot read config for AER disable");
            return;
        }
    };

    let Some(aer_base) = find_ext_cap(&config, AER_CAP_ID) else {
        tracing::debug!(bdf = bridge_bdf, "AER capability not found");
        return;
    };
    let root_cmd_off = aer_base + AER_ROOT_CMD_OFFSET;
    if (root_cmd_off as usize) + 4 > config.len() {
        return;
    }

    let current = u32::from_le_bytes([
        config[root_cmd_off as usize],
        config[root_cmd_off as usize + 1],
        config[root_cmd_off as usize + 2],
        config[root_cmd_off as usize + 3],
    ]);

    if current & AER_ROOT_CMD_REPORT_MASK == 0 {
        tracing::info!(bdf = bridge_bdf, "AER reporting already disabled on bridge");
        return;
    }

    let masked = current & !AER_ROOT_CMD_REPORT_MASK;
    let arg = format!("{root_cmd_off:#x}.L={masked:08x}");
    match setpci_with_timeout(&["-s", bridge_bdf, &arg], Duration::from_secs(5)) {
        Ok(_) => {
            tracing::info!(
                bdf = bridge_bdf,
                old = format_args!("{current:#010x}"),
                new = format_args!("{masked:#010x}"),
                "AER DISABLED on bridge — kernel error handler cannot cascade on stuck device"
            );
        }
        Err(e) => {
            tracing::warn!(bdf = bridge_bdf, error = %e, "setpci AER disable failed/timed out");
        }
    }
}

// ─── AER masking for fork-isolated operations ──────────────────────────────

const AER_CAP_ID: u16 = 0x0001;
/// AER Root Command register offset within the AER extended capability.
const AER_ROOT_CMD_OFFSET: u16 = 0x2C;
/// Bits 0-2: CERptEn, NFERptEn, FERptEn
const AER_ROOT_CMD_REPORT_MASK: u32 = 0x07;

/// Find the offset of an extended capability in PCI config space.
fn find_ext_cap(config: &[u8], cap_id: u16) -> Option<u16> {
    let mut offset = 0x100u16;
    loop {
        if offset == 0 || (offset as usize) + 4 > config.len() {
            return None;
        }
        let header = u32::from_le_bytes([
            config[offset as usize],
            config[offset as usize + 1],
            config[offset as usize + 2],
            config[offset as usize + 3],
        ]);
        if (header & 0xFFFF) as u16 == cap_id {
            return Some(offset);
        }
        let next = ((header >> 20) & 0xFFC) as u16;
        if next == offset {
            return None;
        }
        offset = next;
    }
}

/// Disable AER error reporting on the parent bridge so the kernel's AER
/// handler cannot try to access a hung device's config space (which would
/// cascade into a kernel D-state lockup).
///
/// Returns the previous AER Root Command value so it can be restored later
/// via [`unmask_bridge_aer`].
pub fn mask_bridge_aer(bdf: &str) -> Option<(String, u32)> {
    let bridge_bdf = find_parent_bridge(bdf)?;
    let config_path = linux_paths::sysfs_pci_device_file(&bridge_bdf, "config");
    let config = std::fs::read(&config_path).ok()?;

    let aer_base = find_ext_cap(&config, AER_CAP_ID)?;
    let root_cmd_off = aer_base + AER_ROOT_CMD_OFFSET;

    if (root_cmd_off as usize) + 4 > config.len() {
        tracing::warn!(bdf, bridge = %bridge_bdf, "AER Root Command register out of bounds");
        return None;
    }

    let current = u32::from_le_bytes([
        config[root_cmd_off as usize],
        config[root_cmd_off as usize + 1],
        config[root_cmd_off as usize + 2],
        config[root_cmd_off as usize + 3],
    ]);

    if current & AER_ROOT_CMD_REPORT_MASK == 0 {
        tracing::debug!(bdf, bridge = %bridge_bdf, "AER already masked");
        return Some((bridge_bdf, current));
    }

    let masked = current & !AER_ROOT_CMD_REPORT_MASK;
    let arg = format!("{root_cmd_off:#x}.L={masked:08x}");
    match setpci_with_timeout(&["-s", &bridge_bdf, &arg], Duration::from_secs(5)) {
        Ok(_) => {
            tracing::info!(
                bdf,
                bridge = %bridge_bdf,
                old = format_args!("{current:#010x}"),
                new = format_args!("{masked:#010x}"),
                "AER MASKED on bridge — kernel error handler will not chase hung device"
            );
        }
        Err(e) => {
            tracing::warn!(bdf, bridge = %bridge_bdf, error = %e, "setpci AER mask failed/timed out");
        }
    }

    Some((bridge_bdf, current))
}

/// Re-enable AER error reporting on a bridge after a fork-isolated operation
/// completes. Pass the (bridge_bdf, original_value) returned by [`mask_bridge_aer`].
pub fn unmask_bridge_aer(bridge_bdf: &str, original_root_cmd: u32) {
    let config_path = linux_paths::sysfs_pci_device_file(bridge_bdf, "config");
    let config = match std::fs::read(&config_path) {
        Ok(d) => d,
        Err(e) => {
            tracing::warn!(bridge = %bridge_bdf, error = %e, "cannot read config for AER unmask");
            return;
        }
    };

    let Some(aer_base) = find_ext_cap(&config, AER_CAP_ID) else {
        return;
    };
    let root_cmd_off = aer_base + AER_ROOT_CMD_OFFSET;

    let arg = format!("{root_cmd_off:#x}.L={original_root_cmd:08x}");
    match setpci_with_timeout(&["-s", bridge_bdf, &arg], Duration::from_secs(5)) {
        Ok(_) => {
            tracing::info!(
                bridge = %bridge_bdf,
                value = format_args!("{original_root_cmd:#010x}"),
                "AER UNMASKED — bridge error reporting restored"
            );
        }
        Err(e) => {
            tracing::warn!(bridge = %bridge_bdf, error = %e, "setpci AER unmask failed/timed out");
        }
    }
}

// ─── Raw Secondary Bus Reset (bypasses kernel pci_save_state) ──────────────

/// Trigger a PCIe Secondary Bus Reset (SBR) by directly toggling bit 6 of
/// `PCI_BRIDGE_CONTROL` (config offset 0x3E) via direct sysfs config write.
///
/// Uses direct `read(2)`/`write(2)` on the bridge's sysfs `config` file
/// instead of spawning `setpci`. This is critical in the fork-isolation
/// timeout handler: if the ECAM path is sluggish (AMD IOHUB processing a
/// PCIe error), spawning `setpci` with blocking `.output()` would hang
/// ember indefinitely. Direct fd writes are faster and can be wrapped in
/// a fork-isolated child of their own if needed.
pub fn raw_bridge_sbr(bdf: &str) -> Result<(), SysfsError> {
    let bridge_bdf = find_parent_bridge(bdf).ok_or_else(|| SysfsError::BridgeNotFound {
        bdf: bdf.to_string(),
    })?;

    raw_bridge_sbr_direct(bdf, &bridge_bdf)
}

/// Direct config-space SBR implementation using fd read/write.
///
/// Opens the bridge's sysfs `config` file and reads/writes PCI_BRIDGE_CONTROL
/// at offset 0x3E. No process spawning, no blocking `.output()`.
fn raw_bridge_sbr_direct(bdf: &str, bridge_bdf: &str) -> Result<(), SysfsError> {
    use std::io::{Read, Seek, SeekFrom, Write};

    const PCI_BRIDGE_CONTROL_OFFSET: u64 = 0x3E;

    let config_path = linux_paths::sysfs_pci_device_file(bridge_bdf, "config");
    let mut config = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&config_path)
        .map_err(|e| SysfsError::Write {
            path: config_path.clone(),
            reason: format!("open bridge config for SBR: {e}"),
        })?;

    config
        .seek(SeekFrom::Start(PCI_BRIDGE_CONTROL_OFFSET))
        .map_err(|e| SysfsError::Write {
            path: config_path.clone(),
            reason: format!("seek to PCI_BRIDGE_CONTROL: {e}"),
        })?;
    let mut ctrl_bytes = [0u8; 2];
    config.read_exact(&mut ctrl_bytes).map_err(|e| SysfsError::Write {
        path: config_path.clone(),
        reason: format!("read PCI_BRIDGE_CONTROL: {e}"),
    })?;
    let ctrl = u16::from_le_bytes(ctrl_bytes);

    tracing::info!(
        bdf,
        bridge = %bridge_bdf,
        ctrl = format_args!("{ctrl:#06x}"),
        "raw SBR: asserting Secondary Bus Reset via direct config write"
    );

    // Assert SBR (bit 6)
    config
        .seek(SeekFrom::Start(PCI_BRIDGE_CONTROL_OFFSET))
        .map_err(|e| SysfsError::Write {
            path: config_path.clone(),
            reason: format!("seek for SBR assert: {e}"),
        })?;
    config
        .write_all(&(ctrl | 0x0040).to_le_bytes())
        .map_err(|e| SysfsError::Write {
            path: config_path.clone(),
            reason: format!("write SBR assert: {e}"),
        })?;

    // Hold reset for 100ms (PCI spec minimum is 1ms, extra margin for safety)
    std::thread::sleep(Duration::from_millis(100));

    // De-assert SBR
    config
        .seek(SeekFrom::Start(PCI_BRIDGE_CONTROL_OFFSET))
        .map_err(|e| SysfsError::Write {
            path: config_path.clone(),
            reason: format!("seek for SBR de-assert: {e}"),
        })?;
    config
        .write_all(&(ctrl & !0x0040).to_le_bytes())
        .map_err(|e| SysfsError::Write {
            path: config_path.clone(),
            reason: format!("write SBR de-assert: {e}"),
        })?;

    // Wait for link re-training
    std::thread::sleep(Duration::from_millis(500));

    pin_power(bdf);
    pin_bridge_power(bdf);

    tracing::info!(bdf, bridge = %bridge_bdf, "raw SBR complete — link should be re-trained");
    Ok(())
}

/// Trigger SBR via legacy PCI I/O ports (CF8/CFC).
///
/// This bypasses ECAM (memory-mapped config space) entirely. On AMD Zen,
/// ECAM accesses go through the same PCIe root complex as data TLPs. When
/// a downstream device stalls PCIe flow-control credits, ECAM accesses to
/// the bridge also stall — making the normal sysfs/ECAM SBR path useless.
///
/// Legacy I/O port config accesses (CF8/CFC) are routed through the CPU's
/// I/O bus → data fabric → NBIO config aperture, which is a separate path
/// from the PCIe transaction layer. This works even when the root complex's
/// posted-write credit pool is completely exhausted.
///
/// Requires CAP_SYS_RAWIO (or root) for `ioperm(2)`.
///
/// `bridge_bdf` must be on bus 0 (legacy PCI config only covers bus 0
/// directly; buses > 0 require Type 1 config cycles which still go through
/// the root complex). For devices behind a root port on bus 0, this works.
#[allow(unsafe_code)]
pub fn io_port_sbr(bridge_bdf: &str) -> Result<(), SysfsError> {
    let parts: Vec<&str> = bridge_bdf.split(':').collect();
    if parts.len() < 3 {
        return Err(SysfsError::BridgeNotFound { bdf: bridge_bdf.to_string() });
    }
    let domain_bus: Vec<&str> = if parts.len() == 3 {
        // "0000:00:01.3" → domain="0000", bus="00", devfn="01.3"
        vec![parts[0], parts[1], parts[2]]
    } else {
        return Err(SysfsError::BridgeNotFound { bdf: bridge_bdf.to_string() });
    };

    let bus = u8::from_str_radix(domain_bus[1], 16).map_err(|_| SysfsError::BridgeNotFound {
        bdf: bridge_bdf.to_string(),
    })?;
    if bus != 0 {
        return Err(SysfsError::BridgeResetMissing {
            bdf: bridge_bdf.to_string(),
            bridge_bdf: format!("bus {bus} != 0, I/O port SBR only works for bus 0"),
        });
    }

    let devfn_parts: Vec<&str> = domain_bus[2].split('.').collect();
    if devfn_parts.len() != 2 {
        return Err(SysfsError::BridgeNotFound { bdf: bridge_bdf.to_string() });
    }
    let dev = u8::from_str_radix(devfn_parts[0], 16).map_err(|_| SysfsError::BridgeNotFound {
        bdf: bridge_bdf.to_string(),
    })?;
    let func = u8::from_str_radix(devfn_parts[1], 16).map_err(|_| SysfsError::BridgeNotFound {
        bdf: bridge_bdf.to_string(),
    })?;

    // CONFIG_ADDRESS: bit31=enable | bus<<16 | dev<<11 | func<<8 | offset
    // Bridge Control is at offset 0x3E. Dword-aligned = 0x3C.
    let config_addr: u32 = 0x8000_0000
        | (u32::from(dev) << 11)
        | (u32::from(func) << 8)
        | 0x3C;

    // SAFETY: ioperm grants access to I/O ports 0xCF8-0xCFF for this process.
    // Requires CAP_SYS_RAWIO. We release permission at the end.
    if unsafe { libc::ioperm(0xCF8, 8, 1) } != 0 {
        return Err(SysfsError::BridgeResetMissing {
            bdf: bridge_bdf.to_string(),
            bridge_bdf: "ioperm(CF8) failed — need CAP_SYS_RAWIO".to_string(),
        });
    }

    // Read current dword at offset 0x3C (contains Bridge Control at bytes 2-3)
    io_outl(0xCF8, config_addr);
    let dword = io_inl(0xCFC);

    // Assert SBR (bit 6 of Bridge Control = bit 22 of the dword)
    io_outl(0xCF8, config_addr);
    io_outl(0xCFC, dword | (1 << 22));

    std::thread::sleep(Duration::from_millis(100));

    // De-assert SBR
    io_outl(0xCF8, config_addr);
    io_outl(0xCFC, dword & !(1 << 22));

    std::thread::sleep(Duration::from_millis(500));

    unsafe { libc::ioperm(0xCF8, 8, 0); }

    Ok(())
}

#[cfg(target_arch = "x86_64")]
#[allow(unsafe_code)]
fn io_outl(port: u16, val: u32) {
    unsafe {
        std::arch::asm!("out dx, eax", in("dx") port, in("eax") val, options(nomem, nostack));
    }
}

#[cfg(target_arch = "x86_64")]
#[allow(unsafe_code)]
fn io_inl(port: u16) -> u32 {
    let val: u32;
    unsafe {
        std::arch::asm!("in eax, dx", out("eax") val, in("dx") port, options(nomem, nostack));
    }
    val
}

#[cfg(not(target_arch = "x86_64"))]
fn io_outl(_port: u16, _val: u32) {}

#[cfg(not(target_arch = "x86_64"))]
fn io_inl(_port: u16) -> u32 { 0xFFFF_FFFF }

/// Raise the kernel NMI watchdog threshold to prevent hard lockups during
/// PCIe error recovery.
///
/// The NMI watchdog fires when a CPU core is stuck in kernel mode for
/// `watchdog_thresh` seconds (default 10). During GPU recovery (bus reset
/// + MMIO retry), a core can appear stuck for several seconds. Raising the
/// threshold to 60s gives our 5s MMIO watchdog + bus reset ample headroom.
fn harden_nmi_watchdog() {
    match std::fs::write("/proc/sys/kernel/watchdog_thresh", "60") {
        Ok(()) => tracing::info!("NMI watchdog threshold raised to 60s"),
        Err(e) => {
            tracing::debug!(error = %e, "cannot set NMI watchdog threshold (may need CAP_SYS_PTRACE)");
        }
    }
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
pub(crate) fn err_if_pm_cycle_d3cold(bdf: &str, after_power_state: &str) -> Result<(), SysfsError> {
    if after_power_state == "D3cold" {
        return Err(SysfsError::PmCycleD3cold {
            bdf: bdf.to_string(),
        });
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
/// Read PCI power state with a 2-second timeout.
///
/// On a wedged GPU the sysfs read can D-state the calling thread
/// indefinitely. The timeout-guarded variant spawns a thread and
/// abandons it if it doesn't return in time.
pub fn read_power_state(bdf: &str) -> Option<String> {
    let path = linux_paths::sysfs_pci_device_file(bdf, "power_state");
    guarded_sysfs_read(&path, Duration::from_secs(2))
}

/// Returns `true` when the device is in D3cold (powered off by the platform).
///
/// D3cold devices must NOT have VFIO operations attempted against them.
/// Ember checks this before reacquire and swap to prevent cascade failures.
/// Uses a timeout-guarded sysfs read to prevent D-state stalls.
pub fn is_d3cold(bdf: &str) -> bool {
    read_power_state(bdf).as_deref() == Some("D3cold")
}

/// Timeout-guarded sysfs read. Spawns a thread for the blocking I/O and
/// abandons it if it doesn't return within `timeout`. This prevents a
/// D-stated sysfs node from stalling the calling thread.
fn guarded_sysfs_read(path: &str, timeout: Duration) -> Option<String> {
    let path_owned = path.to_string();
    let (tx, rx) = std::sync::mpsc::channel();
    let _ = std::thread::Builder::new()
        .name("sysfs-read-guard".into())
        .spawn(move || {
            let result = std::fs::read_to_string(&path_owned)
                .ok()
                .map(|s| s.trim().to_string());
            let _ = tx.send(result);
        });
    match rx.recv_timeout(timeout) {
        Ok(result) => result,
        Err(_) => {
            tracing::warn!(path, "guarded sysfs read TIMED OUT — sysfs may be D-stated");
            None
        }
    }
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

/// Trigger a PCI device reset via the sysfs `reset` file.
///
/// Writes `1` to `/sys/bus/pci/devices/<BDF>/reset`, which triggers
/// a Secondary Bus Reset (SBR) or FLR depending on what the kernel
/// negotiates. Unlike VFIO's `VFIO_DEVICE_RESET` (which requires an
/// open VFIO fd and FLR capability), this path works for any PCI
/// device the kernel can reach — including GV100 Titan V which lacks
/// FLR but supports SBR.
///
/// Uses the guarded write path because a reset on a hung device can
/// stall the writing thread indefinitely.
pub fn pci_device_reset(bdf: &str) -> Result<(), SysfsError> {
    let path = linux_paths::sysfs_pci_device_file(bdf, "reset");
    tracing::info!(bdf, path = %path, "triggering PCI device reset via sysfs");
    sysfs_write(&path, "1")
}

/// Discover the parent PCI bridge for a device by walking sysfs.
///
/// Returns the BDF of the parent bridge (e.g. `0000:00:01.3` for a device
/// at `0000:03:00.0`). Returns `None` if the topology cannot be resolved.
pub fn find_parent_bridge(bdf: &str) -> Option<String> {
    let device_path = linux_paths::sysfs_pci_device_path(bdf);
    let real_path = std::fs::canonicalize(&device_path).ok()?;
    let parent = real_path.parent()?;
    let parent_name = parent.file_name()?.to_str()?;

    // Parent directory should be a PCI BDF like "0000:00:01.3"
    if parent_name.contains(':') && parent_name.contains('.') {
        tracing::debug!(bdf, bridge = parent_name, "found parent PCI bridge");
        Some(parent_name.to_string())
    } else {
        tracing::debug!(bdf, parent = parent_name, "parent is not a PCI bridge");
        None
    }
}

/// Reset a device via its parent PCI bridge's `reset` file (bridge-level SBR).
///
/// This is the correct reset mechanism for hardware that lacks FLR (like GV100).
/// Writing to the bridge's reset triggers a Secondary Bus Reset that affects all
/// devices behind the bridge. This works even when the device is VFIO-bound,
/// unlike the device-level `reset` file which often fails with I/O errors on
/// FLR-incapable hardware.
pub fn pci_bridge_reset(bdf: &str) -> Result<(), SysfsError> {
    let bridge_bdf = find_parent_bridge(bdf).ok_or_else(|| SysfsError::BridgeNotFound {
        bdf: bdf.to_string(),
    })?;

    let bridge_reset = linux_paths::sysfs_pci_device_file(&bridge_bdf, "reset");
    if !std::path::Path::new(&bridge_reset).exists() {
        return Err(SysfsError::BridgeResetMissing {
            bdf: bdf.to_string(),
            bridge_bdf,
        });
    }

    tracing::info!(
        bdf,
        bridge = %bridge_bdf,
        path = %bridge_reset,
        "triggering bridge-level SBR"
    );
    sysfs_write(&bridge_reset, "1")?;

    // Brief settle after bridge reset — device needs time to re-enumerate
    std::thread::sleep(Duration::from_millis(500));

    // Re-pin power after reset (bridge reset can change power state)
    pin_power(bdf);
    pin_bridge_power(bdf);

    tracing::info!(bdf, bridge = %bridge_bdf, "bridge-level SBR complete");
    Ok(())
}

/// Full PCI remove + bus rescan cycle. This is the most aggressive reset
/// available: it tears down the kernel's entire device tree entry and
/// forces full re-enumeration and driver re-probe on rescan.
///
/// Used as a fallback when both device-level and bridge-level resets fail.
/// WARNING: The device will be absent from sysfs between remove and rescan.
/// VFIO fds become invalid and must be reacquired after rescan.
pub fn pci_remove_rescan(bdf: &str) -> Result<(), SysfsError> {
    pci_remove_rescan_targeted(bdf, None)
}

/// PCI remove + rescan with an optional target driver override.
///
/// When `target_driver` is `Some`, the kernel's `drivers_autoprobe` is
/// disabled before rescan, `driver_override` is set on the reappeared
/// device, and a manual `drivers_probe` triggers binding. This prevents
/// the kernel's `vfio-pci.ids` cmdline parameter (or any other built-in
/// match table) from reclaiming the device during rescan.
pub fn pci_remove_rescan_targeted(
    bdf: &str,
    target_driver: Option<&str>,
) -> Result<(), SysfsError> {
    pin_bridge_power(bdf);
    pin_power(bdf);

    let _ = sysfs_write_direct(&linux_paths::sysfs_pci_device_file(bdf, "reset_method"), "");

    // When targeting a specific driver, disable autoprobe so the kernel
    // does not match vfio-pci.ids (or any other ID table) during rescan.
    let autoprobe_disabled = target_driver.is_some();
    if autoprobe_disabled {
        tracing::info!(
            bdf,
            target = ?target_driver,
            "disabling drivers_autoprobe before rescan"
        );
        let _ = sysfs_write_direct(&linux_paths::sysfs_pci_drivers_autoprobe(), "0");
    }

    // Ensure autoprobe is re-enabled on all exit paths.
    let result = pci_remove_rescan_inner(bdf, target_driver);

    if autoprobe_disabled {
        let _ = sysfs_write_direct(&linux_paths::sysfs_pci_drivers_autoprobe(), "1");
        tracing::debug!(bdf, "drivers_autoprobe re-enabled");
    }

    result
}

fn pci_remove_rescan_inner(bdf: &str, target_driver: Option<&str>) -> Result<(), SysfsError> {
    tracing::info!(bdf, "PCI remove + rescan: removing device");
    pci_remove(bdf)?;

    for i in 0..6 {
        std::thread::sleep(Duration::from_secs(1));
        if !std::path::Path::new(&linux_paths::sysfs_pci_device_path(bdf)).exists() {
            tracing::info!(bdf, seconds = i + 1, "device removed from sysfs");
            break;
        }
    }

    std::thread::sleep(Duration::from_secs(2));

    tracing::info!(bdf, "PCI remove + rescan: rescanning bus");
    pci_rescan()?;

    for i in 0..10 {
        std::thread::sleep(Duration::from_secs(1));
        if std::path::Path::new(&linux_paths::sysfs_pci_device_path(bdf)).exists() {
            tracing::info!(bdf, seconds = i + 1, "device re-appeared after rescan");
            pin_power(bdf);
            pin_bridge_power(bdf);
            let _ =
                sysfs_write_direct(&linux_paths::sysfs_pci_device_file(bdf, "reset_method"), "");

            if let Some(driver) = target_driver {
                tracing::info!(
                    bdf,
                    driver,
                    "setting driver_override before probe (autoprobe disabled)"
                );
                let _ = sysfs_write_direct(
                    &linux_paths::sysfs_pci_device_file(bdf, "driver_override"),
                    driver,
                );
                tracing::info!(bdf, "triggering manual drivers_probe");
                let _ = sysfs_write(&linux_paths::sysfs_pci_drivers_probe(), bdf);
            }

            return Ok(());
        }
    }

    Err(SysfsError::DeviceNotReappeared {
        bdf: bdf.to_string(),
    })
}

/// Remove a PCI device from the kernel's device tree.
/// This forces full cleanup of sysfs entries, DRM nodes, hwmon, etc.
pub fn pci_remove(bdf: &str) -> Result<(), SysfsError> {
    let path = linux_paths::sysfs_pci_device_file(bdf, "remove");
    sysfs_write(&path, "1")
}

/// Trigger a PCI bus rescan, causing the kernel to re-enumerate
/// all devices and probe matching drivers.
pub fn pci_rescan() -> Result<(), SysfsError> {
    sysfs_write(&linux_paths::sysfs_pci_bus_rescan(), "1")
}

/// PM power cycle: transition through D3hot → D0 to reinitialize the
/// function without a bus reset. The PCIe spec requires D3hot→D0 to
/// reset function-level state while preserving PCI topology.
///
/// Power state transitions use the guarded write path since they can
/// stall if the device firmware is unresponsive.
pub fn pm_power_cycle(bdf: &str) -> Result<(), SysfsError> {
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
        assert!(err.to_string().contains("sysfs write"));
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
        assert!(err.to_string().contains("sysfs write"));
    }

    #[test]
    fn err_if_pm_cycle_d3cold_rejects_d3cold() {
        let bdf = "0000:01:00.0";
        let err = err_if_pm_cycle_d3cold(bdf, "D3cold").unwrap_err();
        let s = err.to_string();
        assert!(s.contains(bdf));
        assert!(s.contains("D3cold"));
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
        assert!(err.to_string().contains("sysfs write"));
    }

    #[test]
    fn pci_remove_rescan_targeted_accepts_none_target() {
        // With target=None, behaves like the original pci_remove_rescan.
        // Invalid BDF ensures early failure (device doesn't exist).
        let err = pci_remove_rescan_targeted("9999:99:99.9", None).unwrap_err();
        assert!(
            err.to_string().contains("sysfs write"),
            "expected sysfs error, got: {err}"
        );
    }

    #[test]
    fn pci_remove_rescan_targeted_accepts_some_target() {
        // With a target driver, the function should still fail on
        // invalid BDF but exercise the autoprobe-disable path.
        let err = pci_remove_rescan_targeted("9999:99:99.9", Some("nouveau")).unwrap_err();
        assert!(
            err.to_string().contains("sysfs write"),
            "expected sysfs error, got: {err}"
        );
    }
}
