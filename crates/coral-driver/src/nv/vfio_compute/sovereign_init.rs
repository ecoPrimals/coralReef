// SPDX-License-Identifier: AGPL-3.0-only
//! Sovereign GPU initialization — pure Rust replacement for nouveau.
//!
//! Chains all subsystems in dependency order to bring a GPU from cold (or warm)
//! to compute-ready without any kernel GPU driver. Proprietary firmware blobs
//! (VBIOS, ACR, GR) are consumed as *ingredients* — we handle the loading,
//! patching, and sequencing in pure Rust.
//!
//! ## Init sequence
//!
//! ```text
//! Stage 0: HBM2 Training           — VBIOS DEVINIT (cold only, auto-detect)
//! Stage 1: PMC + Engine Gating     — un-gate all engine clock domains
//! Stage 2: PRI Topology Discovery   — enumerate GPC/TPC/SM/FBP
//! Stage 3: PFB + Memory Controller  — configure FBPA, FBHUB, BAR2
//! Stage 4: Falcon Boot Chain        — SEC2 → ACR → FECS/GPCCS (solver)
//! Stage 5: GR Engine Init           — firmware BAR0 writes + dynamic GR config
//! Stage 6: PFIFO + Channel          — PBDMA discovery, channel creation
//! ```
//!
//! Each stage is independently validatable against a nouveau reference BAR0
//! snapshot using [`super::super::vfio::channel::diagnostic::subsystem_validator`].

use crate::vfio::channel::hbm2_training::{self, TrainingBackend};
use crate::vfio::device::{DmaBackend, MappedBar};

use std::borrow::Cow;

/// DEVINIT status register — bit 1 set means DEVINIT has completed.
const DEVINIT_STATUS_REG: usize = 0x0002_240C;

/// GPU topology discovered during init.
#[derive(Debug, Clone)]
pub struct GpuTopology {
    pub gpc_count: u32,
    pub tpc_per_gpc: Vec<u32>,
    pub sm_count: u32,
    pub fbp_count: u32,
    pub ltc_count: u32,
    pub pbdma_mask: u32,
    pub pbdma_count: u32,
    pub gr_runlist_id: u32,
}

/// Result of a single init stage.
#[derive(Debug)]
pub struct StageResult {
    pub stage: &'static str,
    pub writes_applied: u32,
    pub writes_failed: u32,
    pub duration_us: u64,
}

impl StageResult {
    pub fn ok(&self) -> bool {
        self.writes_failed == 0
    }
}

/// Full init result.
#[derive(Debug)]
pub struct SovereignInitResult {
    /// Per-stage results.
    pub stages: Vec<StageResult>,
    /// Discovered GPU topology (GPC/TPC/SM/FBP/PBDMA).
    pub topology: Option<GpuTopology>,
    /// Whether HBM2 training was executed (cold boot) or skipped (warm).
    pub hbm2_trained: bool,
    /// Whether FECS/GPCCS falcons are alive after boot.
    pub falcons_alive: bool,
    /// Whether GR engine registers indicate readiness.
    pub gr_ready: bool,
    /// Whether PFIFO/PBDMA infrastructure is present.
    pub pfifo_ready: bool,
    /// Whether FECS method interface responded to a context-size query.
    pub fecs_responsive: bool,
    /// GR context info (populated when DMA backend available and FECS alive).
    pub gr_context: Option<GrContextInfo>,
}

impl SovereignInitResult {
    /// Whether every stage reported zero failures.
    pub fn all_ok(&self) -> bool {
        self.stages.iter().all(StageResult::ok)
    }

    /// Whether the GPU is ready for sovereign compute dispatch.
    ///
    /// Requires: PFIFO present, FECS responsive (or at least falcons alive).
    pub fn compute_ready(&self) -> bool {
        self.pfifo_ready && (self.fecs_responsive || self.falcons_alive)
    }

    /// Structured diagnostic summary for logging or handoff.
    pub fn diagnostic_summary(&self) -> String {
        use std::fmt::Write;
        let mut s = String::with_capacity(512);
        let _ = writeln!(s, "SovereignInit: {} stages", self.stages.len());
        for stage in &self.stages {
            let mark = if stage.ok() { "OK" } else { "FAIL" };
            let _ = writeln!(
                s,
                "  [{mark:4}] {:20} writes={}/{} {}us",
                stage.stage,
                stage.writes_applied,
                stage.writes_applied + stage.writes_failed,
                stage.duration_us,
            );
        }
        if let Some(ref t) = self.topology {
            let _ = writeln!(
                s,
                "  Topology: {}GPC {}SM {}FBP {}PBDMA runlist={}",
                t.gpc_count, t.sm_count, t.fbp_count, t.pbdma_count, t.gr_runlist_id,
            );
        }
        let _ = writeln!(s, "  HBM2={} Falcons={} GR={} PFIFO={} FECS_methods={}",
            self.hbm2_trained, self.falcons_alive, self.gr_ready,
            self.pfifo_ready, self.fecs_responsive);
        if let Some(ref ctx) = self.gr_context {
            let _ = writeln!(
                s,
                "  GR_Context: image={}B zcull={}B pm={}B iova={:#x} golden={}",
                ctx.image_size, ctx.zcull_size, ctx.pm_size, ctx.iova, ctx.golden_saved,
            );
        }
        let _ = writeln!(s, "  compute_ready={}", self.compute_ready());
        s
    }
}

