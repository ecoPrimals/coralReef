// SPDX-License-Identifier: AGPL-3.0-only

//! Safe DMA preparation for post-driver-swap VFIO devices.
//!
//! After a driver swap (e.g. nouveau → vfio-pci), the GPU's internal DMA
//! engines (PFIFO, PBDMA, COPY, etc.) retain stale channel descriptors.
//! Enabling PCI bus mastering without first quiescing these engines allows
//! them to fire DMA transactions that fault the PRI ring, causing subsequent
//! BAR0 reads to block the CPU forever in the PCIe fabric.
//!
//! This module provides a single, centralized implementation of the safe
//! quiesce sequence that all callers (ember, glowplug, experiments) must
//! use instead of hand-rolling register manipulation.

use super::aer::AerMaskState;
use super::MappedBar;
use super::VfioDevice;
use crate::error::DriverError;

/// BAR0 offsets known to cause silent CPU hangs when read after a nouveau
/// driver teardown on GV100 (Titan V) and likely other Volta+ GPUs.
///
/// Reading these offsets blocks indefinitely in the PCIe fabric: no timeout,
/// no AER error, no NMI, no kernel log. The CPU thread enters TASK_UNINTERRUPTIBLE
/// and requires a hard reboot.
pub const POISONOUS_POST_NOUVEAU: &[usize] = &[
    0x0012_0058, // PRI_RING_INTR_STATUS — blocks when PRI hub has routing fault
];

/// Returns `true` if `offset` is known to cause a silent CPU lockup
/// after a driver swap (nouveau teardown).
pub fn is_poisonous_read(offset: usize) -> bool {
    POISONOUS_POST_NOUVEAU.contains(&offset)
}

const PMC_ENABLE: usize = 0x000200;
const PMC_INTR_EN_SET: usize = 0x000240;
const PMC_INTR_EN_CLR: usize = 0x000244;
const PFIFO_ENABLE: usize = 0x002200;
const PFIFO_BIT: u32 = 1 << 8;
const PMU_BIT: u32 = 1 << 0;
const SEC2_BIT: u32 = 1 << 5;
const TOP_BIT: u32 = 1 << 30;
const PRIV_RING_COMMAND: usize = 0x0012_004C;
const PRIV_RING_CMD_ACK: u32 = 0x2;
/// Minimal safe PMC_ENABLE: only SEC2 + PFIFO + PMU + TOP.
/// All other engines (GR, PBDMA, NVDEC, etc.) are disabled to prevent
/// stale DMA/firmware from causing PRI ring faults during BAR0 reads.
const MINIMAL_PMC: u32 = SEC2_BIT | PFIFO_BIT | PMU_BIT | TOP_BIT;

/// Saved state from [`prepare_dma`], needed by [`cleanup_dma`] to restore
/// AER masks and bus mastering.
#[derive(Debug, Clone)]
pub struct DmaPrepareState {
    /// Saved AER mask state for restoration.
    pub aer_state: Option<AerMaskState>,
    /// PMC_ENABLE before quiesce (diagnostic).
    pub pmc_before: u32,
    /// PMC_ENABLE after quiesce (diagnostic).
    pub pmc_after: u32,
}

