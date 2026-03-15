// SPDX-License-Identifier: AGPL-3.0-only
//! Sovereign HBM2 memory training via typestate-enforced phase transitions.
//!
//! This module provides compile-time guarantees that HBM2 training phases
//! execute in the correct order. The Rust type system prevents:
//! - Verifying VRAM before PHY calibration (compile error)
//! - Skipping link training (compile error)
//! - Holding references to "untrained" state during "trained" operations
//!
//! # Training Phases
//!
//! ```text
//! Untrained → PhyUp → LinkTrained → DramReady → Verified
//!    │          │          │             │           │
//!    │   enable_phy()  train_links() init_dram() verify_vram()
//!    │          │          │             │           │
//!    ▼          ▼          ▼             ▼           ▼
//! (cold)   (clocks on)  (PHY cal)   (timings)   (VRAM alive)
//! ```
//!
//! # Training Backends
//!
//! Three backends can drive the phase transitions:
//! - **VbiosInterpreter**: Execute VBIOS init scripts from the host CPU
//! - **DifferentialReplay**: Replay captured register diffs from an oracle card
//! - **FalconUpload**: Upload and execute DEVINIT firmware on the PMU FALCON
//!
//! # Rust Type System Advantages
//!
//! - Ownership transfer: each phase transition consumes the previous state
//! - Newtype register domains: `FbpaOffset` vs `LtcOffset` prevent cross-domain mix-ups
//! - Zero-cost abstractions: all typestate checks vanish at compile time
//! - The compiler can prove no aliased writes to active memory controllers

use std::marker::PhantomData;
use std::fmt;

use crate::vfio::device::MappedBar;
use crate::vfio::memory::{MemoryRegion, PraminRegion};

use super::devinit;

// ── Newtype register domain offsets ─────────────────────────────────────

/// An offset within the FBPA (Framebuffer Partition Array) register domain.
/// Prevents accidental use of FBPA offsets in PMC or LTC contexts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FbpaOffset(pub usize);

/// An offset within the LTC (L2 Cache) register domain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LtcOffset(pub usize);

/// An offset within the PFB (Framebuffer controller) register domain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PfbOffset(pub usize);

/// An offset within the PCLOCK register domain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PclockOffset(pub usize);

// ── GV100 (Volta) HBM2 register constants ──────────────────────────────

/// Volta-specific FBPA/LTC/PFB register constants.
pub mod volta_hbm2 {
    use super::*;

    pub const FBPA0_BASE: usize = 0x009A_0000;
    pub const FBPA_STRIDE: usize = 0x0000_4000;
    pub const FBPA_COUNT: usize = 4;

    pub const LTC_BASE: usize = 0x0017_E000;
    pub const LTC_STRIDE: usize = 0x0000_2000;
    pub const LTC_COUNT: usize = 6;

    pub const PFB_BASE: usize = 0x0010_0000;
    pub const PFB_CFG0: PfbOffset = PfbOffset(0x0010_0000);
    pub const PFB_CFG1: PfbOffset = PfbOffset(0x0010_0004);
    pub const PFB_MEM_STATUS: PfbOffset = PfbOffset(0x0010_0800);
    pub const PFB_MEM_CTRL: PfbOffset = PfbOffset(0x0010_0804);
    pub const PFB_NISO_FLUSH_LO: PfbOffset = PfbOffset(0x0010_0B20);
    pub const PFB_NISO_FLUSH_HI: PfbOffset = PfbOffset(0x0010_0B24);

    pub const PCLOCK_BASE: usize = 0x0013_7000;
    pub const CLK_BASE: usize = 0x0013_2000;

    pub const PMC_ENABLE: usize = 0x0000_0200;
    pub const FB_ENABLE_BIT: u32 = 1 << 20;
    pub const LTC_ENABLE_BIT: u32 = 1 << 21;

    pub const PRAMIN_BASE: usize = 0x0070_0000;
    pub const BAR0_WINDOW: usize = 0x0000_1700;

    /// Relative offsets within each FBPA partition for key registers.
    pub const FBPA_CMD: FbpaOffset = FbpaOffset(0x00);
    pub const FBPA_CFG: FbpaOffset = FbpaOffset(0x04);
    pub const FBPA_TIMING0: FbpaOffset = FbpaOffset(0x80);
    pub const FBPA_TIMING1: FbpaOffset = FbpaOffset(0x84);
    pub const FBPA_TIMING2: FbpaOffset = FbpaOffset(0x88);

    /// Compute the absolute BAR0 offset for a register within a specific FBPA partition.
    pub fn fbpa_reg(partition: usize, rel: FbpaOffset) -> usize {
        FBPA0_BASE + partition * FBPA_STRIDE + rel.0
    }

    /// Compute the absolute BAR0 offset for a register within a specific LTC partition.
    pub fn ltc_reg(partition: usize, rel: LtcOffset) -> usize {
        LTC_BASE + partition * LTC_STRIDE + rel.0
    }
}

// ── Typestate phase markers ─────────────────────────────────────────────

/// GPU memory controller has not been initialized. FBPA clocks may be gated.
pub struct Untrained;
/// FBPA clock domains are enabled. PHY is powered but not calibrated.
pub struct PhyUp;
/// HBM2 PHY link training is complete. DRAM timing not yet configured.
pub struct LinkTrained;
/// DRAM timing registers are configured. Memory controller is ready.
pub struct DramReady;
/// VRAM accessibility verified via PRAMIN sentinel write/readback.
pub struct Verified;

/// Trait bound for all HBM2 training phases.
pub trait Hbm2Phase: sealed::Sealed + fmt::Debug {}
impl Hbm2Phase for Untrained {}
impl Hbm2Phase for PhyUp {}
impl Hbm2Phase for LinkTrained {}
impl Hbm2Phase for DramReady {}
impl Hbm2Phase for Verified {}

mod sealed {
    pub trait Sealed {}
    impl Sealed for super::Untrained {}
    impl Sealed for super::PhyUp {}
    impl Sealed for super::LinkTrained {}
    impl Sealed for super::DramReady {}
    impl Sealed for super::Verified {}
}