/// GR context discovery info from FECS method interface.
#[derive(Debug, Clone)]
pub struct GrContextInfo {
    /// Context image size in bytes.
    pub image_size: u32,
    /// Zcull image size in bytes (0 if unsupported).
    pub zcull_size: u32,
    /// PM image size in bytes (0 if unsupported).
    pub pm_size: u32,
    /// DMA IOVA where the context buffer was allocated.
    pub iova: u64,
    /// Whether a golden save was performed.
    pub golden_saved: bool,
}

/// Sovereign GPU init — replace nouveau with pure Rust, subsystem by subsystem.
///
/// After construction, call [`init_all`] to run the full sequence, or call
/// individual `init_*` methods to run/validate one subsystem at a time.
pub struct SovereignInit<'a> {
    bar0: &'a MappedBar,
    sm_version: u32,
    chip: Cow<'a, str>,
    bdf: Option<Cow<'a, str>>,
    dma_backend: Option<DmaBackend>,
    training_backend: Option<TrainingBackend>,
}

impl<'a> SovereignInit<'a> {
    pub fn new(bar0: &'a MappedBar, sm_version: u32) -> Self {
        let chip = super::sm_to_chip(sm_version);
        Self {
            bar0,
            sm_version,
            chip: Cow::Borrowed(chip),
            bdf: None,
            dma_backend: None,
            training_backend: None,
        }
    }

    /// Set the PCI BDF address (e.g. `"0000:03:00.0"`) for sysfs operations.
    pub fn with_bdf(mut self, bdf: &'a str) -> Self {
        self.bdf = Some(Cow::Borrowed(bdf));
        self
    }

    /// Set a DMA backend for strategies that need IOMMU-mapped host memory.
    pub fn with_dma_backend(mut self, backend: DmaBackend) -> Self {
        self.dma_backend = Some(backend);
        self
    }

    /// Set the HBM2 training backend (overrides auto-detection).
    pub fn with_training_backend(mut self, backend: TrainingBackend) -> Self {
        self.training_backend = Some(backend);
        self
    }

    /// Run the full init sequence: HBM2 → PMC → Topology → PFB → Falcon → GR → PFIFO.
    ///
    /// This is the sovereign replacement for nouveau's `nvidia_drm_load` →
    /// `nvkm_device_init` call chain.
    pub fn init_all(&self) -> SovereignInitResult {
        let mut stages = Vec::new();
        let mut topology = None;

        // Stage 0: HBM2 Training (cold boot only — auto-detected)
        let hbm2_result = self.init_hbm2();
        let hbm2_trained = hbm2_result.ok();
        stages.push(hbm2_result);

        // Stage 1: PMC + Engine Gating
        stages.push(self.init_pmc());

        // Stage 2: Topology Discovery
        match self.init_topology() {
            (result, Some(topo)) => {
                topology = Some(topo);
                stages.push(result);
            }
            (result, None) => {
                stages.push(result);
            }
        }

        // Stage 3: PFB + Memory Controller
        stages.push(self.init_pfb());

        // Stage 3.5: PRI Ring Reset (clear faults from engine ungating)
        stages.push(self.reset_pri_ring());

        // Stage 4: Falcon Boot (SEC2 → ACR → FECS/GPCCS via solver)
        let falcon_result = self.init_falcons();
        let falcons_alive = falcon_result.ok();
        stages.push(falcon_result);

        // Stage 5: GR Engine Init (includes FECS method probe)
        let gr_result = self.init_gr();
        let gr_ready = gr_result.ok();
        stages.push(gr_result);

        let fecs_responsive = self.probe_fecs_methods();

        // Stage 6: PFIFO Discovery (actual channel creation is separate)
        let pfifo_result = self.init_pfifo_discovery();
        let pfifo_ready = pfifo_result.ok();
        stages.push(pfifo_result);

        // Stage 7: GR Context Setup (requires DMA + FECS alive)
        let gr_context = if self.dma_backend.is_some() {
            let (ctx_result, ctx_info) = self.init_gr_context();
            stages.push(ctx_result);
            ctx_info
        } else {
            tracing::info!("sovereign init: skipping GR context (no DMA backend)");
            None
        };

        SovereignInitResult {
            stages,
            topology,
            hbm2_trained,
            falcons_alive,
            gr_ready,
            pfifo_ready,
            fecs_responsive,
            gr_context,
        }
    }