/// Aggressively quiesce a GPU after a warm driver swap (nouveau/nvidia → vfio).
///
/// After nouveau teardown, the GPU may have many engines alive in PMC_ENABLE
/// with stale firmware, channels, and DMA descriptors.  Reading registers on
/// these live engines can trigger PRI ring hangs that lock up the entire system.
///
/// This function strips PMC_ENABLE to a minimal set (SEC2 + PFIFO + PMU + TOP),
/// stops the PFIFO scheduler, and blind-ACKs PRI ring faults.  Call this
/// **immediately** after a warm swap before any BAR0 register reads beyond
/// BOOT0/PMC_ENABLE.
///
/// PRAMIN and the memory fabric survive because PFB is always-on on GV100+
/// (not gated by PMC_ENABLE).
pub fn post_swap_quiesce(bar0: &MappedBar) {
    let r = |off: usize| bar0.read_u32(off).unwrap_or(0xDEAD_DEAD);
    let w = |off: usize, val: u32| {
        let _ = bar0.write_u32(off, val);
    };

    let pmc_before = r(PMC_ENABLE);
    let warm = pmc_before.count_ones() > 4;

    if !warm {
        tracing::info!(
            pmc = format_args!("{pmc_before:#010x}"),
            "post_swap_quiesce: already minimal — skipping"
        );
        return;
    }

    tracing::warn!(
        pmc_before = format_args!("{pmc_before:#010x}"),
        active_engines = pmc_before.count_ones(),
        "post_swap_quiesce: WARM GPU detected — stripping engines"
    );

    // Step 1: Disable all interrupts to prevent cascading during quiesce
    w(PMC_INTR_EN_CLR, 0xFFFF_FFFF);

    // Step 2: Stop PFIFO scheduler before touching PMC_ENABLE
    w(PFIFO_ENABLE, 0);
    std::thread::sleep(std::time::Duration::from_millis(5));

    // Step 3: Blind-ACK any pending PRI ring faults (write-only)
    w(PRIV_RING_COMMAND, PRIV_RING_CMD_ACK);
    std::thread::sleep(std::time::Duration::from_millis(10));

    // Step 4: Strip PMC_ENABLE to minimal — kills GR, PBDMA, NVDEC, etc.
    w(PMC_ENABLE, MINIMAL_PMC);
    std::thread::sleep(std::time::Duration::from_millis(20));

    // Step 5: Second blind-ACK to clear any faults from engine teardown
    w(PRIV_RING_COMMAND, PRIV_RING_CMD_ACK);
    std::thread::sleep(std::time::Duration::from_millis(10));

    let pmc_after = r(PMC_ENABLE);
    tracing::info!(
        pmc_before = format_args!("{pmc_before:#010x}"),
        pmc_after = format_args!("{pmc_after:#010x}"),
        "post_swap_quiesce: engines stripped, PRI ACKed"
    );
}

/// Safely prepare a GPU for DMA after a driver swap.
///
/// Performs the complete quiesce sequence:
/// 1. Mask PCIe AER errors (prevents kernel cascade on stray GPU faults)
/// 2. Reset PFIFO via PMC_ENABLE bit 8 toggle (clears stale channel state)
/// 3. Stop PFIFO scheduler (PFIFO_ENABLE = 0)
/// 4. Blind-ACK PRI ring faults (write-only — **never** reads 0x120058)
/// 5. Enable PCI bus mastering + D0 power state transition
///
/// After this call, bus mastering is ON and stale DMA engines are quiesced.
/// Call [`cleanup_dma`] when the experiment is finished.
pub fn prepare_dma(
    bar0: &MappedBar,
    device: &VfioDevice,
) -> Result<DmaPrepareState, DriverError> {
    let r = |off: usize| bar0.read_u32(off).unwrap_or(0xDEAD_DEAD);
    let w = |off: usize, val: u32| {
        let _ = bar0.write_u32(off, val);
    };

    // Step 1: Mask AER
    let aer_state = match device.mask_aer() {
        Ok(state) => {
            tracing::info!(
                aer_cap = format_args!("{:#x}", state.aer_cap_offset),
                "dma_safety: AER masked"
            );
            Some(state)
        }
        Err(e) => {
            tracing::warn!(error = %e, "dma_safety: AER mask failed (proceeding)");
            None
        }
    };

    // Step 2: PFIFO reset via PMC_ENABLE bit 8 toggle
    let pmc = r(PMC_ENABLE);
    w(PMC_ENABLE, pmc & !PFIFO_BIT);
    std::thread::sleep(std::time::Duration::from_millis(5));
    w(PMC_ENABLE, pmc | PFIFO_BIT);
    std::thread::sleep(std::time::Duration::from_millis(10));
    let pmc_after = r(PMC_ENABLE);
    tracing::info!(
        pmc_before = format_args!("{pmc:#010x}"),
        pmc_after = format_args!("{pmc_after:#010x}"),
        "dma_safety: PFIFO reset (PMC bit 8 toggle)"
    );

    // Step 3: Stop PFIFO scheduler
    w(PFIFO_ENABLE, 0);
    std::thread::sleep(std::time::Duration::from_millis(5));
    tracing::info!("dma_safety: PFIFO scheduler stopped (0x2200=0)");

    // Step 4: Blind-ACK PRI ring faults.
    //
    // NEVER read 0x120058 (PRI_RING_INTR_STATUS) after a nouveau teardown.
    // The PRI hub may have an uncleared routing fault that makes reads to
    // that register block the CPU forever in the PCIe fabric.
    w(PRIV_RING_COMMAND, PRIV_RING_CMD_ACK);
    std::thread::sleep(std::time::Duration::from_millis(10));
    tracing::info!("dma_safety: PRI ring blind ACK (0x120058 read SKIPPED)");

    // Step 5: Enable bus master
    device.enable_bus_master()?;
    tracing::info!("dma_safety: bus master ENABLED (stale DMA quiesced)");

    Ok(DmaPrepareState {
        aer_state,
        pmc_before: pmc,
        pmc_after,
    })
}