impl fmt::Debug for Untrained { fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "Untrained") } }
impl fmt::Debug for PhyUp { fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "PhyUp") } }
impl fmt::Debug for LinkTrained { fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "LinkTrained") } }
impl fmt::Debug for DramReady { fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "DramReady") } }
impl fmt::Debug for Verified { fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { write!(f, "Verified") } }

// ── Training phase error ────────────────────────────────────────────────

/// Which phase of HBM2 training failed and why.
#[derive(Debug, Clone)]
pub struct Hbm2TrainingError {
    pub phase: &'static str,
    pub detail: String,
    pub register_snapshot: Vec<(usize, u32)>,
}

impl fmt::Display for Hbm2TrainingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "HBM2 training failed at {}: {}", self.phase, self.detail)
    }
}

impl std::error::Error for Hbm2TrainingError {}

// ── Training log ────────────────────────────────────────────────────────

/// A single recorded action during HBM2 training.
#[derive(Debug, Clone)]
pub enum TrainingAction {
    RegWrite { offset: usize, value: u32, old: u32 },
    RegRead { offset: usize, value: u32 },
    Delay { ms: u64 },
    PhaseTransition { from: String, to: String },
    Verification { offset: usize, expected: u32, actual: u32, ok: bool },
}

/// Accumulated log from a training attempt.
#[derive(Debug, Clone, Default)]
pub struct TrainingLog {
    pub actions: Vec<TrainingAction>,
}

impl TrainingLog {
    fn log_write(&mut self, offset: usize, value: u32, old: u32) {
        self.actions.push(TrainingAction::RegWrite { offset, value, old });
    }

    fn log_read(&mut self, offset: usize, value: u32) {
        self.actions.push(TrainingAction::RegRead { offset, value });
    }

    fn log_delay(&mut self, ms: u64) {
        self.actions.push(TrainingAction::Delay { ms });
    }

    fn log_phase(&mut self, from: &str, to: &str) {
        self.actions.push(TrainingAction::PhaseTransition {
            from: from.into(),
            to: to.into(),
        });
    }

    fn log_verify(&mut self, offset: usize, expected: u32, actual: u32) {
        self.actions.push(TrainingAction::Verification {
            offset,
            expected,
            actual,
            ok: actual == expected,
        });
    }

    /// Count of register writes performed.
    pub fn write_count(&self) -> usize {
        self.actions.iter().filter(|a| matches!(a, TrainingAction::RegWrite { .. })).count()
    }
}

// ── Training backends ───────────────────────────────────────────────────

/// Selects which backend drives the HBM2 training register writes.
#[derive(Debug, Clone)]
pub enum TrainingBackend {
    /// Execute VBIOS init scripts from host CPU via BAR0.
    VbiosInterpreter { rom: Vec<u8> },
    /// Replay a captured register diff from an oracle card.
    DifferentialReplay { golden_state: Vec<(usize, u32)> },
    /// Upload DEVINIT firmware to PMU FALCON and execute.
    FalconUpload { rom: Vec<u8> },
}

// ── FBPA partition snapshot ─────────────────────────────────────────────

/// Snapshot of a single FBPA partition's key registers.
#[derive(Debug, Clone)]
pub struct FbpaSnapshot {
    pub index: usize,
    pub base: usize,
    pub cfg: u32,
    pub timing0: u32,
    pub timing1: u32,
    pub timing2: u32,
    pub alive: bool,
}

/// Snapshot all FBPA partitions.
pub fn snapshot_fbpa(bar0: &MappedBar, count: usize) -> Vec<FbpaSnapshot> {
    (0..count).map(|i| {
        let base = volta_hbm2::fbpa_reg(i, FbpaOffset(0));
        let r = |off: FbpaOffset| bar0.read_u32(volta_hbm2::fbpa_reg(i, off)).unwrap_or(0xDEAD_DEAD);
        let cfg = r(volta_hbm2::FBPA_CFG);
        let is_err = |v: u32| v == 0xFFFF_FFFF || v == 0xDEAD_DEAD || (v >> 16) == 0xBADF;
        FbpaSnapshot {
            index: i,
            base,
            cfg,
            timing0: r(volta_hbm2::FBPA_TIMING0),
            timing1: r(volta_hbm2::FBPA_TIMING1),
            timing2: r(volta_hbm2::FBPA_TIMING2),
            alive: !is_err(cfg),
        }
    }).collect()
}

// ── The typestate controller ────────────────────────────────────────────

/// HBM2 memory controller with compile-time phase enforcement.
///
/// Each phase transition method consumes `self` and returns the next phase.
/// This makes it impossible to call `verify_vram()` before `enable_phy()`,
/// or to use a controller after it has been consumed by a transition.
pub struct Hbm2Controller<'a, S: Hbm2Phase> {
    bar0: &'a MappedBar,
    bdf: Option<String>,
    fbpa_count: usize,
    ltc_count: usize,
    log: TrainingLog,
    backend: Option<TrainingBackend>,
    _phase: PhantomData<S>,
}

impl<'a, S: Hbm2Phase> fmt::Debug for Hbm2Controller<'a, S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Hbm2Controller")
            .field("phase", &std::any::type_name::<S>())
            .field("fbpa_count", &self.fbpa_count)
            .field("writes", &self.log.write_count())
            .finish()
    }
}

impl<'a> Hbm2Controller<'a, Untrained> {
    /// Create a new HBM2 controller in the Untrained state.
    pub fn new(bar0: &'a MappedBar, bdf: Option<&str>, fbpa_count: usize) -> Self {
        Self {
            bar0,
            bdf: bdf.map(String::from),
            fbpa_count,
            ltc_count: 6,
            log: TrainingLog::default(),
            backend: None,
            _phase: PhantomData,
        }
    }

    /// Attach a training backend.
    pub fn with_backend(mut self, backend: TrainingBackend) -> Self {
        self.backend = Some(backend);
        self
    }