    // ── Stage 0: HBM2 Training ──────────────────────────────────────

    /// Detect cold vs warm boot and train HBM2 if needed.
    ///
    /// On cold boot (DEVINIT not done), runs the HBM2 training pipeline
    /// via the configured backend (or auto-detects: PROM -> sysfs -> file).
    /// On warm boot (HBM2 already trained), skips training and verifies VRAM.
    pub fn init_hbm2(&self) -> StageResult {
        let start = std::time::Instant::now();

        let devinit_status = self.bar0.read_u32(DEVINIT_STATUS_REG).unwrap_or(0);
        let needs_post = (devinit_status & 2) == 0;

        tracing::info!(
            devinit_status = format_args!("{devinit_status:#010x}"),
            needs_post,
            "sovereign init: HBM2 stage — probing DEVINIT status"
        );

        if !needs_post {
            let vram_alive = self.probe_vram();
            tracing::info!(
                vram_alive,
                "sovereign init: HBM2 warm — DEVINIT already complete"
            );
            return StageResult {
                stage: "HBM2_TRAINING",
                writes_applied: 0,
                writes_failed: if vram_alive { 0 } else { 1 },
                duration_us: start.elapsed().as_micros() as u64,
            };
        }

        tracing::info!("sovereign init: HBM2 cold boot — running training pipeline");
        let bdf_ref = self.bdf.as_deref();
        let result = hbm2_training::train_hbm2(
            self.bar0,
            bdf_ref,
            self.training_backend.clone(),
        );

        match result {
            Ok(ctrl) => {
                let log = ctrl.training_log();
                let writes = log.write_count() as u32;
                let vram_alive = self.probe_vram();
                tracing::info!(
                    writes,
                    vram_alive,
                    "sovereign init: HBM2 training complete"
                );
                StageResult {
                    stage: "HBM2_TRAINING",
                    writes_applied: writes,
                    writes_failed: if vram_alive { 0 } else { 1 },
                    duration_us: start.elapsed().as_micros() as u64,
                }
            }
            Err((err, _log)) => {
                tracing::error!(
                    phase = err.phase,
                    detail = %err.detail,
                    "sovereign init: HBM2 training failed"
                );
                StageResult {
                    stage: "HBM2_TRAINING",
                    writes_applied: 0,
                    writes_failed: 1,
                    duration_us: start.elapsed().as_micros() as u64,
                }
            }
        }
    }

    /// Quick VRAM liveness check via PRAMIN sentinel write/readback.
    fn probe_vram(&self) -> bool {
        use crate::vfio::memory::{MemoryRegion, PraminRegion};
        if let Ok(mut region) = PraminRegion::new(self.bar0, 0x0002_6000, 8) {
            region.probe_sentinel(0, 0xCAFE_DEAD).is_working()
        } else {
            false
        }
    }

    // ── Stage 1: PMC + Engine Gating ─────────────────────────────────

    /// Un-gate all engine clock domains via PMC_ENABLE.
    ///
    /// Equivalent to nouveau's `gp100_mc_init()` + glow-plug sequence.
    pub fn init_pmc(&self) -> StageResult {
        let start = std::time::Instant::now();
        let mut applied = 0u32;
        let mut failed = 0u32;

        let w = |off: usize, val: u32| -> bool {
            self.bar0.write_u32(off, val).is_ok()
        };

        // Clear stale PRI faults first
        self.clear_pri_faults();

        // PMC_ENABLE = 0xFFFFFFFF — un-gate all engine clock domains
        if w(0x200, 0xFFFF_FFFF) { applied += 1; } else { failed += 1; }
        std::thread::sleep(std::time::Duration::from_millis(10));

        // PMC UNK260 = 0 → PMC PFIFO bit toggle → UNK260 = 1
        // (nouveau's clock distribution gate bracket)
        if w(0x260, 0) { applied += 1; } else { failed += 1; }
        std::thread::sleep(std::time::Duration::from_millis(1));

        let pmc_cur = self.bar0.read_u32(0x200).unwrap_or(0);
        const PFIFO_BIT: u32 = 1 << 8;
        if w(0x200, pmc_cur & !PFIFO_BIT) { applied += 1; } else { failed += 1; }
        std::thread::sleep(std::time::Duration::from_millis(5));
        if w(0x200, pmc_cur | PFIFO_BIT) { applied += 1; } else { failed += 1; }
        std::thread::sleep(std::time::Duration::from_millis(10));

        if w(0x260, 1) { applied += 1; } else { failed += 1; }
        std::thread::sleep(std::time::Duration::from_millis(1));

        let pmc_after = self.bar0.read_u32(0x200).unwrap_or(0);
        tracing::info!(
            pmc_after = format_args!("{pmc_after:#010x}"),
            applied,
            "sovereign init: PMC engine gating complete"
        );

        StageResult {
            stage: "PMC_ENGINE_GATING",
            writes_applied: applied,
            writes_failed: failed,
            duration_us: start.elapsed().as_micros() as u64,
        }
    }

