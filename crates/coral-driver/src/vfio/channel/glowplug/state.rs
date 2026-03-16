// SPDX-License-Identifier: AGPL-3.0-only
//! Thermal state detection and register snapshot helpers.

use crate::vfio::bar_cartography;
use crate::vfio::gpu_vendor::PowerBounds;
use crate::vfio::memory::{MemoryRegion, PraminRegion};
use crate::vfio::pci_discovery;

use super::super::registers::{misc, pfifo, pmc};
use super::GlowPlug;
use super::types::GpuThermalState;

impl GlowPlug<'_> {
    /// Diagnose the current thermal state of the GPU.
    ///
    /// Uses vendor-agnostic register offsets when a `GpuMetal` is attached,
    /// falling back to hardcoded NVIDIA Volta defaults otherwise.
    pub fn check_state(&self) -> GpuThermalState {
        let boot0 = self.r(self.boot0_off());
        if boot0 == 0xFFFF_FFFF {
            return GpuThermalState::D3Hot;
        }

        let pmc = self.r(self.pmc_enable_off());
        if pmc == 0x4000_0020 || pmc == 0 {
            return GpuThermalState::ColdGated;
        }

        if let Some(pbdma_off) = self.pbdma_map_off() {
            let pbdma_map = self.r(pbdma_off);
            let pfifo_bit = pmc & (1 << 8) != 0;
            let pbdma_alive = pbdma_map != 0 && pbdma_map != 0xBAD0_DA00;
            if !pfifo_bit || !pbdma_alive {
                return GpuThermalState::EnginesClocked;
            }
        }

        let vram_ok = self.check_vram();
        if !vram_ok {
            return GpuThermalState::PfifoAliveVramDead;
        }

        if let Some(bar2_off) = self.bar2_block_off() {
            let bar2_block = self.r(bar2_off);
            let bar2_valid =
                bar2_block != 0x4000_0000 && bar2_block != 0 && (bar2_block >> 16) != 0xBAD0;
            if !bar2_valid {
                return GpuThermalState::VramAliveBar2Dead;
            }
        }

        GpuThermalState::Warm
    }

    /// Quick VRAM accessibility check via PRAMIN at offset 0x26000.
    pub fn check_vram(&self) -> bool {
        if let Ok(mut region) = PraminRegion::new(self.bar0, 0x0002_6000, 8) {
            let status = region.probe_sentinel(0, 0xCAFE_DEAD);
            status.is_working()
        } else {
            false
        }
    }

    /// Empirically map what state survives each power transition.
    ///
    /// Tests D3hot, D3cold, and clock gating transitions by snapshotting
    /// key registers before/after and reporting what persists vs. what is
    /// lost. Requires BDF to be set (for PCI power state transitions).
    pub fn probe_bounds(&self) -> PowerBounds {
        let mut bounds = PowerBounds::default();
        let bdf = match &self.bdf {
            Some(b) => b.clone(),
            None => return bounds,
        };

        // Collect key register offsets to snapshot
        let snapshot_offsets: Vec<usize> = if let Some(ref metal) = self.metal {
            let mut offsets = vec![metal.boot0_offset(), metal.pmc_enable_offset()];
            if let Some(pbdma) = metal.pbdma_map_offset() {
                offsets.push(pbdma);
            }
            // Add domain-specific registers
            for domain in metal.power_domains() {
                if let Some(reg) = domain.enable_reg
                    && !offsets.contains(&reg)
                {
                    offsets.push(reg);
                }
            }
            offsets
        } else {
            vec![
                misc::BOOT0,
                pmc::ENABLE,
                pfifo::PBDMA_MAP,
                pfifo::ENABLE,
                0x100800, // FBHUB
            ]
        };

        // Snapshot before D3hot test
        let before = bar_cartography::snapshot_registers(self.bar0, &snapshot_offsets);

        // D3hot → D0 cycle
        if pci_discovery::set_pci_power_state(&bdf, pci_discovery::PciPmState::D3Hot).is_ok() {
            std::thread::sleep(std::time::Duration::from_millis(100));
            let _ = pci_discovery::force_pci_d0(&bdf);
            std::thread::sleep(std::time::Duration::from_millis(50));

            let after = bar_cartography::snapshot_registers(self.bar0, &snapshot_offsets);
            let deltas = bar_cartography::diff_snapshots(&before, &after);

            if deltas.is_empty() {
                bounds
                    .d3hot_survives
                    .push("All snapshotted registers survived".into());
            } else {
                for (off, v_before, v_after) in &deltas {
                    bounds
                        .d3hot_lost
                        .push(format!("{off:#08x}: {v_before:#010x} → {v_after:#010x}"));
                }
                let survived = snapshot_offsets.len() - deltas.len();
                bounds.d3hot_survives.push(format!(
                    "{survived}/{} registers survived",
                    snapshot_offsets.len()
                ));
            }
        }

        // Clock gate test: toggle PMC enable bit 8 (PFIFO)
        let pmc_off = self.pmc_enable_off();
        let pmc_val = self.r(pmc_off);
        let pfifo_bit: u32 = 1 << 8;

        let before_cg = bar_cartography::snapshot_registers(self.bar0, &snapshot_offsets);
        self.w(pmc_off, pmc_val & !pfifo_bit);
        std::thread::sleep(std::time::Duration::from_millis(50));
        self.w(pmc_off, pmc_val | pfifo_bit);
        std::thread::sleep(std::time::Duration::from_millis(50));

        let after_cg = bar_cartography::snapshot_registers(self.bar0, &snapshot_offsets);
        let cg_deltas = bar_cartography::diff_snapshots(&before_cg, &after_cg);

        if cg_deltas.is_empty() {
            bounds
                .clock_gate_survives
                .push("All registers survived PFIFO clock gate".into());
        } else {
            for (off, v_before, v_after) in &cg_deltas {
                bounds
                    .clock_gate_lost
                    .push(format!("{off:#08x}: {v_before:#010x} → {v_after:#010x}"));
            }
        }

        bounds
    }
}
