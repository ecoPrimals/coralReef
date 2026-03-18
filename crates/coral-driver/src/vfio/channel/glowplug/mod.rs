// SPDX-License-Identifier: AGPL-3.0-only
#![expect(missing_docs, reason = "GlowPlug warm-up engine; full docs planned")]
//! GlowPlug — sovereign GPU warm-up from cold state.
//!
//! A diesel engine glowplug pre-warms the cylinders so ignition can occur.
//! This module does the same for a VFIO-bound GPU: it brings the GPU from
//! a cold/reset state to one where VRAM is accessible, PFIFO is alive, and
//! BAR2 page tables are configured — without needing nouveau or any vendor driver.
//!
//! The warm-up sequence:
//! 1. PMC_ENABLE — clock all engine domains
//! 2. PFIFO reset cycle — bring up the scheduler and PBDMAs
//! 3. BAR2 page tables — build V2 MMU page tables in VRAM for GPU internal access
//! 4. FB init (WIP) — configure the framebuffer/HBM2 controller so VRAM is accessible
//! 5. MMU fault buffers — configure so the scheduler doesn't stall on faults
//! 6. Memory topology verification — confirm all paths are working

mod constants;
mod oracle;
mod pri;
mod state;
mod types;
mod warm;

use std::os::fd::RawFd;

use crate::vfio::bar_cartography;
use crate::vfio::device::MappedBar;
use crate::vfio::gpu_vendor::GpuMetal;
use crate::vfio::memory::{MemoryRegion, PraminRegion};

use super::oracle::OracleState;
use super::registers::{misc, mmu, pbdma, pfifo, pmc};

pub use types::{GpuThermalState, HealthSnapshot, StepSnapshot, WarmResult};

/// The GlowPlug — sovereign GPU warm-up engine.
///
/// Accepts an optional `GpuMetal` trait object for vendor-agnostic register
/// access. When provided, the warm-up sequence uses vendor-specific register
/// maps instead of hardcoded NVIDIA offsets. Falls back to NVIDIA Volta
/// defaults when no metal is set (backward compatibility).
pub struct GlowPlug<'a> {
    pub(crate) bar0: &'a MappedBar,
    pub(crate) container_fd: RawFd,
    /// PCI BDF string for sysfs access (e.g., "0000:4a:00.0").
    pub(crate) bdf: Option<String>,
    /// BDF of an oracle card (same GPU model, running nouveau) for register cloning.
    pub(crate) oracle_bdf: Option<String>,
    /// Vendor-agnostic GPU metal interface (optional).
    pub(crate) metal: Option<Box<dyn GpuMetal>>,
    /// Pre-loaded oracle state for digital PMU emulation.
    pub(crate) oracle_state: Option<OracleState>,
}

impl<'a> GlowPlug<'a> {
    pub fn new(bar0: &'a MappedBar, container_fd: RawFd) -> Self {
        Self {
            bar0,
            container_fd,
            bdf: None,
            oracle_bdf: None,
            metal: None,
            oracle_state: None,
        }
    }

    /// Create a GlowPlug with BDF for VBIOS access.
    pub fn with_bdf(bar0: &'a MappedBar, container_fd: RawFd, bdf: &str) -> Self {
        Self {
            bar0,
            container_fd,
            bdf: Some(bdf.to_string()),
            oracle_bdf: None,
            metal: None,
            oracle_state: None,
        }
    }

    /// Create a GlowPlug with both BDF and an oracle card for register cloning.
    pub fn with_oracle(
        bar0: &'a MappedBar,
        container_fd: RawFd,
        bdf: &str,
        oracle_bdf: &str,
    ) -> Self {
        Self {
            bar0,
            container_fd,
            bdf: Some(bdf.to_string()),
            oracle_bdf: Some(oracle_bdf.to_string()),
            metal: None,
            oracle_state: None,
        }
    }

    /// Load oracle state from a live nouveau-warm card.
    pub fn load_oracle_live(&mut self, oracle_bdf: &str) -> Result<(), String> {
        let state = OracleState::from_live_card(oracle_bdf)?;
        self.oracle_state = Some(state);
        Ok(())
    }

    /// Load oracle state from a BAR0 binary dump file.
    pub fn load_oracle_dump(&mut self, path: &std::path::Path) -> Result<(), String> {
        let state = OracleState::from_bar0_dump(path)?;
        self.oracle_state = Some(state);
        Ok(())
    }

    /// Load oracle state from a text register dump file.
    pub fn load_oracle_text(&mut self, path: &std::path::Path) -> Result<(), String> {
        let state = OracleState::from_text_dump(path)?;
        self.oracle_state = Some(state);
        Ok(())
    }

    /// Set a pre-loaded oracle state directly.
    pub fn set_oracle_state(&mut self, state: OracleState) {
        self.oracle_state = Some(state);
    }

    /// Attach a vendor-agnostic GPU metal implementation.
    pub fn with_metal(mut self, metal: Box<dyn GpuMetal>) -> Self {
        self.metal = Some(metal);
        self
    }

    pub(crate) fn r(&self, reg: usize) -> u32 {
        self.bar0.read_u32(reg).unwrap_or(0xDEAD_DEAD)
    }

    pub(crate) fn w(&self, reg: usize, val: u32) {
        let _ = self.bar0.write_u32(reg, val);
    }