    // ── Stage 2: Topology Discovery ──────────────────────────────────

    /// Discover GPU topology: GPC/TPC/SM/FBP counts, PBDMA map.
    ///
    /// Equivalent to nouveau's `gk104_fifo_oneinit()` topology reads +
    /// `gf100_gr_oneinit()` GPC/TPC enumeration.
    pub fn init_topology(&self) -> (StageResult, Option<GpuTopology>) {
        let start = std::time::Instant::now();
        let r = |off: usize| self.bar0.read_u32(off).unwrap_or(0);

        // PTOP device info table (0x22430+)
        // GPC count from FUSE register 0x021C04
        let gpc_mask = r(0x022438);
        let gpc_count = gpc_mask.count_ones();

        // TPC count per GPC from GPC broadcast registers
        // 0x41BE90 = GPC_BROADCAST TPC_DISABLE mask
        let tpc_disable = r(0x41BE90);
        let tpc_mask = !tpc_disable;
        let max_tpc_per_gpc = (tpc_mask & 0xFF).count_ones();
        let tpc_per_gpc = vec![max_tpc_per_gpc; gpc_count as usize];
        let sm_count = gpc_count * max_tpc_per_gpc * 2; // 2 SMs per TPC on Volta

        // FBP count from 0x12006C
        let fbp_count = r(0x12006C) & 0xF;

        // Active LTC count from FBHUB (0x100800)
        let ltc_count = r(0x100800);

        // PBDMA map from PFIFO (0x2004)
        let pbdma_mask = r(0x2004);
        let pbdma_count = pbdma_mask.count_ones();

        // Discover GR runlist ID from engine topology table (0x22700+)
        let mut gr_runlist_id = 1u32; // default for GV100
        for i in 0..32u32 {
            let entry = r(0x22700 + i as usize * 4);
            // Engine type 0 = GR, bits [7:0] = type, [15:8] = runlist
            if entry & 0xFF == 0 {
                gr_runlist_id = (entry >> 8) & 0xFF;
                break;
            }
        }

        tracing::info!(
            gpc_count,
            max_tpc_per_gpc,
            sm_count,
            fbp_count,
            ltc_count,
            pbdma_mask = format_args!("{pbdma_mask:#010x}"),
            pbdma_count,
            gr_runlist_id,
            "sovereign init: topology discovery complete"
        );

        let topo = GpuTopology {
            gpc_count,
            tpc_per_gpc,
            sm_count,
            fbp_count,
            ltc_count,
            pbdma_mask,
            pbdma_count,
            gr_runlist_id,
        };

        let result = StageResult {
            stage: "PRI_TOPOLOGY",
            writes_applied: 0, // read-only stage
            writes_failed: 0,
            duration_us: start.elapsed().as_micros() as u64,
        };

        (result, Some(topo))
    }

    // ── Stage 3: PFB + Memory Controller ─────────────────────────────

    /// Configure PFB, FBHUB, and MMU fault buffers.
    ///
    /// On warm handoff, HBM2 is already trained (silicon BootROM did this).
    /// We configure the NISO flush target and verify FBHUB is responsive.
    pub fn init_pfb(&self) -> StageResult {
        let start = std::time::Instant::now();
        let mut applied = 0u32;
        let mut failed = 0u32;

        let w = |off: usize, val: u32| -> bool {
            self.bar0.write_u32(off, val).is_ok()
        };
        let r = |off: usize| self.bar0.read_u32(off).unwrap_or(0);

        // Verify PFB is accessible (not PRI-gated)
        let pfb_cfg0 = r(0x100000);
        if crate::vfio::channel::registers::pri::is_pri_error(pfb_cfg0) {
            tracing::warn!(
                pfb_cfg0 = format_args!("{pfb_cfg0:#010x}"),
                "PFB is PRI-gated — cannot configure memory controller"
            );
            return StageResult {
                stage: "PFB_MEMORY",
                writes_applied: 0,
                writes_failed: 1,
                duration_us: start.elapsed().as_micros() as u64,
            };
        }

        // NISO flush target — point at a safe DMA address (0 = disabled)
        if w(0x100B20, 0) { applied += 1; } else { failed += 1; }
        if w(0x100B24, 0) { applied += 1; } else { failed += 1; }

        // MMU control — read and verify
        let mmu_ctrl = r(0x100C80);
        tracing::info!(
            pfb_cfg0 = format_args!("{pfb_cfg0:#010x}"),
            mmu_ctrl = format_args!("{mmu_ctrl:#010x}"),
            applied,
            "sovereign init: PFB memory controller configured"
        );

        StageResult {
            stage: "PFB_MEMORY",
            writes_applied: applied,
            writes_failed: failed,
            duration_us: start.elapsed().as_micros() as u64,
        }
    }

