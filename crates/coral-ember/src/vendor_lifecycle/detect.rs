// SPDX-License-Identifier: AGPL-3.0-only
//! PCI ID tables and lifecycle detection from sysfs PCI IDs.

use crate::sysfs;

use super::types::VendorLifecycle;
use super::{
    AmdRdnaLifecycle, AmdVega20Lifecycle, BrainChipLifecycle, GenericLifecycle, IntelXeLifecycle,
    NvidiaKeplerLifecycle, NvidiaLifecycle, NvidiaOpenLifecycle, NvidiaOracleLifecycle,
};

const NVIDIA_VENDOR: u16 = 0x10de;
const AMD_VENDOR: u16 = 0x1002;
const INTEL_VENDOR: u16 = 0x8086;
const BRAINCHIP_VENDOR: u16 = 0x1e7c;

const AMD_VEGA20_IDS: &[u16] = &[0x66a0, 0x66a1, 0x66af];

/// Kepler device IDs: GK110, GK110B, GK210 families.
const NVIDIA_KEPLER_IDS: &[u16] = &[
    // GK110 — original Kepler big-die
    0x1003, // GK110 (Tesla K20X)
    0x1004, // GK110 (Tesla K20)
    0x1005, // GK110 (Tesla K20X)
    0x100a, // GK110 (engineering sample)
    0x100c, // GK110 (GTX Titan)
    // GK110B — revised GK110
    0x1021, // GK110B (Tesla K20X)
    0x1022, // GK110B (Tesla K20c)
    0x1024, // GK110B (Tesla K40m)
    0x1026, // GK110B (Tesla K20s)
    0x1027, // GK110B (Tesla K40st)
    0x1028, // GK110B (Tesla K20m)
    0x1029, // GK110B (Tesla K40s)
    0x102a, // GK110B (Tesla K40t)
    0x102e, // GK110B (GTX Titan Black)
    0x102f, // GK110B (Tesla accelerator)
    // GK210 — K80 dual-die
    0x102d, // GK210 (Tesla K80)
];

pub(crate) fn is_nvidia_kepler(device_id: u16) -> bool {
    NVIDIA_KEPLER_IDS.contains(&device_id)
}

pub(crate) fn is_amd_vega20(device_id: u16) -> bool {
    AMD_VEGA20_IDS.contains(&device_id)
}

/// Build a [`VendorLifecycle`] from PCI config-space IDs (used by [`detect_lifecycle`] and unit tests).
pub(crate) fn lifecycle_from_pci_ids(vendor_id: u16, device_id: u16) -> Box<dyn VendorLifecycle> {
    match vendor_id {
        NVIDIA_VENDOR => {
            if is_nvidia_kepler(device_id) {
                Box::new(NvidiaKeplerLifecycle { device_id })
            } else {
                Box::new(NvidiaLifecycle { device_id })
            }
        }
        AMD_VENDOR => {
            if is_amd_vega20(device_id) {
                Box::new(AmdVega20Lifecycle { device_id })
            } else {
                Box::new(AmdRdnaLifecycle { device_id })
            }
        }
        INTEL_VENDOR => Box::new(IntelXeLifecycle { device_id }),
        BRAINCHIP_VENDOR => Box::new(BrainChipLifecycle { device_id }),
        _ => Box::new(GenericLifecycle {
            vendor_id,
            device_id,
        }),
    }
}

/// Build a lifecycle for a specific target driver override (e.g. `nvidia_oracle_535`).
/// Falls back to [`detect_lifecycle`] if the target doesn't need special handling.
pub fn detect_lifecycle_for_target(bdf: &str, target: &str) -> Box<dyn VendorLifecycle> {
    if target.starts_with("nvidia_oracle") {
        let device_id = sysfs::read_pci_id(bdf, "device");
        return Box::new(NvidiaOracleLifecycle {
            device_id,
            module_name: target.to_string(),
        });
    }
    if target == "nvidia-open" {
        let device_id = sysfs::read_pci_id(bdf, "device");
        return Box::new(NvidiaOpenLifecycle { device_id });
    }
    detect_lifecycle(bdf)
}

/// Auto-detect the appropriate VendorLifecycle for a PCI device.
pub fn detect_lifecycle(bdf: &str) -> Box<dyn VendorLifecycle> {
    let vendor_id = sysfs::read_pci_id(bdf, "vendor");
    let device_id = sysfs::read_pci_id(bdf, "device");

    tracing::info!(
        bdf,
        vendor = format!("0x{vendor_id:04x}"),
        device = format!("0x{device_id:04x}"),
        "detecting vendor lifecycle"
    );

    let lc = lifecycle_from_pci_ids(vendor_id, device_id);
    match vendor_id {
        NVIDIA_VENDOR => {
            if is_nvidia_kepler(device_id) {
                tracing::info!(bdf, "lifecycle: NVIDIA Kepler (cold-sensitive, no FLR)");
            } else {
                tracing::info!(bdf, "lifecycle: NVIDIA (Volta+)");
            }
        }
        AMD_VENDOR => {
            if is_amd_vega20(device_id) {
                tracing::info!(bdf, "lifecycle: AMD Vega 20 (D3cold-sensitive)");
            } else {
                tracing::info!(bdf, "lifecycle: AMD RDNA (conservative)");
            }
        }
        INTEL_VENDOR => tracing::info!(bdf, "lifecycle: Intel Xe"),
        BRAINCHIP_VENDOR => tracing::info!(bdf, "lifecycle: BrainChip Akida"),
        _ => tracing::warn!(
            bdf,
            vendor = format!("0x{vendor_id:04x}"),
            "lifecycle: unknown vendor, using conservative defaults"
        ),
    }
    lc
}