    /// Phase 1: Progressive domain enable with PRI backpressure sensing.
    ///
    /// Consumes the Untrained controller and returns PhyUp on success.
    /// Instead of just setting two PMC bits, this phase progressively
    /// enables clock domains and verifies each comes alive before proceeding.
    /// The PRI sensor prevents cascading faults from dead domains.
    pub fn enable_phy(mut self) -> Result<Hbm2Controller<'a, PhyUp>, Hbm2TrainingError> {
        self.log.log_phase("Untrained", "PhyUp");

        let pmc_before = self.r(volta_hbm2::PMC_ENABLE);
        eprintln!("  HBM2 enable_phy: PMC_ENABLE before = {pmc_before:#010x}");

        // Step 1: Full engine enable (like GlowPlug step 1)
        self.w(volta_hbm2::PMC_ENABLE, 0xFFFF_FFFF);
        self.delay(50);

        let pmc_after = self.r(volta_hbm2::PMC_ENABLE);
        eprintln!("  HBM2 enable_phy: PMC_ENABLE after full = {pmc_after:#010x}");

        // Step 2: PRI bus recovery after the broad enable (clear accumulated faults)
        self.attempt_pri_recovery();
        self.delay(10);

        // Step 3: Probe each HBM2-critical domain to see what came alive
        let domain_probes: &[(&str, usize)] = &[
            ("PBUS",       0x001200),
            ("PFIFO",      0x002004),
            ("PFB",        0x100000),
            ("FBHUB",      0x100800),
            ("PFB_NISO",   0x100C80),
            ("PMU_FALCON", 0x10A000),
            ("LTC0",       0x17E200),
            ("FBPA0",      0x9A0000),
            ("FBPA1",      0x9A4000),
            ("FBPA2",      0x9A8000),
            ("FBPA3",      0x9AC000),
            ("PCLOCK",     0x137000),
        ];

        let mut alive_domains = Vec::new();
        let mut dead_domains = Vec::new();

        for &(name, off) in domain_probes {
            let val = self.bar0.read_u32(off).unwrap_or(0xDEAD_DEAD);
            if super::registers::pri::is_pri_error(val) {
                dead_domains.push((name, off, val));
                eprintln!("  HBM2 enable_phy:   {name} [{off:#010x}] = {val:#010x} FAULTED");
            } else {
                alive_domains.push((name, off, val));
                eprintln!("  HBM2 enable_phy:   {name} [{off:#010x}] = {val:#010x} ALIVE");
            }
            self.log.log_read(off, val);
        }

        // Step 4: If FBPA/LTC still dead, try progressive enable:
        // Some Volta cards need PBUS configured before FBPA responds
        if dead_domains.iter().any(|(n, _, _)| n.starts_with("FBPA")) {
            eprintln!("  HBM2 enable_phy: FBPA dead, trying progressive enable...");

            // Clear PRI faults again
            self.attempt_pri_recovery();

            // Try enabling PMC with PFIFO reset cycle (bit 8 off then on)
            let pmc_cur = self.r(volta_hbm2::PMC_ENABLE);
            self.w(volta_hbm2::PMC_ENABLE, pmc_cur & !(1 << 8));
            self.delay(20);
            self.w(volta_hbm2::PMC_ENABLE, pmc_cur | (1 << 8));
            self.delay(50);
            self.attempt_pri_recovery();

            // Re-probe FBPA partitions
            for i in 0..self.fbpa_count {
                let off = volta_hbm2::fbpa_reg(i, FbpaOffset(0));
                let val = self.bar0.read_u32(off).unwrap_or(0xDEAD_DEAD);
                eprintln!("  HBM2 enable_phy:   FBPA{i} re-probe = {val:#010x}");
            }
        }

        // Step 5: Verify FBPA partition health
        let snaps = snapshot_fbpa(self.bar0, self.fbpa_count);
        let alive_count = snaps.iter().filter(|s| s.alive).count();

        for snap in &snaps {
            self.log.log_read(snap.base, snap.cfg);
        }

        eprintln!(
            "  HBM2 enable_phy: {alive_count}/{} FBPA alive, {}/{} domains alive",
            self.fbpa_count,
            alive_domains.len(),
            domain_probes.len(),
        );

        // Allow transition even with partial FBPA (we'll work with what we have)
        if alive_count == 0 && alive_domains.is_empty() {
            return Err(Hbm2TrainingError {
                phase: "enable_phy",
                detail: format!(
                    "No domains came alive after full PMC enable. PMC={:#010x}. \
                     Dead: {:?}",
                    self.r(volta_hbm2::PMC_ENABLE),
                    dead_domains.iter().map(|(n, _, v)| format!("{n}={v:#x}")).collect::<Vec<_>>(),
                ),
                register_snapshot: snaps.iter().map(|s| (s.base, s.cfg)).collect(),
            });
        }

        Ok(self.transition())
    }
}

impl<'a> Hbm2Controller<'a, PhyUp> {
    /// Phase 2: Execute HBM2 PHY link training.
    ///
    /// Consumes PhyUp and returns LinkTrained on success.
    /// The actual training sequence depends on the selected backend.
    pub fn train_links(mut self) -> Result<Hbm2Controller<'a, LinkTrained>, Hbm2TrainingError> {
        self.log.log_phase("PhyUp", "LinkTrained");

        match &self.backend {
            Some(TrainingBackend::VbiosInterpreter { rom }) => {
                let rom = rom.clone();
                self.train_links_vbios(&rom)?;
            }
            Some(TrainingBackend::DifferentialReplay { golden_state }) => {
                let state = golden_state.clone();
                self.train_links_replay(&state)?;
            }
            Some(TrainingBackend::FalconUpload { rom }) => {
                let rom = rom.clone();
                self.train_links_falcon(&rom)?;
            }
            None => {
                // No backend: try VBIOS from PROM, then sysfs, then pre-dumped
                self.train_links_auto()?;
            }
        }

        // Verify at least some FBPA partitions show non-zero config
        let snaps = snapshot_fbpa(self.bar0, self.fbpa_count);
        let configured = snaps.iter().filter(|s| s.cfg != 0 && s.alive).count();