    // ── Stage 4: Falcon Boot Chain ───────────────────────────────────

    /// Pre-configure FECS/GPCCS interrupt and interface enables.
    ///
    /// Nouveau has IRQMODE + ITFEN set from its GR init path. Without them,
    /// FECS gets stuck polling for interrupts after ACR boots it. This must
    /// run **before** the boot solver so all strategies benefit.
    fn prepare_falcon_interrupts(&self) -> u32 {
        use crate::vfio::channel::registers::falcon;
        let mut writes = 0u32;

        let w = |off: usize, val: u32| -> bool {
            self.bar0.write_u32(off, val).is_ok()
        };

        // IRQMODE 0xfc24: nouveau's standard interrupt routing for FECS/GPCCS
        if w(falcon::FECS_BASE + falcon::IRQMODE, 0x0000_fc24) { writes += 1; }
        if w(falcon::GPCCS_BASE + falcon::IRQMODE, 0x0000_fc24) { writes += 1; }
        // ITFEN bit 2 = ACCESS_EN: enable DMA interface for falcon firmware
        if w(falcon::FECS_BASE + falcon::ITFEN, 0x0000_0004) { writes += 1; }
        if w(falcon::GPCCS_BASE + falcon::ITFEN, 0x0000_0004) { writes += 1; }

        // IRQMASK: enable all interrupt sources that FECS expects
        if w(falcon::FECS_BASE + 0x018, 0xFFFF_FFFF) { writes += 1; }
        if w(falcon::GPCCS_BASE + 0x018, 0xFFFF_FFFF) { writes += 1; }

        tracing::info!(writes, "sovereign init: falcon IRQMODE/ITFEN/IRQMASK configured");
        writes
    }

