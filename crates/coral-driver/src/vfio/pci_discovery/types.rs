// SPDX-License-Identifier: AGPL-3.0-only
//! PCI / PCIe data types: power states, BARs, capabilities, link speed.

use std::fmt;

use crate::nv::identity::{PCI_VENDOR_AMD, PCI_VENDOR_INTEL, PCI_VENDOR_NVIDIA};

/// PCI PM power state (from PCI Power Management spec).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PciPmState {
    /// Fully operational.
    D0,
    /// Low-power state (optional, rarely used by GPUs).
    D1,
    /// Lower-power state (optional, rarely used by GPUs).
    D2,
    /// PCIe sleep — BARs disabled, config space partially accessible.
    D3Hot,
    /// Full power off — requires PCI bus rescan to recover.
    D3Cold,
    /// State could not be determined.
    Unknown(u8),
}

impl PciPmState {
    pub(super) fn from_pmcsr_bits(bits: u8) -> Self {
        match bits & 0x03 {
            0 => Self::D0,
            1 => Self::D1,
            2 => Self::D2,
            3 => Self::D3Hot,
            _ => Self::Unknown(bits),
        }
    }

    /// PMCSR bit encoding for this state.
    pub fn pmcsr_bits(self) -> u8 {
        match self {
            Self::D0 => 0,
            Self::D1 => 1,
            Self::D2 => 2,
            Self::D3Hot => 3,
            Self::D3Cold | Self::Unknown(_) => 3,
        }
    }
}

impl fmt::Display for PciPmState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::D0 => write!(f, "D0"),
            Self::D1 => write!(f, "D1"),
            Self::D2 => write!(f, "D2"),
            Self::D3Hot => write!(f, "D3hot"),
            Self::D3Cold => write!(f, "D3cold"),
            Self::Unknown(v) => write!(f, "Unknown({v:#x})"),
        }
    }
}

/// GPU vendor identified from PCI vendor ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuVendor {
    /// NVIDIA Corporation (0x10DE).
    Nvidia,
    /// Advanced Micro Devices (0x1002).
    Amd,
    /// Intel Corporation (0x8086).
    Intel,
    /// Unknown or non-GPU vendor.
    Unknown(u16),
}

impl GpuVendor {
    /// Identify vendor from PCI vendor ID.
    pub fn from_vendor_id(id: u16) -> Self {
        match id {
            PCI_VENDOR_NVIDIA => Self::Nvidia,
            PCI_VENDOR_AMD => Self::Amd,
            PCI_VENDOR_INTEL => Self::Intel,
            other => Self::Unknown(other),
        }
    }

    /// True if this vendor matches the given PCI vendor ID.
    #[must_use]
    pub fn matches_vendor_id(self, id: u16) -> bool {
        matches!(
            (self, id),
            (Self::Nvidia, PCI_VENDOR_NVIDIA)
                | (Self::Amd, PCI_VENDOR_AMD)
                | (Self::Intel, PCI_VENDOR_INTEL)
        )
    }
}

impl fmt::Display for GpuVendor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Nvidia => write!(f, "NVIDIA"),
            Self::Amd => write!(f, "AMD"),
            Self::Intel => write!(f, "Intel"),
            Self::Unknown(id) => write!(f, "Unknown({id:#06x})"),
        }
    }
}

/// A PCI Base Address Register (BAR).
#[derive(Debug, Clone)]
pub struct PciBar {
    /// BAR index (0–5).
    pub index: u8,
    /// Physical base address.
    pub base: u64,
    /// Region size in bytes.
    pub size: u64,
    /// MMIO (memory-mapped) vs I/O port.
    pub is_mmio: bool,
    /// 64-bit BAR (consumes two BAR slots).
    pub is_64bit: bool,
    /// Prefetchable memory.
    pub is_prefetchable: bool,
}