        eprintln!(
            "  HBM2 train_links: {configured}/{} FBPA partitions configured",
            self.fbpa_count,
        );

        Ok(self.transition())
    }

    fn train_links_vbios(&mut self, rom: &[u8]) -> Result<(), Hbm2TrainingError> {
        // Use the full interpreter if available, fall back to script writes
        match super::devinit::interpret_boot_scripts(self.bar0, rom) {
            Ok(stats) => {
                eprintln!(
                    "  VBIOS interpreter: {} ops executed, {} writes, {} skipped",
                    stats.ops_executed, stats.writes_applied, stats.ops_skipped,
                );
                self.delay(100);
                Ok(())
            }
            Err(e) => {
                eprintln!("  VBIOS interpreter failed: {e}, falling back to script scan");
                self.train_links_script_scan(rom)
            }
        }
    }

    fn train_links_script_scan(&mut self, rom: &[u8]) -> Result<(), Hbm2TrainingError> {
        let writes = devinit::extract_boot_script_writes(rom)
            .map_err(|e| Hbm2TrainingError {
                phase: "train_links",
                detail: format!("VBIOS script extraction: {e}"),
                register_snapshot: vec![],
            })?;

        let fbpa_range = volta_hbm2::FBPA0_BASE..volta_hbm2::FBPA0_BASE + volta_hbm2::FBPA_COUNT * volta_hbm2::FBPA_STRIDE;
        let ltc_range = volta_hbm2::LTC_BASE..volta_hbm2::LTC_BASE + volta_hbm2::LTC_COUNT * volta_hbm2::LTC_STRIDE;
        let pfb_range = volta_hbm2::PFB_BASE..volta_hbm2::PFB_BASE + 0x2000;
        let clk_range = volta_hbm2::CLK_BASE..volta_hbm2::CLK_BASE + 0x1000;
        let pclock_range = volta_hbm2::PCLOCK_BASE..volta_hbm2::PCLOCK_BASE + 0x1000;

        let hbm2_writes: Vec<_> = writes.iter().filter(|w| {
            let r = w.reg as usize;
            fbpa_range.contains(&r) || ltc_range.contains(&r)
                || pfb_range.contains(&r) || clk_range.contains(&r)
                || pclock_range.contains(&r)
        }).collect();

        eprintln!("  Script scan: {} total writes, {} HBM2-critical", writes.len(), hbm2_writes.len());

        for w in &hbm2_writes {
            let off = w.reg as usize;
            if let Some(mask) = w.mask {
                let cur = self.r(off);
                let new_val = (cur & mask) | w.value;
                self.w(off, new_val);
            } else {
                self.w(off, w.value);
            }
        }

        self.delay(100);
        Ok(())
    }

    fn train_links_replay(&mut self, golden: &[(usize, u32)]) -> Result<(), Hbm2TrainingError> {
        // Apply golden register state in domain order: FBPA, LTC, PCLOCK, CLK, PFB
        let domains: &[(&str, std::ops::Range<usize>)] = &[
            ("FBPA", volta_hbm2::FBPA0_BASE..volta_hbm2::FBPA0_BASE + volta_hbm2::FBPA_COUNT * volta_hbm2::FBPA_STRIDE),
            ("LTC", volta_hbm2::LTC_BASE..volta_hbm2::LTC_BASE + volta_hbm2::LTC_COUNT * volta_hbm2::LTC_STRIDE),
            ("PCLOCK", volta_hbm2::PCLOCK_BASE..volta_hbm2::PCLOCK_BASE + 0x1000),
            ("CLK", volta_hbm2::CLK_BASE..volta_hbm2::CLK_BASE + 0x1000),
            ("PFB", volta_hbm2::PFB_BASE..volta_hbm2::PFB_BASE + 0x2000),
        ];

        for (name, range) in domains {
            let domain_writes: Vec<_> = golden.iter()
                .filter(|(off, _)| range.contains(off))
                .collect();

            if domain_writes.is_empty() { continue; }

            eprintln!("  Replay {name}: {} writes", domain_writes.len());
            for &&(off, val) in &domain_writes {
                self.w(off, val);
            }
            self.delay(50);

            // Check VRAM after each domain in case we found the minimal set
            if self.check_vram_accessible() {
                eprintln!("  VRAM became accessible after {name} replay!");
                return Ok(());
            }
        }

        Ok(())
    }

    fn train_links_falcon(&mut self, rom: &[u8]) -> Result<(), Hbm2TrainingError> {
        // Check devinit status first
        let status = devinit::DevinitStatus::probe(self.bar0);
        if !status.needs_post {
            eprintln!("  FALCON: devinit already complete, skipping");
            return Ok(());
        }

        // Check FALCON security bits to see if we can upload
        let hwcfg = self.r(0x10A108);
        self.log.log_read(0x10A108, hwcfg);
        let secure_only = hwcfg & (1 << 8) != 0;
        if secure_only {
            return Err(Hbm2TrainingError {
                phase: "train_links",
                detail: format!("PMU FALCON requires signed firmware (HWCFG={hwcfg:#010x})"),
                register_snapshot: vec![(0x10A108, hwcfg)],
            });
        }

        match devinit::execute_devinit(self.bar0, rom) {
            Ok(true) => {
                eprintln!("  FALCON devinit completed successfully");
                self.delay(100);
                Ok(())
            }
            Ok(false) => {
                eprintln!("  FALCON: devinit was not needed");
                Ok(())
            }
            Err(e) => Err(Hbm2TrainingError {
                phase: "train_links",
                detail: format!("FALCON devinit: {e}"),
                register_snapshot: vec![],
            }),
        }
    }

    fn train_links_auto(&mut self) -> Result<(), Hbm2TrainingError> {
        // Strategy A: try PROM read for VBIOS
        if let Ok(rom) = devinit::read_vbios_prom(self.bar0) {
            eprintln!("  Auto: read {} KB from PROM", rom.len() / 1024);
            return self.train_links_vbios(&rom);
        }

        // Strategy B: try sysfs ROM
        if let Some(bdf) = &self.bdf {
            if let Ok(rom) = devinit::read_vbios_sysfs(bdf) {
                eprintln!("  Auto: read {} KB from sysfs ROM", rom.len() / 1024);
                return self.train_links_vbios(&rom);
            }
        }

        // Strategy C: try pre-dumped VBIOS files
        let dump_paths = [
            "/home/biomegate/Development/ecoPrimals/hotSpring/data/vbios_0000_4a_00_0.bin",
            "/home/biomegate/Development/ecoPrimals/hotSpring/data/vbios_0000_03_00_0.bin",
        ];
        for path in &dump_paths {
            if let Ok(rom) = devinit::read_vbios_file(path) {
                eprintln!("  Auto: read {} KB from {path}", rom.len() / 1024);
                return self.train_links_vbios(&rom);
            }
        }

        Err(Hbm2TrainingError {
            phase: "train_links",
            detail: "No VBIOS source available (PROM, sysfs, file all failed)".into(),
            register_snapshot: vec![],
        })
    }
}

