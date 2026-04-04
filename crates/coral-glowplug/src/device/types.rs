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
/// Covers PMC, PBUS, PFIFO, PBDMA, PFB, FBHUB, PMU, PCLOCK, GR/FECS/GPCCS,
/// LTC, FBPA, PRAMIN, and thermal domains.
pub const DEFAULT_REGISTER_DUMP_OFFSETS: &[usize] = &[
    // PMC
    0x00_0000, 0x00_0004, 0x00_0200, 0x00_0204, // PBUS
    0x00_1C00, 0x00_1C04, // PFIFO
    0x00_2004, 0x00_2100, 0x00_2140, 0x00_2200, 0x00_2254, 0x00_2270, 0x00_2274, 0x00_2280,
    0x00_2284, 0x00_228C, 0x00_2390, 0x00_2394, 0x00_2398, 0x00_239C, 0x00_2504, 0x00_2508,
    0x00_252C, 0x00_2630, 0x00_2634, 0x00_2638, 0x00_2640, 0x00_2A00, 0x00_2A04,
    // PBDMA idle + PBDMA0
    0x00_3080, 0x00_3084, 0x00_3088, 0x00_308C, 0x04_0040, 0x04_0044, 0x04_0048, 0x04_004C,
    0x04_0054, 0x04_0060, 0x04_0068, 0x04_0080, 0x04_0084, 0x04_00A4, 0x04_0100, 0x04_0104,
    0x04_0108, 0x04_010C, 0x04_0110, 0x04_0114, 0x04_0118, // PFB / FBHUB
    0x10_0000, 0x10_0200, 0x10_0204, 0x10_0C80, 0x10_0C84, 0x10_0800, 0x10_0804, 0x10_0808,
    0x10_080C, 0x10_0810, // BAR1 / BAR2 PRAMIN
    0x10_1000, 0x10_1004, 0x10_1008, 0x10_1714, // PMU Falcon
    0x10_A000, 0x10_A040, 0x10_A044, 0x10_A04C, 0x10_A100, 0x10_A104, 0x10_A108, 0x10_A110,
    0x10_A114, 0x10_A118, // PCLOCK
    0x13_7000, 0x13_7050, 0x13_7100, // GR (graphics engine)
    0x40_0100, 0x40_0108, 0x40_0110, // FECS Falcon
    0x40_9028, 0x40_9030, 0x40_9034, 0x40_9038, 0x40_9040, 0x40_9044, 0x40_904C, 0x40_9080,
    0x40_9084, 0x40_9100, 0x40_9104, 0x40_9108, 0x40_9110, 0x40_9210, 0x40_9380,
    // GPCCS Falcon
    0x41_A028, 0x41_A030, 0x41_A034, 0x41_A038, 0x41_A040, 0x41_A044, 0x41_A04C, 0x41_A080,
    0x41_A084, 0x41_A100, 0x41_A108, // MMU Fault buffer
    0x10_0E24, 0x10_0E28, 0x10_0E2C, 0x10_0E30, // LTC (L2 cache)
    0x17_E200, 0x17_E204, 0x17_E210, // FBPA0
    0x9A_0000, 0x9A_0004, 0x9A_0200, // THERM
    0x02_0400, 0x02_0460, // NV_PRAMIN window
    0x70_0000, 0x70_0004, // PROM
    0x30_0000, 0x30_0004,
];

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
