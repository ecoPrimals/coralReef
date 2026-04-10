// SPDX-License-Identifier: AGPL-3.0-or-later
//! Sysfs-driven PCI power transitions and config snapshots.

use crate::PciDiscoveryError;

use crate::linux_paths;

use super::config_space::find_pm_capability_offset;
use super::parse::parse_pci_bdf;
use super::types::PciPmState;

/// Force a PCI device from D3hot back to D0 by writing to the PM capability.
///
/// When `vfio-pci` binds, it unconditionally transitions the GPU to D3hot.
/// BAR0 reads return 0xFFFFFFFF in D3hot, making VRAM inaccessible.
/// However, HBM2 training is NOT lost — the data is still in the memory
/// controller's registers. Writing D0 to the PCI PMCSR restores BAR0
/// access and VRAM is immediately alive again.
///
/// This is vendor-agnostic — works for any PCI device with PM capability.
pub fn force_pci_d0(bdf: &str) -> Result<(), PciDiscoveryError> {
    parse_pci_bdf(bdf).ok_or_else(|| PciDiscoveryError::InvalidBdf {
        bdf: bdf.to_string(),
    })?;
    let config_path = linux_paths::sysfs_pci_device_file(bdf, "config");
    let config = std::fs::read(&config_path)
        .map_err(|e| PciDiscoveryError::sysfs_io("read PCI config", config_path.clone(), e))?;

    let pm_off = find_pm_capability_offset(&config)?;
    let pmcsr_off = pm_off + 4;

    if pmcsr_off + 2 > config.len() {
        return Err(PciDiscoveryError::PmcsrBeyondConfig {
            pmcsr_off,
            config_len: config.len(),
        });
    }

    let pmcsr = u16::from_le_bytes([config[pmcsr_off], config[pmcsr_off + 1]]);
    let current_state = pmcsr & 0x03;

    if current_state == 0 {
        return Ok(());
    }

    let pm_states = ["D0", "D1", "D2", "D3hot"];
    let new_pmcsr_masked = pmcsr & !0x03;
    tracing::info!(
        from_state = pm_states[current_state as usize],
        pmcsr = format!("{pmcsr:#06x}"),
        new_pmcsr = format!("{new_pmcsr_masked:#06x}"),
        pmcsr_off = format!("{pmcsr_off:#04x}"),
        "PCI PM transition to D0"
    );

    let new_pmcsr = (pmcsr & !0x03).to_le_bytes();
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .open(&config_path)
        .map_err(|e| {
            PciDiscoveryError::sysfs_io("open PCI config for write", config_path.clone(), e)
        })?;

    use std::io::{Seek, Write};
    file.seek(std::io::SeekFrom::Start(pmcsr_off as u64))
        .map_err(|e| {
            PciDiscoveryError::sysfs_io("seek PMCSR in PCI config", config_path.clone(), e)
        })?;
    file.write_all(&new_pmcsr)
        .map_err(|e| PciDiscoveryError::sysfs_io("write PMCSR", config_path.clone(), e))?;

    // PCI spec requires 10ms after D3hot → D0 transition
    std::thread::sleep(std::time::Duration::from_millis(20));

    // Pin runtime PM to "on" so the kernel doesn't put the device back to D3hot
    let power_control = linux_paths::sysfs_pci_device_file(bdf, "power/control");
    if let Err(e) = std::fs::write(&power_control, "on") {
        tracing::warn!(error = %e, path = %power_control, "could not pin power/control=on");
    }

    Ok(())
}