/// BAR0-only portion of [`prepare_dma`] — suitable for fork-isolated children.
///
/// Performs only the BAR0 MMIO operations (PFIFO reset, scheduler stop, PRI
/// ACK). AER masking and bus master enable use VFIO ioctls and must run in
/// the parent process.
///
/// Returns `(pmc_before, pmc_after)` on success.
pub fn prepare_dma_bar0_only(bar0: &MappedBar) -> Result<(u32, u32), DriverError> {
    let r = |off: usize| bar0.read_u32(off).unwrap_or(0xDEAD_DEAD);
    let w = |off: usize, val: u32| {
        let _ = bar0.write_u32(off, val);
    };

    let pmc = r(PMC_ENABLE);
    w(PMC_ENABLE, pmc & !PFIFO_BIT);
    std::thread::sleep(std::time::Duration::from_millis(5));
    w(PMC_ENABLE, pmc | PFIFO_BIT);
    std::thread::sleep(std::time::Duration::from_millis(10));
    let pmc_after = r(PMC_ENABLE);

    w(PFIFO_ENABLE, 0);
    std::thread::sleep(std::time::Duration::from_millis(5));

    w(PRIV_RING_COMMAND, PRIV_RING_CMD_ACK);
    std::thread::sleep(std::time::Duration::from_millis(10));

    Ok((pmc, pmc_after))
}

/// Disable bus mastering and restore AER masks after an experiment.
pub fn cleanup_dma(
    device: &VfioDevice,
    state: &DmaPrepareState,
) -> Result<(), DriverError> {
    if let Err(e) = device.disable_bus_master() {
        tracing::warn!(error = %e, "dma_safety cleanup: bus master disable failed");
    } else {
        tracing::info!("dma_safety cleanup: bus master disabled");
    }

    if let Some(ref aer) = state.aer_state {
        if let Err(e) = device.unmask_aer(aer) {
            tracing::warn!(error = %e, "dma_safety cleanup: AER unmask failed");
        } else {
            tracing::info!("dma_safety cleanup: AER masks restored");
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn poisonous_registers_include_pri_ring_status() {
        assert!(is_poisonous_read(0x0012_0058));
    }

    #[test]
    fn safe_registers_not_flagged() {
        assert!(!is_poisonous_read(0x0000_0000)); // BOOT0
        assert!(!is_poisonous_read(0x0000_0200)); // PMC_ENABLE
        assert!(!is_poisonous_read(0x0000_2200)); // PFIFO_ENABLE
        assert!(!is_poisonous_read(0x0012_004C)); // PRIV_RING_COMMAND (write-only, safe)
    }
}
