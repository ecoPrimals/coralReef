// SPDX-License-Identifier: AGPL-3.0-only
#![expect(
    missing_docs,
    reason = "internal device state types; glowplug crate docs cover lifecycle semantics."
)]

use coral_driver::vfio::VfioDevice;
use coral_driver::vfio::device::MappedBar;

/// Holds a `VfioDevice` and its BAR0 mapping for register access.
///
/// Replaces direct `RawVfioDevice` usage — glowplug only needs BAR0
/// register reads, not DMA buffers or compute dispatch.
pub(crate) struct VfioHolder {
    device: VfioDevice,
    pub(crate) bar0: MappedBar,
}

impl VfioHolder {
    pub(crate) fn new(device: VfioDevice, bar0: MappedBar) -> Self {
        Self { device, bar0 }
    }

    /// Trigger a PCIe Function Level Reset via VFIO_DEVICE_RESET.
    pub(crate) fn reset(&self) -> Result<(), coral_driver::error::DriverError> {
        self.device.reset()
    }

    /// Disable PCI Bus Master — suppress GPU-initiated DMA.
    ///
    /// After nouveau→vfio swaps, stale DMA engines can fire requests to
    /// invalid IOMMU mappings, causing AER cascades that hard-lock the system.
    pub(crate) fn disable_bus_master(&self) -> Result<(), coral_driver::error::DriverError> {
        self.device.disable_bus_master()
    }
}

/// Comprehensive BAR0 register offsets for NVIDIA GV100 (Titan V / V100).
///
/// **Prefer using `coral_driver::nv::chip::ChipCapability::register_dump_offsets()`**
/// for per-chip register sets. This constant remains for backward compatibility.
pub const DEFAULT_REGISTER_DUMP_OFFSETS: &[usize] =
    coral_driver::nv::chip::VOLTA_REGISTER_DUMP_OFFSETS;

pub(crate) const QUIESCENCE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);
pub(crate) const QUIESCENCE_POLL_MS: u64 = 50;

pub(crate) const PCI_READ_DEAD: u32 = 0xDEAD_DEAD;
pub(crate) const PCI_READ_ALL_ONES: u32 = 0xFFFF_FFFF;
pub(crate) const PCI_FAULT_BADF: u16 = 0xBADF;
pub(crate) const PCI_FAULT_BAD0: u16 = 0xBAD0;
pub(crate) const PCI_FAULT_BAD1: u16 = 0xBAD1;

#[must_use]
pub(crate) const fn is_faulted_read(val: u32) -> bool {
    val == PCI_READ_DEAD
        || val == PCI_READ_ALL_ONES
        || (val >> 16) as u16 == PCI_FAULT_BADF
        || (val >> 16) as u16 == PCI_FAULT_BAD0
        || (val >> 16) as u16 == PCI_FAULT_BAD1
}

/// Graduated health probing phase after a driver swap.
///
/// GPU hardware needs time to settle after driver transitions. Probing
/// too many registers too early can hit powered-down engines and trigger
/// PCIe completion timeouts that cascade to system lockups.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealthPhase {
    /// 0-30s post-swap: only BOOT0 + PMC_ENABLE (always safe).
    Minimal,
    /// 30-120s: add PTIMER and PRI_RING_STATUS.
    Intermediate,
    /// 120s+ or no recent swap: full domain probe.
    Full,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerState {
    D0,
    D3Hot,
    D3Cold,
    Unknown,
}

impl std::fmt::Display for PowerState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::D0 => write!(f, "D0"),
            Self::D3Hot => write!(f, "D3hot"),
            Self::D3Cold => write!(f, "D3cold"),
            Self::Unknown => write!(f, "unknown"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct DeviceHealth {
    pub vram_alive: bool,
    pub boot0: u32,
    pub pmc_enable: u32,
    pub power: PowerState,
    pub pci_link_width: Option<u8>,
    pub domains_alive: usize,
    pub domains_faulted: usize,
}