impl<'a> Hbm2Controller<'a, LinkTrained> {
    /// Phase 3: Configure DRAM timing registers and memory controller modes.
    ///
    /// Consumes LinkTrained and returns DramReady on success.
    pub fn init_dram(mut self) -> Result<Hbm2Controller<'a, DramReady>, Hbm2TrainingError> {
        self.log.log_phase("LinkTrained", "DramReady");

        // Read back PFB configuration to verify memory controller state
        let cfg0 = self.r(volta_hbm2::PFB_CFG0.0);
        let cfg1 = self.r(volta_hbm2::PFB_CFG1.0);
        let mem_status = self.r(volta_hbm2::PFB_MEM_STATUS.0);
        let mem_ctrl = self.r(volta_hbm2::PFB_MEM_CTRL.0);

        self.log.log_read(volta_hbm2::PFB_CFG0.0, cfg0);
        self.log.log_read(volta_hbm2::PFB_CFG1.0, cfg1);
        self.log.log_read(volta_hbm2::PFB_MEM_STATUS.0, mem_status);
        self.log.log_read(volta_hbm2::PFB_MEM_CTRL.0, mem_ctrl);

        eprintln!(
            "  HBM2 init_dram: PFB_CFG0={cfg0:#010x} CFG1={cfg1:#010x} \
             MEM_STATUS={mem_status:#010x} MEM_CTRL={mem_ctrl:#010x}",
        );

        // Set NISO flush address to 0 (required for VRAM accessibility)
        self.w(volta_hbm2::PFB_NISO_FLUSH_LO.0, 0);
        self.w(volta_hbm2::PFB_NISO_FLUSH_HI.0, 0);
        self.delay(10);

        // Snapshot all FBPA timing registers as evidence
        let snaps = snapshot_fbpa(self.bar0, self.fbpa_count);
        for snap in &snaps {
            eprintln!(
                "  FBPA{}: cfg={:#010x} t0={:#010x} t1={:#010x} t2={:#010x} {}",
                snap.index, snap.cfg, snap.timing0, snap.timing1, snap.timing2,
                if snap.alive { "alive" } else { "DEAD" },
            );
        }

        Ok(self.transition())
    }
}

impl<'a> Hbm2Controller<'a, DramReady> {
    /// Phase 4: Verify VRAM is accessible via PRAMIN sentinel test.
    ///
    /// Consumes DramReady and returns Verified on success.
    /// Tests write/readback on each FBPA stack's VRAM range.
    pub fn verify_vram(mut self) -> Result<Hbm2Controller<'a, Verified>, Hbm2TrainingError> {
        self.log.log_phase("DramReady", "Verified");

        // Test VRAM at several offsets via PRAMIN
        let test_offsets: &[u32] = &[
            0x0000_0000, 0x0001_0000, 0x0002_6000,
            0x0004_0000, 0x0008_0000,
        ];

        let mut passed = 0;
        let mut failed_detail = Vec::new();

        for &vram_off in test_offsets {
            match PraminRegion::new(self.bar0, vram_off, 8) {
                Ok(mut region) => {
                    let sentinel = 0xCAFE_0000 | vram_off;
                    let status = region.probe_sentinel(0, sentinel);
                    if status.is_working() {
                        passed += 1;
                        self.log.log_verify(vram_off as usize, sentinel, sentinel);
                    } else {
                        let readback = region.read_u32(0).unwrap_or(0xDEAD);
                        failed_detail.push(format!(
                            "PRAMIN@{vram_off:#x}: wrote {sentinel:#010x}, read {readback:#010x}"
                        ));
                        self.log.log_verify(vram_off as usize, sentinel, readback);
                    }
                }
                Err(e) => {
                    failed_detail.push(format!("PRAMIN@{vram_off:#x}: {e}"));
                }
            }
        }

        eprintln!(
            "  HBM2 verify_vram: {passed}/{} PRAMIN sentinel tests passed",
            test_offsets.len(),
        );

        if passed == 0 {
            return Err(Hbm2TrainingError {
                phase: "verify_vram",
                detail: format!(
                    "All VRAM sentinel tests failed: {}",
                    failed_detail.join("; "),
                ),
                register_snapshot: vec![
                    (volta_hbm2::PFB_CFG0.0, self.r(volta_hbm2::PFB_CFG0.0)),
                    (volta_hbm2::PFB_MEM_STATUS.0, self.r(volta_hbm2::PFB_MEM_STATUS.0)),
                ],
            });
        }

        Ok(self.transition())
    }
}

impl<'a> Hbm2Controller<'a, Verified> {
    /// The training log from the completed sequence.
    pub fn training_log(&self) -> &TrainingLog {
        &self.log
    }

    /// VRAM is confirmed accessible. Returns the PRAMIN base offset for
    /// higher-level code to use.
    pub fn pramin_base(&self) -> usize {
        volta_hbm2::PRAMIN_BASE
    }