/// Transition a PCI device to a specific power state.
///
/// Writes the target state to PMCSR bits \[1:0\]. Observe PCI spec recovery
/// delays: D3hot→D0 requires 10ms, D2→D0 requires 200µs, etc.
pub fn set_pci_power_state(bdf: &str, target: PciPmState) -> Result<PciPmState, PciDiscoveryError> {
    parse_pci_bdf(bdf).ok_or_else(|| PciDiscoveryError::InvalidBdf {
        bdf: bdf.to_string(),
    })?;
    let config_path = linux_paths::sysfs_pci_device_file(bdf, "config");
    let config = std::fs::read(&config_path)
        .map_err(|e| PciDiscoveryError::sysfs_io("read PCI config", config_path.clone(), e))?;

    let pm_off = find_pm_capability_offset(&config)?;
    let pmcsr_off = pm_off + 4;
    if pmcsr_off + 2 > config.len() {
        return Err(PciDiscoveryError::PmcsrBeyondConfig {
            pmcsr_off,
            config_len: config.len(),
        });
    }

    let old_pmcsr = u16::from_le_bytes([config[pmcsr_off], config[pmcsr_off + 1]]);
    let old_state = PciPmState::from_pmcsr_bits((old_pmcsr & 0x03) as u8);

    let new_bits = target.pmcsr_bits() as u16;
    let new_pmcsr = (old_pmcsr & !0x03) | new_bits;

    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .open(&config_path)
        .map_err(|e| {
            PciDiscoveryError::sysfs_io("open PCI config for write", config_path.clone(), e)
        })?;

    use std::io::{Seek, Write};
    file.seek(std::io::SeekFrom::Start(pmcsr_off as u64))
        .map_err(|e| {
            PciDiscoveryError::sysfs_io("seek PMCSR in PCI config", config_path.clone(), e)
        })?;
    file.write_all(&new_pmcsr.to_le_bytes())
        .map_err(|e| PciDiscoveryError::sysfs_io("write PMCSR", config_path.clone(), e))?;

    // Recovery delays per PCI PM spec
    let delay_ms = match (old_state, target) {
        (PciPmState::D3Hot, PciPmState::D0) => 20,
        (PciPmState::D2, PciPmState::D0) => 1,
        _ => 5,
    };
    std::thread::sleep(std::time::Duration::from_millis(delay_ms));

    Ok(old_state)
}

/// Trigger a PCI D3cold → D0 power cycle via sysfs.
///
/// Forces a full power-off/power-on cycle, which causes the boot ROM to
/// re-execute devinit (including HBM2 training). The device must NOT be
/// bound to any driver. Vendor-agnostic.
pub fn pci_power_cycle(bdf: &str) -> Result<bool, PciDiscoveryError> {
    parse_pci_bdf(bdf).ok_or_else(|| PciDiscoveryError::InvalidBdf {
        bdf: bdf.to_string(),
    })?;
    let dev_path = linux_paths::sysfs_pci_device_path(bdf);

    let driver_link = format!("{dev_path}/driver");
    if std::fs::read_link(&driver_link).is_ok() {
        return Err(PciDiscoveryError::DriverBoundForPowerCycle);
    }

    let _ = std::fs::write(format!("{dev_path}/d3cold_allowed"), "1");
    let _ = std::fs::write(format!("{dev_path}/power/control"), "auto");

    let remove_path = format!("{dev_path}/remove");
    std::fs::write(&remove_path, "1")
        .map_err(|e| PciDiscoveryError::sysfs_io("write PCI remove", remove_path, e))?;

    std::thread::sleep(std::time::Duration::from_secs(2));

    let rescan_path = linux_paths::sysfs_pci_bus_rescan();
    std::fs::write(&rescan_path, "1")
        .map_err(|e| PciDiscoveryError::sysfs_io("write PCI bus rescan", rescan_path.clone(), e))?;

    std::thread::sleep(std::time::Duration::from_secs(3));

    if !std::path::Path::new(&dev_path).exists() {
        return Err(PciDiscoveryError::DeviceMissingAfterRescan);
    }

    let _ = std::fs::write(format!("{dev_path}/d3cold_allowed"), "0");
    let _ = std::fs::write(format!("{dev_path}/power/control"), "on");

    Ok(true)
}

/// Snapshot a range of PCI config space registers.
///
/// Returns `(offset, value)` pairs for each 32-bit register in the range.
pub fn snapshot_config_space(
    bdf: &str,
    start: usize,
    end: usize,
) -> Result<Vec<(usize, u32)>, PciDiscoveryError> {
    parse_pci_bdf(bdf).ok_or_else(|| PciDiscoveryError::InvalidBdf {
        bdf: bdf.to_string(),
    })?;
    let config_path = linux_paths::sysfs_pci_device_file(bdf, "config");
    let config = std::fs::read(&config_path)
        .map_err(|e| PciDiscoveryError::sysfs_io("read PCI config", config_path.clone(), e))?;

    let mut regs = Vec::new();
    let end = end.min(config.len());
    for off in (start..end).step_by(4) {
        if off + 4 <= config.len() {
            let val = u32::from_le_bytes([
                config[off],
                config[off + 1],
                config[off + 2],
                config[off + 3],
            ]);
            regs.push((off, val));
        }
    }
    Ok(regs)
}
