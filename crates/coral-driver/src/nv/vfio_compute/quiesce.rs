// SPDX-License-Identifier: AGPL-3.0-or-later
//! GPU engine quiesce — disables DMA-generating engines before VFIO teardown.

use crate::vfio::device::MappedBar;

/// Quiesce all GPU engines that may generate DMA traffic.
///
/// Must be called BEFORE the VFIO device fd is closed, otherwise the
/// ongoing DMA hits a torn-down IOMMU domain → `IO_PAGE_FAULT` → the
/// kernel resets the device via secondary bus reset, wiping GPU state.
///
/// Disables PFIFO (stops GPFIFO fetches), halts all falcon
/// microcontrollers, and clears PMC engine enables that generate DMA.
pub(crate) fn quiesce_gpu_engines(bar0: &MappedBar) {
    let rd = |reg: u32| -> u32 { bar0.read_u32(reg as usize).unwrap_or(0xDEAD_DEAD) };
    let wr = |reg: u32, val: u32| {
        let _ = bar0.write_u32(reg as usize, val);
    };

    let pmc = rd(0x200);
    if pmc == 0xDEAD_DEAD || pmc & 0xBAD0_0000 == 0xBAD0_0000 {
        tracing::warn!(
            pmc = format_args!("{pmc:#010x}"),
            "quiesce_gpu_engines: BAR0 unreachable, skipping"
        );
        return;
    }
    tracing::info!(
        pmc = format_args!("{pmc:#010x}"),
        "quiesce_gpu_engines: starting"
    );

    // Disable PFIFO sub-engines to stop GPFIFO ring fetches (the primary
    // DMA source).  Do NOT touch PMC_ENABLE — stripping PGRAPH or PDAEMON
    // from PMC destroys warm state and forces cold recovery on next open.
    wr(0x2200, 0x0000_0000); // PFIFO_ENABLE = 0
    wr(0x2260, 0x0000_0000); // PBDMA_ENABLE = 0
    rd(0x2200); // flush

    // Halt falcon microcontrollers to stop autonomous DMA.
    let pmu_cpuctl = rd(0x10_A100);
    if pmu_cpuctl != 0xDEAD_DEAD
        && pmu_cpuctl & 0xBAD0_0000 != 0xBAD0_0000
        && pmu_cpuctl & 0x30 == 0
    {
        wr(0x10_A100, 0x10);
    }

    let fecs_cpuctl = rd(0x40_9100);
    if fecs_cpuctl != 0xDEAD_DEAD
        && fecs_cpuctl & 0xBAD0_0000 != 0xBAD0_0000
        && fecs_cpuctl & 0x30 == 0
    {
        wr(0x40_9100, 0x10);
    }

    let gpccs_cpuctl = rd(0x41_A100);
    if gpccs_cpuctl != 0xDEAD_DEAD
        && gpccs_cpuctl & 0xBAD0_0000 != 0xBAD0_0000
        && gpccs_cpuctl & 0x30 == 0
    {
        wr(0x41_A100, 0x10);
    }

    std::thread::sleep(std::time::Duration::from_millis(2));

    // Bus mastering is disabled by the caller via VFIO config space
    // (VfioDevice::disable_bus_master) BEFORE this function is called.
    // BAR0 PBUS mirror write is unreliable, so we skip it.
    let pci_cmd = rd(0x1804);
    if pci_cmd != 0xDEAD_DEAD && pci_cmd & 0x04 != 0 {
        wr(0x1804, pci_cmd & !0x04);
        rd(0x1804); // flush
    }

    std::thread::sleep(std::time::Duration::from_millis(5));
}