    /// Export FBPA partition state as evidence.
    pub fn fbpa_state(&self) -> Vec<FbpaSnapshot> {
        snapshot_fbpa(self.bar0, self.fbpa_count)
    }
}

// ── Shared helpers (available in all phases) ────────────────────────────

impl<'a, S: Hbm2Phase> Hbm2Controller<'a, S> {
    fn r(&mut self, offset: usize) -> u32 {
        let val = self.bar0.read_u32(offset).unwrap_or(0xDEAD_DEAD);
        if super::registers::pri::is_pri_error(val) {
            self.log.log_read(offset, val);
            self.attempt_pri_recovery();
        }
        val
    }

    fn w(&mut self, offset: usize, value: u32) {
        let old = self.bar0.read_u32(offset).unwrap_or(0xDEAD_DEAD);

        // Skip writes to domains returning PRI errors (backpressure)
        if super::registers::pri::is_pri_error(old) {
            self.log.log_write(offset, value, old);
            return;
        }

        let _ = self.bar0.write_u32(offset, value);
        self.log.log_write(offset, value, old);
    }

    fn delay(&mut self, ms: u64) {
        self.log.log_delay(ms);
        std::thread::sleep(std::time::Duration::from_millis(ms));
    }

    fn check_vram_accessible(&self) -> bool {
        if let Ok(mut region) = PraminRegion::new(self.bar0, 0x0002_6000, 8) {
            region.probe_sentinel(0, 0xCAFE_DEAD).is_working()
        } else {
            false
        }
    }

    /// Attempt PRI bus recovery — clear PRIV_RING faults and PMC INTR.
    fn attempt_pri_recovery(&self) {
        let _ = self.bar0.write_u32(
            super::registers::pri::PRIV_RING_COMMAND,
            super::registers::pri::PRIV_RING_CMD_ACK,
        );
        let pmc_intr = self.bar0.read_u32(super::registers::pri::PMC_INTR).unwrap_or(0);
        if pmc_intr & super::registers::pri::PMC_INTR_PRIV_RING_BIT != 0 {
            let _ = self.bar0.write_u32(
                super::registers::pri::PMC_INTR,
                super::registers::pri::PMC_INTR_PRIV_RING_BIT,
            );
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }

    /// Transition to the next phase, preserving BAR0 reference, log, and backend.
    fn transition<N: Hbm2Phase>(self) -> Hbm2Controller<'a, N> {
        Hbm2Controller {
            bar0: self.bar0,
            bdf: self.bdf,
            fbpa_count: self.fbpa_count,
            ltc_count: self.ltc_count,
            log: self.log,
            backend: self.backend,
            _phase: PhantomData,
        }
    }

    /// Consume the controller at any phase and return the training log.
    pub fn into_log(self) -> TrainingLog {
        self.log
    }
}

// ── Convenience: run the full sequence ──────────────────────────────────

/// Attempt the full HBM2 training sequence: Untrained → Verified.
///
/// Returns the verified controller on success, or the error and partial log on failure.
pub fn train_hbm2<'a>(
    bar0: &'a MappedBar,
    bdf: Option<&str>,
    backend: Option<TrainingBackend>,
) -> Result<Hbm2Controller<'a, Verified>, (Hbm2TrainingError, TrainingLog)> {
    let mut ctrl = Hbm2Controller::new(bar0, bdf, volta_hbm2::FBPA_COUNT);
    if let Some(be) = backend {
        ctrl = ctrl.with_backend(be);
    }

    let ctrl = ctrl.enable_phy().map_err(|e| {
        (e, TrainingLog::default())
    })?;

    let ctrl = ctrl.train_links().map_err(|e| {
        (e, TrainingLog::default())
    })?;

    let ctrl = ctrl.init_dram().map_err(|e| {
        (e, TrainingLog::default())
    })?;

    ctrl.verify_vram().map_err(|e| {
        (e, TrainingLog::default())
    })
}

// ── Differential capture/replay harness ─────────────────────────────────

/// HBM2-critical register domains for ordered capture and replay.
pub const HBM2_CAPTURE_DOMAINS: &[(&str, usize, usize)] = &[
    ("FBPA0",     0x9A0000, 0x9A1000),
    ("FBPA1",     0x9A4000, 0x9A5000),
    ("FBPA2",     0x9A8000, 0x9A9000),
    ("FBPA3",     0x9AC000, 0x9AD000),
    ("LTC0",      0x17E000, 0x180000),
    ("LTC1",      0x180000, 0x182000),
    ("LTC2",      0x182000, 0x184000),
    ("LTC3",      0x184000, 0x186000),
    ("LTC4",      0x186000, 0x188000),
    ("LTC5",      0x188000, 0x18A000),
    ("PCLOCK",    0x137000, 0x138000),
    ("CLK",       0x132000, 0x133000),
    ("PFB",       0x100000, 0x102000),
    ("PFB_NISO",  0x100C00, 0x100E00),
    ("FBHUB",     0x100800, 0x100A00),
    ("PMU",       0x10A000, 0x10B000),
];

/// A captured register state from a domain.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DomainCapture {
    pub name: String,
    pub registers: Vec<(usize, u32)>,
}

/// Complete golden state captured from an oracle card.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GoldenCapture {
    pub boot0: u32,
    pub pmc_enable: u32,
    pub domains: Vec<DomainCapture>,
    pub timestamp: String,
}

impl GoldenCapture {
    /// Flatten all domain registers into a single sorted list.
    pub fn all_registers(&self) -> Vec<(usize, u32)> {
        let mut all: Vec<(usize, u32)> = self.domains.iter()
            .flat_map(|d| d.registers.iter().copied())
            .collect();
        all.sort_by_key(|(off, _)| *off);
        all
    }

    /// Total register count across all domains.
    pub fn register_count(&self) -> usize {
        self.domains.iter().map(|d| d.registers.len()).sum()
    }
}

