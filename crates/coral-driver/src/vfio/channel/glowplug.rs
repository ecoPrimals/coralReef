// SPDX-License-Identifier: AGPL-3.0-only
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

use std::os::fd::RawFd;

use crate::vfio::bar_cartography;
use crate::vfio::device::MappedBar;
use crate::vfio::gpu_vendor::{GpuMetal, PowerBounds};
use crate::vfio::memory::{MemoryTopology, PraminRegion, MemoryRegion};
use crate::vfio::pci_discovery;

use super::devinit;
use super::diagnostic::interpreter::memory_probe;
use super::oracle::{DigitalPmu, OracleState};
use super::pfifo as pfifo_init;
use super::pri_monitor::PriBusMonitor;
use super::registers::*;

/// Register ranges that are meaningful for HBM2/FB initialization.
/// These are the domains we read from an oracle (nouveau-warm) card and
/// apply to a cold VFIO card to replicate the trained memory controller state.
const ORACLE_RANGES: &[(&str, usize, usize)] = &[
    ("PMC",       0x000000, 0x001000),
    ("PBUS",      0x001000, 0x002000),
    ("PTOP",      0x022000, 0x023000),
    ("PFB",       0x100000, 0x102000),
    ("FBPA0",     0x9A0000, 0x9A1000),
    ("FBPA1",     0x9A4000, 0x9A5000),
    ("FBPA_BC",   0x9A8000, 0x9A9000),
    ("LTC",       0x17E000, 0x17F000),
    ("PCLOCK",    0x137000, 0x138000),
    ("PMU",       0x10A000, 0x10B000),
    ("PFB_NISO",  0x100C00, 0x100E00),
    ("PMEM",      0x1FA000, 0x1FB000),
    ("FUSE",      0x021000, 0x022000),
    ("FBHUB",     0x100800, 0x100A00),
    ("PRI_MASTER",0x122000, 0x123000),
];

/// Registers to NEVER write (triggers, invalidations, dynamic counters).
fn is_dangerous_register(off: usize) -> bool {
    matches!(off,
        0x009000..=0x0090FF |  // PTIMER — dynamic
        0x610000..=0x610FFF |  // PDISP — display engine
        0x100CBC | 0x100CB8 | 0x100CEC |  // MMU invalidation triggers
        0x100E24..=0x100E54 |  // Fault buffer registers
        0x10A040..=0x10A048 |  // PMU mailboxes — dynamic
        0x10A100             // PMU CPUCTL — don't stop the PMU
    )
}

/// Current state of the GPU as understood by the glowplug.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GpuThermalState {
    /// GPU is in D3hot (PCIe sleep) — BAR0 reads 0xFFFFFFFF.
    D3Hot,
    /// GPU responds to BAR0 but engines are clock-gated (PMC = 0x40000020).
    ColdGated,
    /// PMC engines clocked but PFIFO not functional.
    EnginesClocked,
    /// PFIFO alive but VRAM returns error patterns (FB controller not initialized).
    PfifoAliveVramDead,
    /// PFIFO alive, VRAM accessible, BAR2 not configured.
    VramAliveBar2Dead,
    /// Fully warm — PFIFO, VRAM, BAR2 all functional.
    Warm,
}

/// A register snapshot taken before/after a warm-up step.
#[derive(Debug, Clone)]
pub struct StepSnapshot {
    /// Step description.
    pub step: String,
    /// Register values before the step: (offset, value).
    pub before: Vec<(usize, u32)>,
    /// Register values after the step: (offset, value).
    pub after: Vec<(usize, u32)>,
}

impl StepSnapshot {
    /// Registers that changed during this step.
    pub fn deltas(&self) -> Vec<(usize, u32, u32)> {
        bar_cartography::diff_snapshots(&self.before, &self.after)
    }
}

/// Result of a glowplug warm-up attempt.
#[derive(Debug)]
pub struct WarmResult {
    /// State before warm-up.
    pub initial_state: GpuThermalState,
    /// State after warm-up.
    pub final_state: GpuThermalState,
    /// Whether the GPU reached a fully warm state.
    pub success: bool,
    /// Memory topology after warm-up (if probed).
    pub memory: Option<MemoryTopology>,
    /// Diagnostic messages collected during warm-up.
    pub log: Vec<String>,
    /// Per-step register snapshots for forensics.
    pub step_snapshots: Vec<StepSnapshot>,
}

/// The GlowPlug — sovereign GPU warm-up engine.
///
/// Accepts an optional `GpuMetal` trait object for vendor-agnostic register
/// access. When provided, the warm-up sequence uses vendor-specific register
/// maps instead of hardcoded NVIDIA offsets. Falls back to NVIDIA Volta
/// defaults when no metal is set (backward compatibility).
pub struct GlowPlug<'a> {
    bar0: &'a MappedBar,
    container_fd: RawFd,
    /// PCI BDF string for sysfs access (e.g., "0000:4a:00.0").
    /// If set, enables VBIOS-based PMU devinit for HBM2 training.
    bdf: Option<String>,
    /// BDF of an oracle card (same GPU model, running nouveau) for register cloning.
    oracle_bdf: Option<String>,
    /// Vendor-agnostic GPU metal interface (optional).
    metal: Option<Box<dyn GpuMetal>>,
    /// Pre-loaded oracle state for digital PMU emulation.
    /// When set, the warm-up sequence includes an oracle-informed root PLL
    /// programming step before attempting other VRAM strategies.
    oracle_state: Option<OracleState>,
}