/// PCI capability ID: Power Management.
pub const PCI_CAP_ID_PM: u8 = 0x01;
/// PCI capability ID: PCI Express.
pub const PCI_CAP_ID_PCIE: u8 = 0x10;
/// PCI status register: capability list present (bit 4).
pub const PCI_STATUS_CAP_LIST: u16 = 0x10;

/// A PCI capability from the capability chain.
#[derive(Debug, Clone)]
pub struct PciCapability {
    /// Capability ID (0x01 = PM, 0x05 = MSI, 0x10 = PCIe, 0x11 = MSI-X).
    pub id: u8,
    /// Config space offset of this capability.
    pub offset: u8,
    /// Human-readable name.
    pub name: &'static str,
}

impl PciCapability {
    pub(super) fn name_for_id(id: u8) -> &'static str {
        match id {
            PCI_CAP_ID_PM => "Power Management",
            0x02 => "AGP",
            0x03 => "VPD",
            0x04 => "Slot ID",
            0x05 => "MSI",
            0x06 => "CompactPCI Hot Swap",
            0x07 => "PCI-X",
            0x08 => "HyperTransport",
            0x09 => "Vendor Specific",
            0x0A => "Debug Port",
            0x0B => "CompactPCI CRC",
            0x0C => "PCI Hot-Plug",
            0x0D => "PCI Bridge Subsystem VID",
            0x0E => "AGP 8x",
            0x0F => "Secure Device",
            PCI_CAP_ID_PCIE => "PCI Express",
            0x11 => "MSI-X",
            0x12 => "SATA",
            0x13 => "Advanced Features",
            0x14 => "Enhanced Allocation",
            0x15 => "Flattening Portal Bridge",
            _ => "Unknown",
        }
    }
}

/// Power management capability details.
#[derive(Debug, Clone)]
pub struct PciPowerInfo {
    /// Config space offset of the PM capability (None if not present).
    pub pm_cap_offset: Option<u8>,
    /// Current power state.
    pub current_state: PciPmState,
    /// D1 power state supported.
    pub d1_support: bool,
    /// D2 power state supported.
    pub d2_support: bool,
    /// PME (Power Management Event) support mask.
    pub pme_support: u8,
    /// Raw PMCSR register value.
    pub pmcsr_raw: u16,
}

/// PCIe link status and capabilities.
#[derive(Debug, Clone)]
pub struct PcieLinkInfo {
    /// Maximum link speed (e.g., "8.0 GT/s" for Gen3).
    pub max_speed: PcieLinkSpeed,
    /// Current negotiated speed.
    pub current_speed: PcieLinkSpeed,
    /// Maximum link width (e.g., x16).
    pub max_width: u8,
    /// Current negotiated width.
    pub current_width: u8,
}

/// PCIe link speed encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PcieLinkSpeed {
    /// 2.5 GT/s (Gen1).
    Gen1,
    /// 5.0 GT/s (Gen2).
    Gen2,
    /// 8.0 GT/s (Gen3).
    Gen3,
    /// 16.0 GT/s (Gen4).
    Gen4,
    /// 32.0 GT/s (Gen5).
    Gen5,
    /// Unknown speed encoding.
    Unknown(u8),
}

impl PcieLinkSpeed {
    pub(crate) fn from_encoding(val: u8) -> Self {
        match val & 0x0F {
            1 => Self::Gen1,
            2 => Self::Gen2,
            3 => Self::Gen3,
            4 => Self::Gen4,
            5 => Self::Gen5,
            other => Self::Unknown(other),
        }
    }
}

impl fmt::Display for PcieLinkSpeed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Gen1 => write!(f, "2.5 GT/s (Gen1)"),
            Self::Gen2 => write!(f, "5.0 GT/s (Gen2)"),
            Self::Gen3 => write!(f, "8.0 GT/s (Gen3)"),
            Self::Gen4 => write!(f, "16.0 GT/s (Gen4)"),
            Self::Gen5 => write!(f, "32.0 GT/s (Gen5)"),
            Self::Unknown(v) => write!(f, "Unknown({v:#x})"),
        }
    }
}