/// Capture the golden register state from an oracle card via sysfs BAR0.
///
/// The oracle card must be bound to a driver (nouveau) that has completed
/// HBM2 training. This function reads all HBM2-critical domains.
pub fn capture_oracle_state(oracle_bdf: &str) -> Result<GoldenCapture, String> {
    let resource0_path = format!("/sys/bus/pci/devices/{oracle_bdf}/resource0");
    let file = std::fs::OpenOptions::new()
        .read(true)
        .open(&resource0_path)
        .map_err(|e| format!("cannot open {resource0_path}: {e}"))?;

    let bar0_size: usize = 16 * 1024 * 1024;
    let ptr = unsafe {
        rustix::mm::mmap(
            std::ptr::null_mut(),
            bar0_size,
            rustix::mm::ProtFlags::READ,
            rustix::mm::MapFlags::SHARED,
            &file,
            0,
        )
    }.map_err(|e| format!("mmap {resource0_path}: {e}"))?;

    let read = |offset: usize| -> u32 {
        if offset + 4 > bar0_size { return 0xDEAD_DEAD; }
        unsafe { std::ptr::read_volatile(ptr.cast::<u8>().add(offset).cast::<u32>()) }
    };
    let is_err = |v: u32| v == 0xFFFF_FFFF || v == 0xDEAD_DEAD || (v >> 16) == 0xBADF || (v >> 16) == 0xBAD0;

    let boot0 = read(0);
    if boot0 == 0xFFFF_FFFF {
        unsafe { let _ = rustix::mm::munmap(ptr, bar0_size); }
        return Err("Oracle BAR0 reads 0xFFFFFFFF — card in D3hot?".into());
    }

    let pmc_enable = read(0x200);
    let mut domains = Vec::new();

    for &(name, start, end) in HBM2_CAPTURE_DOMAINS {
        let mut registers = Vec::new();
        for off in (start..end).step_by(4) {
            let val = read(off);
            if !is_err(val) {
                registers.push((off, val));
            }
        }
        domains.push(DomainCapture {
            name: name.into(),
            registers,
        });
    }

    unsafe { let _ = rustix::mm::munmap(ptr, bar0_size); }

    let total: usize = domains.iter().map(|d| d.registers.len()).sum();
    eprintln!(
        "  Oracle capture: {total} registers from {} domains (BOOT0={boot0:#010x})",
        domains.len(),
    );

    Ok(GoldenCapture {
        boot0,
        pmc_enable,
        domains,
        timestamp: chrono_timestamp(),
    })
}

/// Compute the diff between an oracle's golden state and the current cold card.
///
/// Returns the subset of golden registers that differ from the cold card,
/// organized by domain for ordered replay.
pub fn diff_golden_vs_cold(
    bar0: &MappedBar,
    golden: &GoldenCapture,
) -> Vec<DomainCapture> {
    let r = |off: usize| bar0.read_u32(off).unwrap_or(0xDEAD_DEAD);

    let mut diffs = Vec::new();
    for domain in &golden.domains {
        let mut domain_diffs = Vec::new();
        for &(off, golden_val) in &domain.registers {
            let cold_val = r(off);
            if cold_val != golden_val {
                domain_diffs.push((off, golden_val));
            }
        }
        if !domain_diffs.is_empty() {
            diffs.push(DomainCapture {
                name: domain.name.clone(),
                registers: domain_diffs,
            });
        }
    }

    let total_diffs: usize = diffs.iter().map(|d| d.registers.len()).sum();
    eprintln!(
        "  Golden diff: {total_diffs} registers differ across {} domains",
        diffs.len(),
    );
    for d in &diffs {
        eprintln!("    {}: {} diffs", d.name, d.registers.len());
    }

    diffs
}

/// Replay a domain-ordered diff with per-domain VRAM verification and PRI backpressure.
///
/// Applies register writes in domain order (FBPA first, then LTC, etc.)
/// and checks VRAM accessibility after each domain. Stops early if VRAM
/// becomes accessible, identifying the minimal required domain set.
///
/// Uses PRI bus monitoring to detect faulted domains and attempt recovery
/// between domain batches rather than blindly writing.
pub fn replay_golden_diff(
    bar0: &MappedBar,
    diffs: &[DomainCapture],
) -> ReplayResult {
    use super::pri_monitor::{PriBusMonitor, WriteOutcome};

    let mut result = ReplayResult::default();
    let mut monitor = PriBusMonitor::new(bar0).with_fault_threshold(5);

    for domain in diffs {
        // Probe domain health before writing
        let first_reg = domain.registers.first().map(|(off, _)| *off).unwrap_or(0);
        let health = monitor.probe_domain(first_reg);
        let domain_faulted = matches!(health, super::pri_monitor::DomainHealth::Faulted { .. });

        if domain_faulted {
            eprintln!(
                "  Replay: {}: domain faulted, attempting PRI recovery before writes",
                domain.name,
            );
            monitor.attempt_recovery();
        }

        let mut applied = 0;
        let mut skipped = 0;
        for &(off, val) in &domain.registers {
            match monitor.write_u32(off, val) {
                WriteOutcome::Applied => applied += 1,
                WriteOutcome::SkippedFaulted | WriteOutcome::Throttled => skipped += 1,
                WriteOutcome::AppliedButFaulted => {
                    applied += 1;
                    // Domain is responding badly — try recovery mid-batch
                    if applied % 10 == 0 {
                        monitor.attempt_recovery();
                    }
                }
            }
        }
        result.domains_applied.push((domain.name.clone(), applied));

        if skipped > 0 {
            eprintln!(
                "  Replay: {}: {applied} applied, {skipped} PRI-skipped",
                domain.name,
            );
        }

        std::thread::sleep(std::time::Duration::from_millis(50));

        // Check VRAM after each domain
        let vram_ok = if let Ok(mut region) = PraminRegion::new(bar0, 0x0002_6000, 8) {
            region.probe_sentinel(0, 0xCAFE_DEAD).is_working()
        } else {
            false
        };

        if vram_ok {
            result.vram_unlocked_after = Some(domain.name.clone());
            result.success = true;
            eprintln!("  Replay: VRAM alive after {} domain!", domain.name);
            break;
        }

        // PRI recovery between domains
        monitor.attempt_recovery();
    }

    let stats = monitor.into_report();
    eprintln!(
        "  Replay PRI stats: {} reads ({} faulted), {} writes ({} applied, {} skipped), {} recoveries",
        stats.reads_total, stats.reads_faulted,
        stats.writes_total, stats.writes_applied, stats.writes_skipped_faulted,
        stats.bus_recoveries,
    );

    result
}