impl<'a> GlowPlug<'a> {
    pub fn new(bar0: &'a MappedBar, container_fd: RawFd) -> Self {
        Self { bar0, container_fd, bdf: None, oracle_bdf: None, metal: None, oracle_state: None }
    }

    /// Create a GlowPlug with BDF for VBIOS access.
    /// This enables the sovereign PMU devinit path for HBM2 training.
    pub fn with_bdf(bar0: &'a MappedBar, container_fd: RawFd, bdf: &str) -> Self {
        Self { bar0, container_fd, bdf: Some(bdf.to_string()), oracle_bdf: None, metal: None, oracle_state: None }
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
    /// Call before `warm()` to enable digital PMU emulation.
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
    ///
    /// When set, the warm-up sequence uses register offsets from the metal
    /// trait instead of hardcoded NVIDIA Volta defaults.
    pub fn with_metal(mut self, metal: Box<dyn GpuMetal>) -> Self {
        self.metal = Some(metal);
        self
    }

    fn r(&self, reg: usize) -> u32 {
        self.bar0.read_u32(reg).unwrap_or(0xDEAD_DEAD)
    }

    fn w(&self, reg: usize, val: u32) {
        let _ = self.bar0.write_u32(reg, val);
    }

    /// PRI backpressure check — probe all HBM2-critical domains and report health.
    ///
    /// Returns (alive_count, faulted_count, log_messages).
    /// If the bus is faulted, attempts recovery before returning.
    pub fn check_pri_health(&self) -> (usize, usize, Vec<String>) {
        let mut monitor = PriBusMonitor::new(self.bar0);
        let health = monitor.probe_all_domains();
        let mut log = Vec::new();

        let alive = health.iter().filter(|(_, _, h)| matches!(h, super::pri_monitor::DomainHealth::Alive)).count();
        let faulted = health.iter().filter(|(_, _, h)| matches!(h, super::pri_monitor::DomainHealth::Faulted { .. })).count();

        for (name, off, h) in &health {
            match h {
                super::pri_monitor::DomainHealth::Alive => {
                    log.push(format!("  PRI {name} ({off:#08x}): ALIVE"));
                }
                super::pri_monitor::DomainHealth::Faulted { fault_count, last_error } => {
                    log.push(format!(
                        "  PRI {name} ({off:#08x}): FAULTED ({fault_count}x, last={last_error:#010x})"
                    ));
                }
                super::pri_monitor::DomainHealth::Skipped => {
                    log.push(format!("  PRI {name} ({off:#08x}): SKIPPED"));
                }
            }
        }

        if faulted > 0 {
            log.push(format!("  PRI bus: {alive} alive, {faulted} faulted — attempting recovery..."));
            let recovered = monitor.attempt_recovery();
            if recovered {
                log.push("  PRI bus: recovery successful (BOOT0 reads clean)".into());
            } else {
                log.push("  PRI bus: recovery failed — bus may be locked".into());
            }
        } else {
            log.push(format!("  PRI bus: all {alive} probed domains alive"));
        }

        (alive, faulted, log)
    }

    /// Attempt PRI bus recovery without full health probe.
    fn recover_pri_bus(&self) -> bool {
        let mut monitor = PriBusMonitor::new(self.bar0);
        monitor.attempt_recovery()
    }

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
            let bar2_valid = bar2_block != 0x4000_0000 && bar2_block != 0 && (bar2_block >> 16) != 0xBAD0;
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

    /// Vendor-agnostic BOOT0 register offset.
    fn boot0_off(&self) -> usize {
        self.metal.as_ref().map_or(misc::BOOT0, |m| m.boot0_offset())
    }

    /// Vendor-agnostic PMC enable register offset.
    fn pmc_enable_off(&self) -> usize {
        self.metal.as_ref().map_or(pmc::ENABLE, |m| m.pmc_enable_offset())
    }

    /// Vendor-agnostic PBDMA map register offset.
    fn pbdma_map_off(&self) -> Option<usize> {
        self.metal
            .as_ref()
            .map_or(Some(pfifo::PBDMA_MAP), |m| m.pbdma_map_offset())
    }

    /// Vendor-agnostic BAR2 block register offset.
    fn bar2_block_off(&self) -> Option<usize> {
        self.metal
            .as_ref()
            .map_or(Some(misc::PBUS_BAR2_BLOCK), |m| m.bar2_block_offset())
    }

    /// Key register offsets to snapshot before/after each warm-up step.
    fn snapshot_offsets(&self) -> Vec<usize> {
        let mut offsets = vec![
            self.boot0_off(),
            self.pmc_enable_off(),
        ];
        if let Some(pbdma) = self.pbdma_map_off() {
            offsets.push(pbdma);
        }
        if let Some(bar2) = self.bar2_block_off() {
            offsets.push(bar2);
        }
        // PFIFO_ENABLE, PTIMER, devinit status, PRAMIN window
        offsets.extend_from_slice(&[
            0x2200, // PFIFO_ENABLE
            0x9000, // PTIMER_0
            0x2240C, // devinit status
            0x1700, // BAR0_WINDOW
        ]);
        if let Some(ref metal) = self.metal {
            for domain in metal.power_domains() {
                if let Some(reg) = domain.enable_reg {
                    if !offsets.contains(&reg) {
                        offsets.push(reg);
                    }
                }
            }
        }
        offsets
    }

    /// Take a snapshot of key registers.
    fn snap(&self) -> Vec<(usize, u32)> {
        bar_cartography::snapshot_registers(self.bar0, &self.snapshot_offsets())
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
                if let Some(reg) = domain.enable_reg {
                    if !offsets.contains(&reg) {
                        offsets.push(reg);
                    }
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
                bounds.d3hot_survives.push("All snapshotted registers survived".into());
            } else {
                for (off, v_before, v_after) in &deltas {
                    bounds.d3hot_lost.push(format!(
                        "{off:#08x}: {v_before:#010x} → {v_after:#010x}"
                    ));
                }
                let survived = snapshot_offsets.len() - deltas.len();
                bounds.d3hot_survives.push(format!(
                    "{survived}/{} registers survived", snapshot_offsets.len()
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
            bounds.clock_gate_survives.push("All registers survived PFIFO clock gate".into());
        } else {
            for (off, v_before, v_after) in &cg_deltas {
                bounds.clock_gate_lost.push(format!(
                    "{off:#08x}: {v_before:#010x} → {v_after:#010x}"
                ));
            }
        }

        bounds
    }

    /// Clone register state from an oracle (nouveau-warm) card of the same model.
    ///
    /// Opens the oracle's BAR0 via sysfs `resource0` (must be readable),
    /// reads all registers in [`ORACLE_RANGES`], compares with the cold card,
    /// and applies differences. Returns `(applied, stuck, total_diff)` counts.
    pub fn apply_oracle_registers(&self, log: &mut Vec<String>) -> (usize, usize, usize) {
        let oracle_bdf = match &self.oracle_bdf {
            Some(b) => b.clone(),
            None => {
                log.push("oracle: no oracle BDF configured".into());
                return (0, 0, 0);
            }
        };

        let resource0_path = format!("/sys/bus/pci/devices/{oracle_bdf}/resource0");
        let oracle_file = match std::fs::OpenOptions::new().read(true).open(&resource0_path) {
            Ok(f) => f,
            Err(e) => {
                log.push(format!("oracle: cannot open {resource0_path}: {e}"));
                return (0, 0, 0);
            }
        };

        use std::os::unix::io::AsRawFd;
        let bar0_size: usize = 16 * 1024 * 1024;
        let oracle_ptr = unsafe {
            rustix::mm::mmap(
                std::ptr::null_mut(),
                bar0_size,
                rustix::mm::ProtFlags::READ,
                rustix::mm::MapFlags::SHARED,
                &oracle_file,
                0,
            )
        };
        let oracle_ptr = match oracle_ptr {
            Ok(p) => p,
            Err(e) => {
                log.push(format!("oracle: mmap failed: {e}"));
                return (0, 0, 0);
            }
        };

        let oracle_read = |offset: usize| -> u32 {
            assert!(offset + 4 <= bar0_size);
            unsafe { std::ptr::read_volatile(oracle_ptr.cast::<u8>().add(offset).cast::<u32>()) }
        };

        // Verify oracle is the same GPU
        let oracle_boot0 = oracle_read(0);
        let cold_boot0 = self.r(misc::BOOT0);
        if oracle_boot0 != cold_boot0 {
            log.push(format!(
                "oracle: BOOT0 mismatch! oracle={oracle_boot0:#010x} cold={cold_boot0:#010x}"
            ));
            unsafe { let _ = rustix::mm::munmap(oracle_ptr, bar0_size); }
            return (0, 0, 0);
        }

        log.push(format!(
            "oracle: reading {} ranges from {oracle_bdf} (BOOT0={oracle_boot0:#010x})",
            ORACLE_RANGES.len()
        ));

        // Collect all diffs
        let mut diffs: Vec<(usize, u32, u32)> = Vec::new();
        for &(name, start, end) in ORACLE_RANGES {
            let mut range_diffs = 0;
            for off in (start..end).step_by(4) {
                let ov = oracle_read(off);
                let cv = self.r(off);
                // Skip if both are error patterns or identical
                if ov == cv { continue; }
                if ov == 0xFFFFFFFF || ov == 0xDEADDEAD { continue; }
                if (ov & 0xFFFF0000) == 0xBADF0000 { continue; } // PRI error on oracle
                diffs.push((off, ov, cv));
                range_diffs += 1;
            }
            if range_diffs > 0 {
                log.push(format!("oracle: {name}: {range_diffs} diffs"));
            }
        }

        let total_diff = diffs.len();
        log.push(format!("oracle: total {total_diff} register differences"));

        // Apply oracle values with PRI backpressure monitoring
        let mut applied = 0;
        let mut stuck = 0;
        let mut pri_skipped = 0;
        let mut monitor = PriBusMonitor::new(self.bar0).with_fault_threshold(5);

        for &(off, ov, _cv) in &diffs {
            if is_dangerous_register(off) { continue; }
            match monitor.write_u32(off, ov) {
                super::pri_monitor::WriteOutcome::Applied => applied += 1,
                super::pri_monitor::WriteOutcome::SkippedFaulted
                | super::pri_monitor::WriteOutcome::Throttled => pri_skipped += 1,
                super::pri_monitor::WriteOutcome::AppliedButFaulted => {
                    applied += 1;
                    if applied % 20 == 0 {
                        monitor.attempt_recovery();
                    }
                }
            }
        }

        if pri_skipped > 0 {
            log.push(format!("oracle: {pri_skipped} writes PRI-skipped (domain faulted)"));
        }

        std::thread::sleep(std::time::Duration::from_millis(50));

        // Verify a sample of writes
        for &(off, ov, _) in diffs.iter().take(100) {
            if is_dangerous_register(off) { continue; }
            let rb = self.r(off);
            if rb != ov {
                stuck += 1;
                if stuck <= 10 {
                    log.push(format!(
                        "oracle: STUCK [{off:#010x}] wrote={ov:#010x} rb={rb:#010x}"
                    ));
                }
            }
        }

        log.push(format!(
            "oracle: applied {applied}, stuck {stuck}, total_diff {total_diff}"
        ));

        unsafe { let _ = rustix::mm::munmap(oracle_ptr, bar0_size); }
        (applied, stuck, total_diff)
    }

    /// Full warm-up sequence — bring the GPU from any state to Warm.
    pub fn warm(&self) -> WarmResult {
        let mut log = Vec::new();
        let mut step_snapshots = Vec::new();
        let initial_state = self.check_state();
        log.push(format!("initial state: {initial_state:?}"));

        if initial_state == GpuThermalState::Warm {
            return WarmResult {
                initial_state,
                final_state: GpuThermalState::Warm,
                success: true,
                memory: None,
                log,
                step_snapshots,
            };
        }

        if initial_state == GpuThermalState::D3Hot {
            if let Some(bdf) = &self.bdf {
                log.push("step 0: GPU in D3hot — forcing D0 via PCI PMCSR write".into());
                let before_d0 = self.snap();
                match devinit::force_pci_d0(bdf) {
                    Ok(()) => {
                        std::thread::sleep(std::time::Duration::from_millis(50));
                        let after_d0 = self.snap();
                        step_snapshots.push(StepSnapshot {
                            step: "D3hot → D0 force".into(),
                            before: before_d0,
                            after: after_d0,
                        });
                        let post_d0 = self.check_state();
                        log.push(format!("  After D0 force: {post_d0:?}"));
                        if post_d0 == GpuThermalState::D3Hot {
                            log.push("  D0 force failed — device still in D3hot.".into());
                        }
                    }
                    Err(e) => log.push(format!("  D0 force failed: {e}")),
                }
            } else {
                log.push("GPU in D3hot — no BDF set, cannot force D0.".into());
            }

            if self.check_state() == GpuThermalState::D3Hot {
                return WarmResult {
                    initial_state,
                    final_state: GpuThermalState::D3Hot,
                    success: false,
                    memory: None,
                    log,
                    step_snapshots,
                };
            }
        }

        // Step 1: PMC_ENABLE — clock all engines
        // Uses metal trait for register offset when available.
        if matches!(
            initial_state,
            GpuThermalState::ColdGated | GpuThermalState::EnginesClocked
        ) {
            let pmc_off = self.pmc_enable_off();
            let before_pmc = self.snap();

            // If we have a metal trait, execute its warmup sequence
            if let Some(ref metal) = self.metal {
                let steps = metal.warmup_sequence();
                for (i, step) in steps.iter().enumerate() {
                    log.push(format!("step 1.{i}: {}", step.description));
                    let step_before = self.snap();
                    for w in &step.writes {
                        if let Some(mask) = w.mask {
                            let cur = self.r(w.offset);
                            self.w(w.offset, (cur & mask) | w.value);
                        } else {
                            self.w(w.offset, w.value);
                        }
                    }
                    if step.delay_ms > 0 {
                        std::thread::sleep(std::time::Duration::from_millis(step.delay_ms));
                    }
                    let step_after = self.snap();
                    step_snapshots.push(StepSnapshot {
                        step: step.description.clone(),
                        before: step_before,
                        after: step_after,
                    });
                    for v in &step.verify {
                        let val = self.r(v.offset);
                        let ok = (val & v.mask) == (v.expected & v.mask);
                        log.push(format!("  verify {:#x}: {val:#010x} (ok={ok})", v.offset));
                    }
                }
            } else {
                // Fallback: NVIDIA Volta hardcoded sequence
                log.push("step 1: PMC_ENABLE = 0xFFFFFFFF".into());
                self.w(pmc_off, 0xFFFF_FFFF);
                std::thread::sleep(std::time::Duration::from_millis(50));

                let after_pmc = self.snap();
                step_snapshots.push(StepSnapshot {
                    step: "PMC_ENABLE = 0xFFFFFFFF".into(),
                    before: before_pmc,
                    after: after_pmc,
                });
                let pmc_after = self.r(pmc_off);
                log.push(format!("  PMC_ENABLE after: {pmc_after:#010x}"));
            }
        }

        // Step 2: PFIFO reset cycle (bit 8)
        let state_after_pmc = self.check_state();
        if matches!(
            state_after_pmc,
            GpuThermalState::ColdGated | GpuThermalState::EnginesClocked
        ) {
            let pmc_off = self.pmc_enable_off();
            log.push("step 2: PFIFO reset cycle (PMC bit 8)".into());
            let before_pfifo = self.snap();
            let pmc_cur = self.r(pmc_off);
            let pfifo_bit: u32 = 1 << 8;
            self.w(pmc_off, pmc_cur & !pfifo_bit);
            std::thread::sleep(std::time::Duration::from_millis(20));
            self.w(pmc_off, pmc_cur | pfifo_bit);
            std::thread::sleep(std::time::Duration::from_millis(50));
            let after_pfifo = self.snap();
            step_snapshots.push(StepSnapshot {
                step: "PFIFO reset cycle".into(),
                before: before_pfifo,
                after: after_pfifo,
            });

            if let Some(pbdma_off) = self.pbdma_map_off() {
                let pbdma_after = self.r(pbdma_off);
                log.push(format!("  PBDMA_MAP after: {pbdma_after:#010x}"));
            }
        }

        // Step 2.5: PRI bus health check — detect faulted domains before VRAM strategies
        {
            let (alive, faulted, pri_log) = self.check_pri_health();
            log.extend(pri_log);
            step_snapshots.push(StepSnapshot {
                step: format!("PRI health check: {alive} alive, {faulted} faulted"),
                before: self.snap(),
                after: self.snap(),
            });
        }

        // Step 2.75: Clock gating sweep — disable BLCG/SLCG/ELCG to ungate faulted domains.
        //
        // Domains reporting 0xBADF1100 (BLCG/SLCG gated) or 0xBADF3000 (hub clock gate)
        // need their clock gating disabled before they'll respond to PRI accesses.
        // This must happen BEFORE any VRAM/HBM2 strategies since those need live FBPA/LTC/PCLOCK.
        {
            let before_cg = self.snap();
            let mut cg_log = Vec::new();
            cg_log.push("step 2.75: Clock gating sweep — disabling CG on faulted domains".into());

            // Phase 1: Sweep all known CG control registers
            for &(offset, name) in cg::CG_SWEEP_TARGETS {
                let old = self.r(offset);
                let is_error = super::registers::pri::is_pri_error(old);
                if is_error {
                    cg_log.push(format!("  {name} [{offset:#08x}]: PRI error {old:#010x} — domain unreachable"));
                } else {
                    self.w(offset, cg::CG_DISABLE);
                    let new = self.r(offset);
                    if old != new {
                        cg_log.push(format!("  {name} [{offset:#08x}]: {old:#010x} → {new:#010x}"));
                    }
                }
            }

            // Phase 2: Per-FBPA clock gating disable
            for i in 0..cg::FBPA_COUNT {
                let base = cg::FBPA0_BASE + i * cg::FBPA_STRIDE;
                let cg_reg = base + cg::FBPA_CG_OFFSET;
                let old = self.r(cg_reg);
                if super::registers::pri::is_pri_error(old) {
                    cg_log.push(format!(
                        "  FBPA{i} CG [{cg_reg:#08x}]: PRI error {old:#010x}"
                    ));
                    // Try writing anyway — sometimes the CG register itself is accessible
                    // even when other FBPA registers are gated
                    self.w(cg_reg, cg::CG_DISABLE);
                } else {
                    self.w(cg_reg, cg::CG_DISABLE);
                    let new = self.r(cg_reg);
                    if old != new {
                        cg_log.push(format!(
                            "  FBPA{i} CG [{cg_reg:#08x}]: {old:#010x} → {new:#010x}"
                        ));
                    }
                }
            }

            // Phase 3: Per-LTC clock gating disable
            for i in 0..cg::LTC_COUNT {
                let base = cg::LTC0_BASE + i * cg::LTC_STRIDE;
                let cg_reg = base + cg::LTC_CG_OFFSET;
                let old = self.r(cg_reg);
                if super::registers::pri::is_pri_error(old) {
                    // Write CG disable even on error — might unlock the domain
                    self.w(cg_reg, cg::CG_DISABLE);
                } else {
                    self.w(cg_reg, cg::CG_DISABLE);
                    let new = self.r(cg_reg);
                    if old != new {
                        cg_log.push(format!(
                            "  LTC{i} CG [{cg_reg:#08x}]: {old:#010x} → {new:#010x}"
                        ));
                    }
                }
            }

            // Phase 4: PRI recovery after CG sweep (clear any faults from probing)
            let recovered = self.recover_pri_bus();
            std::thread::sleep(std::time::Duration::from_millis(50));

            // Phase 5: Re-probe domains to see what came alive
            let (alive2, faulted2, probe_log) = self.check_pri_health();
            cg_log.push(format!(
                "  Post-CG sweep: {alive2} alive, {faulted2} faulted (recovery={})",
                if recovered { "ok" } else { "failed" }
            ));
            cg_log.extend(probe_log.into_iter().map(|l| format!("    {l}")));

            // Phase 6: PCLOCK PLL probe — read PLL status registers
            cg_log.push("  PCLOCK PLL probe:".into());
            let pclock_base = 0x0013_7000_usize;
            for &(off, name) in &[
                (0x000, "PCLOCK_CTL"),
                (0x004, "PCLOCK_STATUS"),
                (0x008, "PCLOCK_COEFF"),
                (0x010, "PCLOCK_PLL0"),
                (0x014, "PCLOCK_PLL1"),
                (0x020, "PCLOCK_BYPASS"),
                (0x050, "NVPLL_CTL"),
                (0x054, "NVPLL_COEFF"),
                (0x100, "MEMPLL_CTL"),
                (0x104, "MEMPLL_COEFF"),
            ] {
                let reg = pclock_base + off;
                let val = self.r(reg);
                let err = if super::registers::pri::is_pri_error(val) {
                    format!(" ← {}", super::registers::pri::decode_pri_error(val))
                } else {
                    String::new()
                };
                cg_log.push(format!("    {name} [{reg:#08x}] = {val:#010x}{err}"));
            }

            let after_cg = self.snap();
            step_snapshots.push(StepSnapshot {
                step: "Clock gating sweep + PLL probe".into(),
                before: before_cg,
                after: after_cg,
            });
            log.extend(cg_log);
        }

        // Step 2.9: Digital PMU emulation — oracle-informed root PLL programming.
        //
        // If oracle data is available (from a live nouveau card, BAR0 dump, or text dump),
        // use it to program registers in dependency order. The key insight: root PLLs at
        // 0x136xxx are in an always-on power domain. Writing oracle values there may cause
        // downstream clock gates (PCLOCK, FBPA, LTC) to open, enabling VRAM access without
        // signed PMU firmware.
        if self.oracle_state.is_some() || self.oracle_bdf.is_some() {
            let oracle_state = if let Some(ref state) = self.oracle_state {
                Some(state.clone())
            } else if let Some(ref obdf) = self.oracle_bdf {
                match OracleState::from_live_card(obdf) {
                    Ok(state) => Some(state),
                    Err(e) => {
                        log.push(format!("step 2.9: oracle load failed: {e}"));
                        None
                    }
                }
            } else {
                None
            };

            if let Some(ref oracle) = oracle_state {
                log.push(format!(
                    "step 2.9: Digital PMU — {} oracle registers from {}",
                    oracle.registers.len(), oracle.source,
                ));
                let before_dpmu = self.snap();

                let mut dpmu = DigitalPmu::new(self.bar0, oracle);

                // Phase 1: Program root PLLs (always-on domain)
                let (pll_applied, pll_skipped) = dpmu.program_root_plls();
                log.extend(dpmu.take_log());

                if pll_applied > 0 {
                    // Phase 2: Program PCLOCK bypass registers
                    let bypass_log = dpmu.program_pclock_bypass();
                    log.extend(bypass_log);

                    // Phase 3: Check if PCLOCK opened
                    let pclock_val = self.r(0x137000);
                    let pclock_alive = !super::registers::pri::is_pri_error(pclock_val);
                    log.push(format!(
                        "  Post-PLL: PCLOCK={pclock_val:#010x} ({})",
                        if pclock_alive { "ALIVE" } else { "still gated" }
                    ));

                    // Phase 4: If PCLOCK came alive, run full digital PMU
                    if pclock_alive || pll_applied > 10 {
                        log.push("  Running full digital PMU sequence...".into());
                        let result = dpmu.execute();
                        log.extend(result.log);

                        if result.vram_unlocked {
                            log.push(format!(
                                "  *** DIGITAL PMU UNLOCKED VRAM after {:?}! ***",
                                result.vram_unlocked_after,
                            ));
                        } else {
                            log.push(format!(
                                "  Digital PMU: {} applied, {} stuck, {} PRI-skipped — VRAM still dead",
                                result.applied, result.stuck, result.pri_skipped,
                            ));
                        }
                    }
                }

                let after_dpmu = self.snap();
                step_snapshots.push(StepSnapshot {
                    step: format!("Digital PMU ({pll_applied} PLLs, {pll_skipped} skipped)"),
                    before: before_dpmu,
                    after: after_dpmu,
                });

                // PRI recovery after digital PMU
                self.recover_pri_bus();
            }
        }

        // Step 3: Check VRAM — if dead, attempt HBM2 training
        let state_after_pfifo = self.check_state();
        if state_after_pfifo == GpuThermalState::PfifoAliveVramDead {
            let devinit_status = devinit::DevinitStatus::probe(self.bar0);
            log.push(format!(
                "step 3: VRAM dead — devinit_reg={:#010x} needs_post={}",
                devinit_status.devinit_reg, devinit_status.needs_post
            ));

            // Strategy 1: D3cold power cycle (triggers boot ROM devinit naturally)
            if devinit_status.needs_post {
                if let Some(bdf) = &self.bdf {
                    log.push("step 3a: Attempting D3cold power cycle (boot ROM devinit)".into());
                    match devinit::pci_power_cycle_devinit(bdf) {
                        Ok(true) => {
                            std::thread::sleep(std::time::Duration::from_millis(500));
                            log.push("  Power cycle complete — re-checking devinit...".into());
                            let post_status = devinit::DevinitStatus::probe(self.bar0);
                            if !post_status.needs_post && self.check_vram() {
                                log.push("  *** DEVINIT COMPLETE + VRAM ALIVE! ***".into());
                            } else {
                                log.push(format!(
                                    "  Post-cycle: devinit={:#010x} needs_post={} vram={}",
                                    post_status.devinit_reg, post_status.needs_post,
                                    if self.check_vram() { "alive" } else { "dead" }
                                ));
                            }
                        }
                        Ok(false) => log.push("  D3cold power cycle returned false.".into()),
                        Err(e) => log.push(format!("  D3cold power cycle failed: {e}")),
                    }
                }
            }

            // Strategy 2: VBIOS script register writes (apply known writes from scripts)
            if !self.check_vram() {
                log.push("step 3b: Scanning VBIOS boot scripts for register writes".into());
                let rom_result = devinit::read_vbios_prom(self.bar0)
                    .or_else(|e1| {
                        log.push(format!("  PROM read failed: {e1}"));
                        if let Some(bdf) = &self.bdf {
                            devinit::read_vbios_sysfs(bdf)
                        } else {
                            Err("no BDF for sysfs fallback".into())
                        }
                    });
                if let Ok(rom) = rom_result {
                    match devinit::extract_boot_script_writes(&rom) {
                        Ok(writes) => {
                            log.push(format!("  Found {} register writes in VBIOS scripts", writes.len()));
                            let mut applied = 0;
                            for w in &writes {
                                if is_dangerous_register(w.reg as usize) { continue; }
                                if let Some(mask) = w.mask {
                                    let cur = self.r(w.reg as usize);
                                    let new_val = (cur & mask) | w.value;
                                    self.w(w.reg as usize, new_val);
                                } else {
                                    self.w(w.reg as usize, w.value);
                                }
                                applied += 1;
                            }
                            log.push(format!("  Applied {applied}/{} script register writes", writes.len()));
                            std::thread::sleep(std::time::Duration::from_millis(100));
                            if self.check_vram() {
                                log.push("  *** VBIOS SCRIPT WRITES UNLOCKED VRAM! ***".into());
                            } else {
                                log.push("  VRAM still dead after script writes.".into());
                            }
                        }
                        Err(e) => log.push(format!("  Script extraction failed: {e}")),
                    }
                }
            }

            // PRI bus recovery between strategies — clear any faults from script writes
            if !self.check_vram() {
                let recovered = self.recover_pri_bus();
                log.push(format!(
                    "  PRI recovery between strategies: {}",
                    if recovered { "success" } else { "bus still faulted" }
                ));
            }

            // Strategy 2b: Sovereign HBM2 training via typestate controller
            if !self.check_vram() {
                log.push("step 3b2: Attempting sovereign HBM2 training".into());
                use super::hbm2_training::{self as hbm2, Hbm2Controller, Untrained};
                let ctrl = Hbm2Controller::<Untrained>::new(
                    self.bar0,
                    self.bdf.as_deref(),
                    hbm2::volta_hbm2::FBPA_COUNT,
                );
                match ctrl.enable_phy()
                    .and_then(|c| c.train_links())
                    .and_then(|c| c.init_dram())
                    .and_then(|c| c.verify_vram())
                {
                    Ok(verified) => {
                        let tlog = verified.training_log();
                        log.push(format!(
                            "  *** SOVEREIGN HBM2 TRAINING SUCCEEDED ({} writes) ***",
                            tlog.write_count(),
                        ));
                    }
                    Err(phase_err) => {
                        log.push(format!("  HBM2 training failed at {}: {}", phase_err.phase, phase_err.detail));
                        if !phase_err.register_snapshot.is_empty() {
                            for (off, val) in &phase_err.register_snapshot {
                                log.push(format!("    [{off:#010x}] = {val:#010x}"));
                            }
                        }
                    }
                }
            }

            // PRI bus recovery before enhanced devinit
            if !self.check_vram() {
                self.recover_pri_bus();
            }

            // Strategy 2c: Enhanced devinit with auto VBIOS source + fallback
            if !self.check_vram() {
                log.push("step 3b3: Enhanced devinit with diagnostics".into());
                match devinit::execute_devinit_with_diagnostics(self.bar0, self.bdf.as_deref()) {
                    Ok(true) => {
                        std::thread::sleep(std::time::Duration::from_millis(100));
                        if self.check_vram() {
                            log.push("  *** ENHANCED DEVINIT UNLOCKED VRAM! ***".into());
                        } else {
                            log.push("  Enhanced devinit returned true but VRAM still dead.".into());
                        }
                    }
                    Ok(false) => log.push("  Devinit reports already complete.".into()),
                    Err(e) => log.push(format!("  Enhanced devinit failed: {e}")),
                }
            }

            // PRI bus recovery before oracle strategies
            if self.oracle_bdf.is_some() && !self.check_vram() {
                let recovered = self.recover_pri_bus();
                log.push(format!(
                    "  PRI recovery before oracle: {}",
                    if recovered { "success" } else { "bus still faulted" }
                ));
            }

            // Strategy 3: Oracle register cloning (if available)
            if self.oracle_bdf.is_some() && !self.check_vram() {
                log.push("step 3c: Applying oracle register state".into());
                let (applied, stuck, total) = self.apply_oracle_registers(&mut log);
                if applied > 0 {
                    std::thread::sleep(std::time::Duration::from_millis(100));
                    if self.check_vram() {
                        log.push("  *** ORACLE REGISTERS UNLOCKED VRAM! ***".into());
                    } else {
                        log.push(format!(
                            "  Oracle: {applied}/{total} regs applied ({stuck} stuck) — VRAM still dead"
                        ));
                    }
                }
            }

            // Strategy 3b: Differential replay from oracle (domain-ordered)
            if self.oracle_bdf.is_some() && !self.check_vram() {
                log.push("step 3c2: Differential replay (domain-ordered) from oracle".into());
                if let Some(ref oracle_bdf) = self.oracle_bdf {
                    match super::hbm2_training::differential_training(self.bar0, oracle_bdf) {
                        Ok(result) => {
                            if result.success {
                                log.push(format!(
                                    "  *** DIFFERENTIAL REPLAY UNLOCKED VRAM after {} domain! ***",
                                    result.vram_unlocked_after.as_deref().unwrap_or("?"),
                                ));
                            } else {
                                log.push(format!(
                                    "  Differential replay: {} domains applied, VRAM still dead",
                                    result.domains_applied.len(),
                                ));
                            }
                        }
                        Err(e) => log.push(format!("  Differential replay failed: {e}")),
                    }
                }
            }

            // Strategy 4: PMU FALCON devinit (encrypted firmware upload attempt)
            if !self.check_vram() && devinit_status.needs_post {
                log.push("step 3d: Attempting PMU FALCON devinit (VBIOS + FALCON)".into());
                let rom_result = devinit::read_vbios_prom(self.bar0)
                    .or_else(|e1| {
                        log.push(format!("  PROM read failed: {e1}"));
                        if let Some(bdf) = &self.bdf {
                            devinit::read_vbios_sysfs(bdf)
                        } else {
                            Err("no BDF for sysfs fallback".into())
                        }
                    });
                if let Ok(rom) = rom_result {
                    match devinit::execute_devinit(self.bar0, &rom) {
                        Ok(true) => {
                            std::thread::sleep(std::time::Duration::from_millis(100));
                            if self.check_vram() {
                                log.push("  *** PMU DEVINIT SUCCEEDED + VRAM ALIVE! ***".into());
                            } else {
                                log.push("  PMU devinit completed but VRAM still dead.".into());
                            }
                        }
                        Ok(false) => log.push("  Devinit reports already complete.".into()),
                        Err(e) => log.push(format!("  PMU devinit failed: {e}")),
                    }
                }
            }

            // Strategy 5: Register-level FB init probe
            if !self.check_vram() {
                log.push("step 3e: Attempting register-level FB init probe...".into());
                let (topo, deltas) = memory_probe::attempt_fb_init(self.bar0, self.container_fd);
                if topo.vram_accessible {
                    log.push("  FB init probe succeeded! VRAM is accessible.".into());
                } else {
                    log.push(format!("  FB init probe: VRAM still dead ({} deltas)", deltas.len()));
                }
            }

            let pfb_regs = memory_probe::snapshot_pfb_registers(self.bar0);
            log.push(format!("  NV_PFB registers readable: {}", pfb_regs.len()));
        }

        // Step 4: BAR2 page tables (requires VRAM)
        let state_after_fb = self.check_state();
        if matches!(
            state_after_fb,
            GpuThermalState::VramAliveBar2Dead | GpuThermalState::Warm
        ) && state_after_fb == GpuThermalState::VramAliveBar2Dead
        {
            log.push("step 4: Setting up BAR2 page tables in VRAM".into());
            match pfifo_init::setup_bar2_page_table(self.bar0) {
                Ok(()) => {
                    log.push("  BAR2 page tables configured successfully.".into());
                    let bar2 = self.r(misc::PBUS_BAR2_BLOCK);
                    log.push(format!("  BAR2_BLOCK = {bar2:#010x}"));
                }
                Err(e) => {
                    log.push(format!("  BAR2 setup failed: {e}"));
                }
            }
        }

        // Step 5: Verify final state with full memory topology
        let final_state = self.check_state();
        let memory = Some(memory_probe::discover_memory_topology(
            self.bar0,
            self.container_fd,
        ));

        let success = final_state == GpuThermalState::Warm;
        log.push(format!("final state: {final_state:?} success={success}"));

        WarmResult {
            initial_state,
            final_state,
            success,
            memory,
            log,
            step_snapshots,
        }
    }

    /// Full initialization — warm + PFIFO interrupts + MMU fault buffers.
    /// This is what the interpreter needs before it can proceed to L4+.
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

        result.log.push("full_init: interrupts + fault buffers configured".into());
        result
    }

    /// Create a GlowPlug health listener that monitors domain health.
    ///
    /// Returns a snapshot of current PRI domain health + thermal state.
    /// Call periodically between operations to detect if the GPU is "cooling"
    /// (domains going faulted) and needs re-warming.
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
    ///
    /// Compares current state against a previous health snapshot. If domains
    /// have gone faulted or VRAM has become inaccessible, triggers PRI
    /// recovery and potentially a full re-warm.
    pub fn rewarm_if_cooled(&self, previous: &HealthSnapshot) -> Option<WarmResult> {
        let current = self.health_check();

        let cooled = current.domains_faulted > previous.domains_faulted
            || (!current.vram_accessible && previous.vram_accessible)
            || (current.thermal_state != previous.thermal_state
                && current.thermal_state != GpuThermalState::Warm);

        if cooled {
            eprintln!(
                "GlowPlug: GPU cooled! was {:?}/{} alive, now {:?}/{} alive",
                previous.thermal_state, previous.domains_alive,
                current.thermal_state, current.domains_alive,
            );
            // Try PRI recovery first (lightweight)
            self.recover_pri_bus();

            // If that didn't help, full re-warm
            let post_recovery = self.check_state();
            if post_recovery != GpuThermalState::Warm {
                return Some(self.warm());
            }
        }

        None
    }
}

/// Snapshot of GPU health at a point in time, for the listener/watchdog.
#[derive(Debug, Clone)]
pub struct HealthSnapshot {
    pub thermal_state: GpuThermalState,
    pub domains_alive: usize,
    pub domains_faulted: usize,
    pub vram_accessible: bool,
    pub pmc_enable: u32,
    pub log: Vec<String>,
}

impl std::fmt::Debug for GlowPlug<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GlowPlug")
            .field("state", &self.check_state())
            .finish()
    }
}