    pub(crate) fn boot0_off(&self) -> usize {
        self.metal
            .as_ref()
            .map_or(misc::BOOT0, |m| m.boot0_offset())
    }

    pub(crate) fn pmc_enable_off(&self) -> usize {
        self.metal
            .as_ref()
            .map_or(pmc::ENABLE, |m| m.pmc_enable_offset())
    }

    pub(crate) fn pbdma_map_off(&self) -> Option<usize> {
        self.metal
            .as_ref()
            .map_or(Some(pfifo::PBDMA_MAP), |m| m.pbdma_map_offset())
    }

    pub(crate) fn bar2_block_off(&self) -> Option<usize> {
        self.metal
            .as_ref()
            .map_or(Some(misc::PBUS_BAR2_BLOCK), |m| m.bar2_block_offset())
    }

    fn snapshot_offsets(&self) -> Vec<usize> {
        let mut offsets = vec![self.boot0_off(), self.pmc_enable_off()];
        if let Some(pbdma) = self.pbdma_map_off() {
            offsets.push(pbdma);
        }
        if let Some(bar2) = self.bar2_block_off() {
            offsets.push(bar2);
        }
        offsets.extend_from_slice(&[
            0x2200,  // PFIFO_ENABLE
            0x9000,  // PTIMER_0
            0x2240C, // devinit status
            0x1700,  // BAR0_WINDOW
        ]);
        if let Some(ref metal) = self.metal {
            for domain in metal.power_domains() {
                if let Some(reg) = domain.enable_reg
                    && !offsets.contains(&reg)
                {
                    offsets.push(reg);
                }
            }
        }
        offsets
    }

    pub(crate) fn snap(&self) -> Vec<(usize, u32)> {
        bar_cartography::snapshot_registers(self.bar0, &self.snapshot_offsets())
    }

    /// Full initialization — warm + PFIFO interrupts + MMU fault buffers.
    pub fn full_init(&self) -> WarmResult {
        let mut result = self.warm();

        if !result.success && result.final_state != GpuThermalState::PfifoAliveVramDead {
            return result;
        }

        // PBDMA/HCE interrupt enables
        let pbdma_map = self.r(pfifo::PBDMA_MAP);
        for pid in 0..32_usize {
            if pbdma_map & (1 << pid) == 0 {
                continue;
            }
            self.w(pbdma::intr(pid), 0xFFFF_FFFF);
            self.w(pbdma::intr_en(pid), 0xEFFF_FEFF);
            self.w(pbdma::hce_intr(pid), 0xFFFF_FFFF);
            self.w(pbdma::hce_intr_en(pid), 0x8000_001F);
        }

        // PFIFO interrupts (oracle mask)
        self.w(pfifo::INTR, 0xFFFF_FFFF);
        self.w(pfifo::INTR_EN, 0x6181_0101);

        // MMU fault buffers (if VRAM accessible)
        if self.check_vram() {
            if let Ok(mut fault_region) = PraminRegion::new(self.bar0, 0x0001_0000, 4096) {
                for i in (0..4096).step_by(4) {
                    let _ = fault_region.write_u32(i, 0);
                }
            }
            self.w(mmu::FAULT_BUF1_LO, 0x0001_0000 >> 12);
            self.w(mmu::FAULT_BUF1_HI, 0x4000_0000);
            self.w(mmu::FAULT_BUF1_SIZE, 0xFFE0_0000);
            self.w(mmu::FAULT_BUF1_GET, 0);
            self.w(mmu::FAULT_BUF1_PUT, 0);

            self.w(mmu::FAULT_BUF0_LO, 0x0001_2000 >> 12);
            self.w(mmu::FAULT_BUF0_HI, 0);
        }

        result
            .log
            .push("full_init: interrupts + fault buffers configured".into());
        result
    }

    /// Create a GlowPlug health listener that monitors domain health.
    pub fn health_check(&self) -> HealthSnapshot {
        let thermal = self.check_state();
        let (alive, faulted, log) = self.check_pri_health();
        let vram_ok = self.check_vram();
        let pmc = self.r(self.pmc_enable_off());

        HealthSnapshot {
            thermal_state: thermal,
            domains_alive: alive,
            domains_faulted: faulted,
            vram_accessible: vram_ok,
            pmc_enable: pmc,
            log,
        }
    }

    /// Re-warm if the GPU has cooled since last check.
    pub fn rewarm_if_cooled(&self, previous: &HealthSnapshot) -> Option<WarmResult> {
        let current = self.health_check();

        let cooled = current.domains_faulted > previous.domains_faulted
            || (!current.vram_accessible && previous.vram_accessible)
            || (current.thermal_state != previous.thermal_state
                && current.thermal_state != GpuThermalState::Warm);

        if cooled {
            eprintln!(
                "GlowPlug: GPU cooled! was {:?}/{} alive, now {:?}/{} alive",
                previous.thermal_state,
                previous.domains_alive,
                current.thermal_state,
                current.domains_alive,
            );
            self.recover_pri_bus();

            let post_recovery = self.check_state();
            if post_recovery != GpuThermalState::Warm {
                return Some(self.warm());
            }
        }

        None
    }
}

impl std::fmt::Debug for GlowPlug<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GlowPlug")
            .field("state", &self.check_state())
            .finish()
    }
}