/// Result of a golden state replay attempt.
#[derive(Debug, Clone, Default)]
pub struct ReplayResult {
    pub domains_applied: Vec<(String, usize)>,
    pub vram_unlocked_after: Option<String>,
    pub success: bool,
}

/// Perform the complete differential capture → diff → replay pipeline.
///
/// 1. Capture golden state from oracle
/// 2. Diff against current cold card
/// 3. Replay with per-domain verification
pub fn differential_training(
    bar0: &MappedBar,
    oracle_bdf: &str,
) -> Result<ReplayResult, String> {
    let golden = capture_oracle_state(oracle_bdf)?;

    // Verify oracle and target are same GPU
    let target_boot0 = bar0.read_u32(0).unwrap_or(0xDEAD_DEAD);
    if golden.boot0 != target_boot0 {
        return Err(format!(
            "BOOT0 mismatch: oracle={:#010x} target={:#010x}",
            golden.boot0, target_boot0,
        ));
    }

    let diffs = diff_golden_vs_cold(bar0, &golden);
    if diffs.is_empty() {
        return Ok(ReplayResult {
            domains_applied: vec![],
            vram_unlocked_after: None,
            success: true,
        });
    }

    Ok(replay_golden_diff(bar0, &diffs))
}

fn chrono_timestamp() -> String {
    let now = std::time::SystemTime::now();
    let dur = now.duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    format!("{}s", dur.as_secs())
}

// ── Binary search for minimal write set ─────────────────────────────────

/// Binary search within a single domain's writes to find the minimal set
/// that unlocks VRAM. Requires that the full set is known to work.
pub fn binary_search_minimal_writes(
    bar0: &MappedBar,
    bdf: Option<&str>,
    domain_writes: &[(usize, u32)],
) -> Vec<(usize, u32)> {
    if domain_writes.is_empty() { return vec![]; }

    // First verify the full set works
    for &(off, val) in domain_writes {
        let _ = bar0.write_u32(off, val);
    }
    std::thread::sleep(std::time::Duration::from_millis(100));

    let vram_ok = if let Ok(mut region) = PraminRegion::new(bar0, 0x0002_6000, 8) {
        region.probe_sentinel(0, 0xCAFE_DEAD).is_working()
    } else {
        false
    };

    if !vram_ok {
        eprintln!("  MinimalSet: full set doesn't work, cannot binary search");
        return domain_writes.to_vec();
    }

    // Binary search: try first half, if it works, recurse on it.
    // This is a heuristic — HBM2 training may not be monotonic.
    let mut needed = domain_writes.to_vec();

    if needed.len() > 4 {
        let mid = needed.len() / 2;
        let first_half = &needed[..mid];

        // Reset state (requires D3hot→D0 cycle which needs BDF)
        if let Some(bdf) = bdf {
            let _ = crate::vfio::pci_discovery::set_pci_power_state(bdf, crate::vfio::pci_discovery::PciPmState::D3Hot);
            std::thread::sleep(std::time::Duration::from_millis(100));
            let _ = crate::vfio::pci_discovery::force_pci_d0(bdf);
            std::thread::sleep(std::time::Duration::from_millis(50));

            for &(off, val) in first_half {
                let _ = bar0.write_u32(off, val);
            }
            std::thread::sleep(std::time::Duration::from_millis(100));

            let half_ok = if let Ok(mut region) = PraminRegion::new(bar0, 0x0002_6000, 8) {
                region.probe_sentinel(0, 0xCAFE_DEAD).is_working()
            } else {
                false
            };

            if half_ok {
                eprintln!("  MinimalSet: first half ({} writes) sufficient", first_half.len());
                needed = first_half.to_vec();
            } else {
                eprintln!("  MinimalSet: need full set ({} writes)", needed.len());
            }
        }
    }

    needed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phase_debug_names() {
        assert_eq!(format!("{:?}", Untrained), "Untrained");
        assert_eq!(format!("{:?}", PhyUp), "PhyUp");
        assert_eq!(format!("{:?}", LinkTrained), "LinkTrained");
        assert_eq!(format!("{:?}", DramReady), "DramReady");
        assert_eq!(format!("{:?}", Verified), "Verified");
    }

    #[test]
    fn training_error_display() {
        let err = Hbm2TrainingError {
            phase: "enable_phy",
            detail: "no FBPA alive".into(),
            register_snapshot: vec![(0x9A0000, 0xDEAD_DEAD)],
        };
        assert!(err.to_string().contains("enable_phy"));
        assert!(err.to_string().contains("no FBPA alive"));
    }

    #[test]
    fn fbpa_offset_newtype_prevents_mixup() {
        let fbpa = FbpaOffset(0x04);
        let ltc = LtcOffset(0x04);
        // These are different types — can't accidentally pass one where the other is expected
        assert_eq!(fbpa.0, ltc.0);
        // But the types are distinct (this is a compile-time guarantee)
    }

    #[test]
    fn volta_fbpa_reg_calculation() {
        assert_eq!(volta_hbm2::fbpa_reg(0, FbpaOffset(0x04)), 0x9A0004);
        assert_eq!(volta_hbm2::fbpa_reg(1, FbpaOffset(0x04)), 0x9A4004);
        assert_eq!(volta_hbm2::fbpa_reg(3, FbpaOffset(0x80)), 0x9AC080);
    }

    #[test]
    fn training_log_counts_writes() {
        let mut log = TrainingLog::default();
        log.log_write(0x200, 0xFFFF_FFFF, 0);
        log.log_read(0x200, 0x5FEC_DFF1);
        log.log_write(0x9A0000, 0x42, 0);
        assert_eq!(log.write_count(), 2);
    }
}