    /// Boot FECS/GPCCS falcons via the ACR boot solver.
    ///
    /// On warm handoff (FECS already running), this is a read-only validation.
    /// On cold boot, first pre-configures interrupt enables (IRQMODE/ITFEN)
    /// that nouveau normally sets from its GR init path, then runs the full
    /// SEC2→ACR→FECS/GPCCS chain via [`super::acr_boot::FalconBootSolver`],
    /// Reset the PRI ring to clear stale faults after engine ungating.
    ///
    /// After PMC_ENABLE writes ungate all engines, the PRI ring accumulates
    /// faults from stale engine state (left over from previous driver teardown).
    /// This makes falcon registers read as 0xbad00100. A GR engine reset +
    /// PRI fault drain restores access.
    pub fn reset_pri_ring(&self) -> StageResult {
        let start = std::time::Instant::now();
        let mut applied = 0u32;
        let w = |off: usize, val: u32| -> bool {
            self.bar0.write_u32(off, val).is_ok()
        };

        // 1. GR engine reset via PMC: toggle bit 12 off then on
        let pmc_cur = self.bar0.read_u32(0x200).unwrap_or(0);
        const GR_BIT: u32 = 1 << 12;
        if w(0x200, pmc_cur & !GR_BIT) { applied += 1; }
        std::thread::sleep(std::time::Duration::from_millis(10));
        if w(0x200, pmc_cur | GR_BIT) { applied += 1; }
        std::thread::sleep(std::time::Duration::from_millis(10));

        // 2. PRI ring master: reset + init
        //    GPC PRIV ring: 0x12_8100 = GP100 priv_ring master INTR route
        //    0x12004C = PRI_RINGMASTER_COMMAND: 0x4 = PRI ring enumerate
        if w(0x12004C, 0x4) { applied += 1; }
        std::thread::sleep(std::time::Duration::from_millis(20));

        // 3. Clear all PRI faults (drain up to 10 times)
        for _ in 0..10 {
            let intr = self.bar0.read_u32(0x120058).unwrap_or(0);
            if intr == 0 { break; }
            let _ = self.bar0.write_u32(0x12004C, 0x2); // ACK
            applied += 1;
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        // 4. Verify: probe a falcon register — should NOT be 0xbad00100
        let fecs_probe = self.bar0.read_u32(0x409100).unwrap_or(0xDEAD_DEAD);
        let pri_ok = !crate::vfio::channel::registers::pri::is_pri_error(fecs_probe);

        tracing::info!(
            fecs_probe = format_args!("{fecs_probe:#010x}"),
            pri_ok,
            applied,
            "sovereign init: PRI ring reset complete"
        );

        StageResult {
            stage: "PRI_RING_RESET",
            writes_applied: applied,
            writes_failed: if pri_ok { 0 } else { 1 },
            duration_us: start.elapsed().as_micros() as u64,
        }
    }

    /// passing the DMA backend for strategies that need IOMMU-mapped host memory.
    pub fn init_falcons(&self) -> StageResult {
        let start = std::time::Instant::now();
        let r = |off: usize| self.bar0.read_u32(off).unwrap_or(0xDEAD_DEAD);

        // Clear any lingering PRI faults before probing falcon registers.
        self.clear_pri_faults();

        let fecs_cpuctl = r(0x409100);
        let fecs_sctl = r(0x409240);
        let fecs_mb0 = r(0x409040);
        let gpccs_cpuctl = r(0x41A100);
        let gpccs_sctl = r(0x41A240);

        // If all registers read as PRI error, the ring is still broken.
        if crate::vfio::channel::registers::pri::is_pri_error(fecs_cpuctl)
            && crate::vfio::channel::registers::pri::is_pri_error(gpccs_cpuctl)
        {
            tracing::error!(
                fecs_cpuctl = format_args!("{fecs_cpuctl:#010x}"),
                gpccs_cpuctl = format_args!("{gpccs_cpuctl:#010x}"),
                "sovereign init: PRI ring still broken — falcon registers inaccessible"
            );
            return StageResult {
                stage: "FALCON_BOOT",
                writes_applied: 0,
                writes_failed: 1,
                duration_us: start.elapsed().as_micros() as u64,
            };
        }

        let fecs_running = fecs_mb0 != 0
            && fecs_cpuctl & 0x10 == 0
            && !crate::vfio::channel::registers::pri::is_pri_error(fecs_cpuctl);
        let gpccs_alive = gpccs_cpuctl & 0x10 != 0 || gpccs_sctl != 0;

        tracing::info!(
            fecs_cpuctl = format_args!("{fecs_cpuctl:#010x}"),
            fecs_sctl = format_args!("{fecs_sctl:#010x}"),
            fecs_mb0 = format_args!("{fecs_mb0:#010x}"),
            gpccs_cpuctl = format_args!("{gpccs_cpuctl:#010x}"),
            gpccs_sctl = format_args!("{gpccs_sctl:#010x}"),
            fecs_running,
            gpccs_alive,
            "sovereign init: falcon state probe"
        );

        if fecs_running && gpccs_alive {
            tracing::info!("sovereign init: FECS already running — skipping boot solver");
            return StageResult {
                stage: "FALCON_BOOT",
                writes_applied: 0,
                writes_failed: 0,
                duration_us: start.elapsed().as_micros() as u64,
            };
        }

        // Pre-configure IRQMODE/ITFEN before any boot strategy runs.
        // Without these, FECS gets stuck polling for interrupts after ACR
        // bootstraps it — nouveau sets them from GR init which runs first.
        let irq_writes = self.prepare_falcon_interrupts();

        tracing::info!(
            chip = %self.chip,
            has_dma = self.dma_backend.is_some(),
            irq_writes,
            "sovereign init: FECS not running — invoking boot solver"
        );

        let solver_result = super::acr_boot::FalconBootSolver::boot(
            self.bar0,
            &self.chip,
            self.dma_backend.clone(),
            None,
        );

        match solver_result {
            Ok(results) => {
                let any_success = results.iter().any(|r| r.success);
                let strategy_count = results.len() as u32;

                if any_success {
                    let winner = results.iter().find(|r| r.success).unwrap();
                    tracing::info!(
                        strategy = winner.strategy,
                        attempts = strategy_count,
                        "sovereign init: falcon boot solver SUCCEEDED"
                    );
                } else {
                    tracing::warn!(
                        attempts = strategy_count,
                        "sovereign init: falcon boot solver exhausted all strategies"
                    );
                }

                StageResult {
                    stage: "FALCON_BOOT",
                    writes_applied: irq_writes + strategy_count,
                    writes_failed: if any_success { 0 } else { 1 },
                    duration_us: start.elapsed().as_micros() as u64,
                }
            }
            Err(e) => {
                tracing::error!(error = %e, "sovereign init: falcon boot solver error");
                StageResult {
                    stage: "FALCON_BOOT",
                    writes_applied: irq_writes,
                    writes_failed: 1,
                    duration_us: start.elapsed().as_micros() as u64,
                }
            }
        }
    }

    // ── Stage 5: GR Engine Init ──────────────────────────────────────

    /// Apply GR engine init: firmware BAR0 writes + dynamic configuration.
    ///
    /// Delegates to the standalone `apply_gr_bar0_init` which loads firmware
    /// blobs from `/lib/firmware/nvidia/{chip}/gr/` and applies them.
    /// After BAR0 writes, probes the FECS method interface to confirm
    /// the GR engine is actually responsive (not just register-level OK).
    pub fn init_gr(&self) -> StageResult {
        let start = std::time::Instant::now();

        let (applied, failed, _fecs_count) =
            super::init::apply_gr_bar0_init(self.bar0, self.sm_version);

        let gr_status = self.bar0.read_u32(0x400700).unwrap_or(0xDEAD);
        let gr_enable = self.bar0.read_u32(0x400500).unwrap_or(0);
        let pgraph_intr = self.bar0.read_u32(0x400100).unwrap_or(0);

        // Probe FECS method interface after GR init — this tells us whether
        // the falcon is actually responsive (not just that registers were written).
        let fecs_responsive = self.probe_fecs_methods();

        tracing::info!(
            gr_status = format_args!("{gr_status:#010x}"),
            gr_enable = format_args!("{gr_enable:#010x}"),
            pgraph_intr = format_args!("{pgraph_intr:#010x}"),
            applied,
            failed,
            fecs_responsive,
            "sovereign init: GR engine init complete"
        );

        let gr_enable_ok = gr_enable == 0x0001_0001;

        StageResult {
            stage: "GR_ENGINE_INIT",
            writes_applied: applied,
            writes_failed: failed + if gr_enable_ok { 0 } else { 1 },
            duration_us: start.elapsed().as_micros() as u64,
        }
    }

    // ── Stage 6: PFIFO Discovery ─────────────────────────────────────

    /// Discover PFIFO state: PBDMA map, scheduler enable, engine table.
    ///
    /// Actual channel creation is done separately via `VfioChannel::create_*`.
    /// This stage validates that PFIFO is in a state where channel creation
    /// can succeed.
    pub fn init_pfifo_discovery(&self) -> StageResult {
        let start = std::time::Instant::now();
        let r = |off: usize| self.bar0.read_u32(off).unwrap_or(0xDEAD_DEAD);

        let pfifo_enable = r(0x2200);
        let sched_en = r(0x2504);
        let pbdma_map = r(0x2004);
        let pfifo_intr = r(0x2100);

        // PBDMA discovery
        let mut pbdma_ids = Vec::new();
        for id in 0..32usize {
            if pbdma_map & (1 << id) != 0 {
                pbdma_ids.push(id);
            }
        }

        // Engine topology table
        let mut engines = Vec::new();
        for i in 0..32u32 {
            let entry = r(0x22700 + i as usize * 4);
            if entry == 0 || entry == 0xDEAD_DEAD {
                break;
            }
            let engine_type = entry & 0xFF;
            let runlist_id = (entry >> 8) & 0xFF;
            engines.push((engine_type, runlist_id));
        }

        tracing::info!(
            pfifo_enable = format_args!("{pfifo_enable:#010x}"),
            sched_en = format_args!("{sched_en:#010x}"),
            pbdma_map = format_args!("{pbdma_map:#010x}"),
            pfifo_intr = format_args!("{pfifo_intr:#010x}"),
            pbdma_count = pbdma_ids.len(),
            engine_count = engines.len(),
            "sovereign init: PFIFO discovery complete"
        );

        let ok = !pbdma_ids.is_empty();

        StageResult {
            stage: "PFIFO_CHANNEL",
            writes_applied: 0, // read-only discovery
            writes_failed: if ok { 0 } else { 1 },
            duration_us: start.elapsed().as_micros() as u64,
        }
    }

    // ── Stage 7 (optional): GR Context Setup ───────────────────────

    /// Configure FECS exceptions, discover context image size, allocate
    /// a DMA-backed context buffer, bind it, and perform a golden save.
    ///
    /// Requires a DMA backend (passed at construction via `with_dma_backend`).
    /// Returns `None` if FECS is not responsive or no DMA backend.
    /// On success, returns `(StageResult, GrContextInfo)`.
    pub fn init_gr_context(&self) -> (StageResult, Option<GrContextInfo>) {
        use super::acr_boot::fecs_method;
        use super::gr_context;
        use crate::vfio::dma::DmaBuffer;

        let start = std::time::Instant::now();

        if !gr_context::fecs_is_alive(self.bar0) {
            tracing::info!("sovereign init: FECS not alive — skipping GR context setup");
            return (
                StageResult {
                    stage: "GR_CONTEXT",
                    writes_applied: 0,
                    writes_failed: 0,
                    duration_us: start.elapsed().as_micros() as u64,
                },
                None,
            );
        }

        let backend = match &self.dma_backend {
            Some(b) => b.clone(),
            None => {
                tracing::info!("sovereign init: no DMA backend — skipping GR context alloc");
                return (
                    StageResult {
                        stage: "GR_CONTEXT",
                        writes_applied: 0,
                        writes_failed: 0,
                        duration_us: start.elapsed().as_micros() as u64,
                    },
                    None,
                );
            }
        };

        fecs_method::fecs_init_exceptions(self.bar0);

        let (image_size, zcull_size, pm_size) = match gr_context::discover_context_sizes(self.bar0)
        {
            Ok(sizes) => sizes,
            Err(e) => {
                tracing::warn!(error = %e, "sovereign init: FECS context size query failed");
                return (
                    StageResult {
                        stage: "GR_CONTEXT",
                        writes_applied: 0,
                        writes_failed: 1,
                        duration_us: start.elapsed().as_micros() as u64,
                    },
                    None,
                );
            }
        };

        let alloc_size = (image_size as usize).max(4096);
        let ctx_iova = 0x0010_0000_u64;
        let ctx_buf = match DmaBuffer::new(backend, alloc_size, ctx_iova) {
            Ok(b) => b,
            Err(e) => {
                tracing::warn!(error = %e, "sovereign init: GR context DMA alloc failed");
                return (
                    StageResult {
                        stage: "GR_CONTEXT",
                        writes_applied: 0,
                        writes_failed: 1,
                        duration_us: start.elapsed().as_micros() as u64,
                    },
                    Some(GrContextInfo {
                        image_size,
                        zcull_size,
                        pm_size,
                        iova: 0,
                        golden_saved: false,
                    }),
                );
            }
        };

        let golden_saved = match gr_context::bind_and_golden_save(self.bar0, ctx_iova) {
            Ok(ctx) => {
                tracing::info!(
                    image_size = ctx.image_size,
                    iova = format_args!("{:#x}", ctx.iova),
                    "sovereign init: GR context golden save complete"
                );
                ctx.golden_saved
            }
            Err(e) => {
                tracing::warn!(error = %e, "sovereign init: GR context bind/save failed");
                false
            }
        };

        std::mem::forget(ctx_buf);

        (
            StageResult {
                stage: "GR_CONTEXT",
                writes_applied: if golden_saved { 1 } else { 0 },
                writes_failed: if golden_saved { 0 } else { 1 },
                duration_us: start.elapsed().as_micros() as u64,
            },
            Some(GrContextInfo {
                image_size,
                zcull_size,
                pm_size,
                iova: ctx_iova,
                golden_saved,
            }),
        )
    }

    // ── Helpers ──────────────────────────────────────────────────────

    /// Probe the FECS method interface to check GR engine liveness.
    ///
    /// Returns `true` if FECS responds to a context-size query.
    /// This goes beyond register reads: it confirms the falcon firmware
    /// is executing and its command loop is operational.
    fn probe_fecs_methods(&self) -> bool {
        use super::gr_context;
        if !gr_context::fecs_is_alive(self.bar0) {
            return false;
        }
        match gr_context::discover_context_sizes(self.bar0) {
            Ok((sz, _, _)) => {
                tracing::info!(
                    context_image_size = sz,
                    "sovereign init: FECS method interface ALIVE"
                );
                sz > 0
            }
            Err(e) => {
                tracing::debug!(
                    error = %e,
                    "sovereign init: FECS method probe failed (expected on cold boot)"
                );
                false
            }
        }
    }

    fn clear_pri_faults(&self) {
        let priv_intr = self.bar0.read_u32(0x120058).unwrap_or(0);
        if priv_intr != 0 {
            for attempt in 0..5 {
                let _ = self.bar0.write_u32(0x12004C, 0x2); // ACK
                std::thread::sleep(std::time::Duration::from_millis(20));
                let status = self.bar0.read_u32(0x120058).unwrap_or(0);
                if status == 0 {
                    tracing::debug!(attempt, "PRI fault cleared");
                    break;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stage_result_ok() {
        let ok = StageResult {
            stage: "PMC",
            writes_applied: 5,
            writes_failed: 0,
            duration_us: 100,
        };
        assert!(ok.ok());

        let bad = StageResult {
            stage: "PMC",
            writes_applied: 3,
            writes_failed: 2,
            duration_us: 100,
        };
        assert!(!bad.ok());
    }

    #[test]
    fn init_result_all_ok() {
        let result = SovereignInitResult {
            stages: vec![
                StageResult { stage: "A", writes_applied: 1, writes_failed: 0, duration_us: 0 },
                StageResult { stage: "B", writes_applied: 2, writes_failed: 0, duration_us: 0 },
            ],
            topology: None,
            hbm2_trained: true,
            falcons_alive: true,
            gr_ready: true,
            pfifo_ready: true,
            fecs_responsive: true,
            gr_context: None,
        };
        assert!(result.all_ok());
        assert!(result.compute_ready());
    }

    #[test]
    fn diagnostic_summary_format() {
        let result = SovereignInitResult {
            stages: vec![
                StageResult { stage: "HBM2", writes_applied: 10, writes_failed: 0, duration_us: 500 },
            ],
            topology: None,
            hbm2_trained: true,
            falcons_alive: false,
            gr_ready: false,
            pfifo_ready: true,
            fecs_responsive: false,
            gr_context: None,
        };
        let summary = result.diagnostic_summary();
        assert!(summary.contains("HBM2"));
        assert!(summary.contains("compute_ready=false"));
    }
}
