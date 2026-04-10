// SPDX-License-Identifier: AGPL-3.0-or-later

use super::super::gpu_vendor::*;
use super::super::pci_discovery::GpuVendor;

/// Decoded NVIDIA GPU identity from BOOT0 register.
#[derive(Debug, Clone)]
pub struct NvVoltaIdentity {
    /// Raw BOOT0 register value.
    pub boot0: u32,
    /// Chip implementation number within the architecture.
    pub chip_impl: u8,
    /// Silicon revision.
    pub chip_rev: u8,
    /// Human-readable chip name (e.g., "GV100", "TU102").
    pub chip_name_str: String,
    /// Architecture generation name (e.g., "Volta", "Turing").
    pub arch_name: String,
}

impl NvVoltaIdentity {
    /// Decode identity from a BOOT0 register value.
    pub fn from_boot0(boot0: u32) -> Self {
        let arch_code = ((boot0 >> 20) & 0x1FF) as u16;
        let chip_impl = ((boot0 >> 20) & 0xFF) as u8;
        let chip_rev = (boot0 & 0xFF) as u8;

        let (chip_name_str, arch_name) = match arch_code {
            0x140 => ("GV100".into(), "Volta".into()),
            0x142 => ("GV10B".into(), "Volta".into()),
            0x160..=0x16F => (format!("TU{:02X}", arch_code & 0xFF), "Turing".into()),
            0x170..=0x17F => (format!("GA{:02X}", arch_code & 0xFF), "Ampere".into()),
            0x190..=0x19F => (format!("AD{:02X}", arch_code & 0xFF), "Ada".into()),
            _ => (
                format!("NV{arch_code:03X}"),
                format!("Unknown({arch_code:#x})"),
            ),
        };

        Self {
            boot0,
            chip_impl,
            chip_rev,
            chip_name_str,
            arch_name,
        }
    }
}

impl GpuIdentity for NvVoltaIdentity {
    fn vendor(&self) -> GpuVendor {
        GpuVendor::Nvidia
    }
    fn chip_name(&self) -> &str {
        &self.chip_name_str
    }
    fn architecture(&self) -> &str {
        &self.arch_name
    }
    fn implementation(&self) -> u8 {
        self.chip_impl
    }
    fn revision(&self) -> u8 {
        self.chip_rev
    }
    fn raw_id(&self) -> u32 {
        self.boot0
    }
}
