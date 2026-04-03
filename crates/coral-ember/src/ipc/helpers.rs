// SPDX-License-Identifier: AGPL-3.0-only
//! JSON-RPC guard helpers (managed BDF allowlist, reset method ordering).

use std::collections::HashSet;
use std::io::Write;

use crate::error::SysfsError;
use crate::sysfs;

use super::jsonrpc::write_jsonrpc_error;

/// Reject a BDF that is not in the managed set from `glowplug.toml`.
pub(crate) fn require_managed_bdf(
    bdf: &str,
    managed: &HashSet<String>,
    stream: &mut impl Write,
    id: serde_json::Value,
) -> Result<(), Result<(), std::io::Error>> {
    if managed.contains(bdf) {
        return Ok(());
    }
    tracing::warn!(bdf, "BDF not in managed allowlist — rejecting RPC");
    let msg = format!(
        "BDF {bdf} is not managed by ember (not listed in glowplug.toml). \
         Only configured devices are accepted."
    );
    write_jsonrpc_error(stream, id, -32001, &msg).map_err(Err)?;
    Err(Ok(()))
}

/// Try reset methods in priority order until one succeeds.
///
/// For VfioFlr, uses the held device's VFIO fd (ember owns the fd).
pub(crate) fn try_reset_methods_with_flr(
    bdf: &str,
    methods: &[crate::vendor_lifecycle::ResetMethod],
    held: &std::sync::Arc<std::sync::RwLock<std::collections::HashMap<String, crate::hold::HeldDevice>>>,
) -> Result<(), SysfsError> {
    let mut last_err = String::new();
    for m in methods {
        let label = match m {
            crate::vendor_lifecycle::ResetMethod::BridgeSbr => "bridge-sbr",
            crate::vendor_lifecycle::ResetMethod::SysfsSbr => "sbr",
            crate::vendor_lifecycle::ResetMethod::RemoveRescan => "remove-rescan",
            crate::vendor_lifecycle::ResetMethod::VfioFlr => "flr",
        };
        tracing::info!(bdf, method = label, "trying reset method");
        let result = match m {
            crate::vendor_lifecycle::ResetMethod::BridgeSbr => sysfs::pci_bridge_reset(bdf),
            crate::vendor_lifecycle::ResetMethod::SysfsSbr => sysfs::pci_device_reset(bdf),
            crate::vendor_lifecycle::ResetMethod::RemoveRescan => sysfs::pci_remove_rescan(bdf),
            crate::vendor_lifecycle::ResetMethod::VfioFlr => {
                try_vfio_flr(bdf, held)
            }
        };
        match result {
            Ok(()) => return Ok(()),
            Err(e) => {
                tracing::warn!(bdf, method = label, error = %e, "reset method failed, trying next");
                last_err = format!("{label}: {e}");
            }
        }
    }
    Err(SysfsError::PciReset {
        bdf: bdf.to_string(),
        reason: last_err,
    })
}

/// Attempt VFIO_DEVICE_RESET (FLR) using ember's held VFIO fd.
///
/// Ember holds the VFIO device fds for all managed GPUs. For GPUs that
/// support FLR (e.g. RTX 3090, RX 6950 XT), this triggers the hardware
/// Function Level Reset through the VFIO ioctl.
///
/// **Not available on K80 or Titan V** — these GPUs lack FLR hardware.
/// The ioctl will return EINVAL; caller should fall back to SBR or PMC reset.
pub(crate) fn try_vfio_flr(
    bdf: &str,
    held: &std::sync::Arc<std::sync::RwLock<std::collections::HashMap<String, crate::hold::HeldDevice>>>,
) -> Result<(), String> {
    let map = held.read().map_err(|e| format!("lock poisoned: {e}"))?;
    let dev = map.get(bdf).ok_or_else(|| {
        format!("device {bdf} not held by ember — cannot perform VFIO FLR")
    })?;
    dev.device
        .reset()
        .map_err(|e| format!("VFIO_DEVICE_RESET (FLR) failed for {bdf}: {e}"))?;
    tracing::info!(bdf, "VFIO FLR completed via ember's held device fd");
    Ok(())
}

/// PMC soft-reset via BAR0 sysfs — works on ALL GPUs including non-FLR (K80, Titan V).
///
/// Opens the sysfs resource0 file and performs a PMC_ENABLE engine toggle,
/// resetting all GPU engines via software. This is the universal fallback for
/// GPUs that lack PCIe FLR.
pub(crate) fn try_pmc_soft_reset(bdf: &str) -> Result<(), String> {
    use coral_driver::gsp::RegisterAccess;

    let resource0 = format!(
        "{}/resource0",
        coral_driver::linux_paths::sysfs_pci_device_path(bdf)
    );
    let mut bar0 = coral_driver::nv::bar0::Bar0Access::open_resource(&resource0)
        .map_err(|e| format!("BAR0 open for PMC reset on {bdf}: {e}"))?;

    const PMC_ENABLE: u32 = 0x200;
    let pmc_before = bar0
        .read_u32(PMC_ENABLE)
        .map_err(|e| format!("PMC_ENABLE read on {bdf}: {e}"))?;

    bar0.write_u32(PMC_ENABLE, 0)
        .map_err(|e| format!("PMC_ENABLE clear on {bdf}: {e}"))?;
    std::thread::sleep(std::time::Duration::from_millis(20));
    bar0.write_u32(PMC_ENABLE, pmc_before)
        .map_err(|e| format!("PMC_ENABLE restore on {bdf}: {e}"))?;
    std::thread::sleep(std::time::Duration::from_millis(20));

    let pmc_after = bar0.read_u32(PMC_ENABLE).unwrap_or(0);
    tracing::info!(
        bdf,
        pmc_before = format_args!("{pmc_before:#010x}"),
        pmc_after = format_args!("{pmc_after:#010x}"),
        "PMC soft-reset completed via sysfs BAR0"
    );
    Ok(())
}

/// Query whether a device supports PCIe FLR by checking the PCI Express
/// capability in config space via sysfs.
pub(crate) fn device_has_flr(bdf: &str) -> bool {
    let config_path = format!(
        "{}/config",
        coral_driver::linux_paths::sysfs_pci_device_path(bdf)
    );
    let Ok(data) = std::fs::read(&config_path) else {
        return false;
    };
    pcie_config_has_flr(&data)
}

/// Parse the PCI Express Capability to check FLReset (bit 28 of DevCap).
fn pcie_config_has_flr(config: &[u8]) -> bool {
    if config.len() < 0x40 {
        return false;
    }
    let mut cap_ptr = config[0x34] as usize & 0xFC;
    let mut iters = 0u8;
    while cap_ptr >= 0x40 && cap_ptr + 1 < config.len() && iters < 48 {
        let cap_id = config[cap_ptr];
        if cap_id == 0x10 {
            let devcap_offset = cap_ptr + 4;
            if devcap_offset + 3 < config.len() {
                let devcap = u32::from_le_bytes([
                    config[devcap_offset],
                    config[devcap_offset + 1],
                    config[devcap_offset + 2],
                    config[devcap_offset + 3],
                ]);
                return devcap & (1 << 28) != 0;
            }
        }
        cap_ptr = config[cap_ptr + 1] as usize & 0xFC;
        iters += 1;
    }
    false
}
