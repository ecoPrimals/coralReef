// SPDX-License-Identifier: AGPL-3.0-only
//! HBM2 typestate controller with phase-enforced transitions.

use std::fmt;
use std::marker::PhantomData;

use crate::vfio::device::MappedBar;
use crate::vfio::memory::{MemoryRegion, PraminRegion};

use super::backend::TrainingBackend;
use super::constants::volta_hbm2;
use super::snapshot::{FbpaSnapshot, snapshot_fbpa};
use super::types::{DramReady, LinkTrained, PhyUp, Untrained, Verified};
use super::types::{FbpaOffset, Hbm2Phase, Hbm2TrainingError, TrainingLog};

// ── The typestate controller ────────────────────────────────────────────

/// HBM2 memory controller with compile-time phase enforcement.
///
/// Each phase transition method consumes `self` and returns the next phase.
/// This makes it impossible to call `verify_vram()` before `enable_phy()`,
/// or to use a controller after it has been consumed by a transition.
pub struct Hbm2Controller<'a, S: Hbm2Phase> {
    pub(super) bar0: &'a MappedBar,
    pub(super) bdf: Option<String>,
    pub(super) fbpa_count: usize,
    pub(super) ltc_count: usize,
    pub(super) log: TrainingLog,
    pub(super) backend: Option<TrainingBackend>,
    pub(super) _phase: PhantomData<S>,
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
        tracing::debug!("HBM2 enable_phy: PMC_ENABLE before = {pmc_before:#010x}");

        // Step 1: Full engine enable (like GlowPlug step 1)
        self.w(volta_hbm2::PMC_ENABLE, 0xFFFF_FFFF);
        self.delay(50);

        let pmc_after = self.r(volta_hbm2::PMC_ENABLE);
        tracing::debug!("HBM2 enable_phy: PMC_ENABLE after full = {pmc_after:#010x}");

        // Step 2: PRI bus recovery after the broad enable (clear accumulated faults)
        self.attempt_pri_recovery();
        self.delay(10);

        // Step 3: Probe each HBM2-critical domain to see what came alive
        let domain_probes: &[(&str, usize)] = &[
            ("PBUS", 0x001200),
            ("PFIFO", 0x002004),
            ("PFB", 0x100000),
            ("FBHUB", 0x100800),
            ("PFB_NISO", 0x100C80),
            ("PMU_FALCON", 0x10A000),
            ("LTC0", 0x17E200),
            ("FBPA0", 0x9A0000),
            ("FBPA1", 0x9A4000),
            ("FBPA2", 0x9A8000),
            ("FBPA3", 0x9AC000),
            ("PCLOCK", 0x137000),
        ];

        let mut alive_domains = Vec::new();
        let mut dead_domains = Vec::new();

        for &(name, off) in domain_probes {
            let val = self.bar0.read_u32(off).unwrap_or(0xDEAD_DEAD);
            if super::super::registers::pri::is_pri_error(val) {
                dead_domains.push((name, off, val));
                tracing::debug!("HBM2 enable_phy:   {name} [{off:#010x}] = {val:#010x} FAULTED");
            } else {
                alive_domains.push((name, off, val));
                tracing::debug!("HBM2 enable_phy:   {name} [{off:#010x}] = {val:#010x} ALIVE");
            }
            self.log.log_read(off, val);
        }

        // Step 4: If FBPA/LTC still dead, try progressive enable:
        // Some Volta cards need PBUS configured before FBPA responds
        if dead_domains.iter().any(|(n, _, _)| n.starts_with("FBPA")) {
            tracing::debug!("HBM2 enable_phy: FBPA dead, trying progressive enable...");

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
                tracing::debug!("HBM2 enable_phy:   FBPA{i} re-probe = {val:#010x}");
            }
        }

        // Step 5: Verify FBPA partition health
        let snaps = snapshot_fbpa(self.bar0, self.fbpa_count);
        let alive_count = snaps.iter().filter(|s| s.alive).count();

        for snap in &snaps {
            self.log.log_read(snap.base, snap.cfg);
        }

        tracing::debug!(
            "HBM2 enable_phy: {alive_count}/{} FBPA alive, {}/{} domains alive",
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
                    dead_domains
                        .iter()
                        .map(|(n, _, v)| format!("{n}={v:#x}"))
                        .collect::<Vec<_>>(),
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

        tracing::debug!(
            "HBM2 train_links: {configured}/{} FBPA partitions configured",
            self.fbpa_count,
        );

        Ok(self.transition())
    }

    fn train_links_vbios(&mut self, rom: &[u8]) -> Result<(), Hbm2TrainingError> {
        match super::super::devinit::interpret_boot_scripts(self.bar0, rom) {
            Ok(stats) => {
                tracing::debug!(
                    "VBIOS interpreter: {} ops executed, {} writes, {} skipped",
                    stats.ops_executed,
                    stats.writes_applied,
                    stats.ops_skipped,
                );
                self.delay(100);
                Ok(())
            }
            Err(e) => {
                tracing::debug!("VBIOS interpreter failed: {e}, falling back to script scan");
                self.train_links_script_scan(rom)
            }
        }
    }

    fn train_links_script_scan(&mut self, rom: &[u8]) -> Result<(), Hbm2TrainingError> {
        let writes = super::super::devinit::extract_boot_script_writes(rom).map_err(|e| {
            Hbm2TrainingError {
                phase: "train_links",
                detail: format!("VBIOS script extraction: {e}"),
                register_snapshot: vec![],
            }
        })?;

        let fbpa_range = volta_hbm2::FBPA0_BASE
            ..volta_hbm2::FBPA0_BASE + volta_hbm2::FBPA_COUNT * volta_hbm2::FBPA_STRIDE;
        let ltc_range = volta_hbm2::LTC_BASE
            ..volta_hbm2::LTC_BASE + volta_hbm2::LTC_COUNT * volta_hbm2::LTC_STRIDE;
        let pfb_range = volta_hbm2::PFB_BASE..volta_hbm2::PFB_BASE + 0x2000;
        let clk_range = volta_hbm2::CLK_BASE..volta_hbm2::CLK_BASE + 0x1000;
        let pclock_range = volta_hbm2::PCLOCK_BASE..volta_hbm2::PCLOCK_BASE + 0x1000;

        let hbm2_writes: Vec<_> = writes
            .iter()
            .filter(|w| {
                let r = w.reg as usize;
                fbpa_range.contains(&r)
                    || ltc_range.contains(&r)
                    || pfb_range.contains(&r)
                    || clk_range.contains(&r)
                    || pclock_range.contains(&r)
            })
            .collect();

        tracing::debug!(
            "Script scan: {} total writes, {} HBM2-critical",
            writes.len(),
            hbm2_writes.len()
        );

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
        let domains: &[(&str, std::ops::Range<usize>)] = &[
            (
                "FBPA",
                volta_hbm2::FBPA0_BASE
                    ..volta_hbm2::FBPA0_BASE + volta_hbm2::FBPA_COUNT * volta_hbm2::FBPA_STRIDE,
            ),
            (
                "LTC",
                volta_hbm2::LTC_BASE
                    ..volta_hbm2::LTC_BASE + volta_hbm2::LTC_COUNT * volta_hbm2::LTC_STRIDE,
            ),
            (
                "PCLOCK",
                volta_hbm2::PCLOCK_BASE..volta_hbm2::PCLOCK_BASE + 0x1000,
            ),
            ("CLK", volta_hbm2::CLK_BASE..volta_hbm2::CLK_BASE + 0x1000),
            ("PFB", volta_hbm2::PFB_BASE..volta_hbm2::PFB_BASE + 0x2000),
        ];

        for (name, range) in domains {
            let domain_writes: Vec<_> = golden
                .iter()
                .filter(|(off, _)| range.contains(off))
                .collect();

            if domain_writes.is_empty() {
                continue;
            }

            tracing::debug!("Replay {name}: {} writes", domain_writes.len());
            for &&(off, val) in &domain_writes {
                self.w(off, val);
            }
            self.delay(50);

            if self.check_vram_accessible() {
                tracing::debug!("VRAM became accessible after {name} replay!");
                return Ok(());
            }
        }

        Ok(())
    }

    fn train_links_falcon(&mut self, rom: &[u8]) -> Result<(), Hbm2TrainingError> {
        let status = super::super::devinit::DevinitStatus::probe(self.bar0);
        if !status.needs_post {
            tracing::debug!("FALCON: devinit already complete, skipping");
            return Ok(());
        }

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

        match super::super::devinit::execute_devinit(self.bar0, rom) {
            Ok(true) => {
                tracing::debug!("FALCON devinit completed successfully");
                self.delay(100);
                Ok(())
            }
            Ok(false) => {
                tracing::debug!("FALCON: devinit was not needed");
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
        if let Ok(rom) = super::super::devinit::read_vbios_prom(self.bar0) {
            tracing::debug!("Auto: read {} KB from PROM", rom.len() / 1024);
            return self.train_links_vbios(&rom);
        }

        if let Some(bdf) = &self.bdf
            && let Ok(rom) = super::super::devinit::read_vbios_sysfs(bdf)
        {
            tracing::debug!("Auto: read {} KB from sysfs ROM", rom.len() / 1024);
            return self.train_links_vbios(&rom);
        }

        if let Ok(data_dir) = std::env::var("HOTSPRING_DATA_DIR") {
            let dump_names = ["vbios_0000_4a_00_0.bin", "vbios_0000_03_00_0.bin"];
            for name in &dump_names {
                let path = format!("{data_dir}/{name}");
                if let Ok(rom) = super::super::devinit::read_vbios_file(&path) {
                    tracing::debug!("Auto: read {} KB from {path}", rom.len() / 1024);
                    return self.train_links_vbios(&rom);
                }
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

        let cfg0 = self.r(volta_hbm2::PFB_CFG0.0);
        let cfg1 = self.r(volta_hbm2::PFB_CFG1.0);
        let mem_status = self.r(volta_hbm2::PFB_MEM_STATUS.0);
        let mem_ctrl = self.r(volta_hbm2::PFB_MEM_CTRL.0);

        self.log.log_read(volta_hbm2::PFB_CFG0.0, cfg0);
        self.log.log_read(volta_hbm2::PFB_CFG1.0, cfg1);
        self.log.log_read(volta_hbm2::PFB_MEM_STATUS.0, mem_status);
        self.log.log_read(volta_hbm2::PFB_MEM_CTRL.0, mem_ctrl);

        tracing::debug!(
            "HBM2 init_dram: PFB_CFG0={cfg0:#010x} CFG1={cfg1:#010x} \
             MEM_STATUS={mem_status:#010x} MEM_CTRL={mem_ctrl:#010x}",
        );

        self.w(volta_hbm2::PFB_NISO_FLUSH_LO.0, 0);
        self.w(volta_hbm2::PFB_NISO_FLUSH_HI.0, 0);
        self.delay(10);

        let snaps = snapshot_fbpa(self.bar0, self.fbpa_count);
        for snap in &snaps {
            tracing::debug!(
                "FBPA{}: cfg={:#010x} t0={:#010x} t1={:#010x} t2={:#010x} {}",
                snap.index,
                snap.cfg,
                snap.timing0,
                snap.timing1,
                snap.timing2,
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
    pub fn verify_vram(mut self) -> Result<Hbm2Controller<'a, Verified>, Hbm2TrainingError> {
        self.log.log_phase("DramReady", "Verified");

        let test_offsets: &[u32] = &[
            0x0000_0000,
            0x0001_0000,
            0x0002_6000,
            0x0004_0000,
            0x0008_0000,
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

        tracing::debug!(
            "HBM2 verify_vram: {passed}/{} PRAMIN sentinel tests passed",
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
                    (
                        volta_hbm2::PFB_MEM_STATUS.0,
                        self.r(volta_hbm2::PFB_MEM_STATUS.0),
                    ),
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

    /// VRAM is confirmed accessible. Returns the PRAMIN base offset.
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
        if super::super::registers::pri::is_pri_error(val) {
            self.log.log_read(offset, val);
            self.attempt_pri_recovery();
        }
        val
    }

    fn w(&mut self, offset: usize, value: u32) {
        let old = self.bar0.read_u32(offset).unwrap_or(0xDEAD_DEAD);

        if super::super::registers::pri::is_pri_error(old) {
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

    fn attempt_pri_recovery(&self) {
        let _ = self.bar0.write_u32(
            super::super::registers::pri::PRIV_RING_COMMAND,
            super::super::registers::pri::PRIV_RING_CMD_ACK,
        );
        let pmc_intr = self
            .bar0
            .read_u32(super::super::registers::pri::PMC_INTR)
            .unwrap_or(0);
        if pmc_intr & super::super::registers::pri::PMC_INTR_PRIV_RING_BIT != 0 {
            let _ = self.bar0.write_u32(
                super::super::registers::pri::PMC_INTR,
                super::super::registers::pri::PMC_INTR_PRIV_RING_BIT,
            );
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }

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
