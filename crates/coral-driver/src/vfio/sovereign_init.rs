// SPDX-License-Identifier: AGPL-3.0-or-later
//! Staged sovereign GPU initialization pipeline.
//!
//! Orchestrates the full path from cold/warm VFIO device to compute-ready state.
//! Each stage runs through fork-isolated MMIO where possible so that a BAR0
//! D-state kills only the probing child, not the ember daemon.
//!
//! # Stages
//!
//! ```text
//! 1. bar0_probe     — Chip ID verification, PMC liveness check
//! 2. pmc_enable     — Master clock/engine enable (PMC_ENABLE = 0xFFFF_FFFF)
//! 3. hbm2_training  — Memory controller bring-up via typestate pipeline
//! 4. falcon_boot    — SEC2/ACR secure boot, then FECS/GPCCS GR falcons
//! 5. gr_init        — GR engine BAR0 register programming
//! 6. verify         — Final VRAM sentinel test and falcon health check
//! ```
//!
//! # Contract
//!
//! The pipeline returns [`SovereignInitResult`] with per-stage outcomes.
//! Glowplug expects `all_ok`, `compute_ready`, and `halted_at` fields.

use std::fmt;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::vfio::channel::hbm2_training::{self, Hbm2Controller, TrainingLog};
use crate::vfio::device::MappedBar;

const PMC_BOOT_0: usize = 0x0000_0000;
const PMC_ENABLE: usize = 0x0000_0200;
const PMC_INTR_EN_0: usize = 0x0000_0140;
const PTIMER_TIME_0: usize = 0x0000_9400;
const PTIMER_TIME_1: usize = 0x0000_9410;

const ISOLATE_TIMEOUT: Duration = Duration::from_secs(3);

/// Which stage to halt before (for debugging partial pipelines).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HaltBefore {
    /// Halt before master clock/engine enable.
    PmcEnable,
    /// Halt before HBM2 memory controller bring-up.
    Hbm2Training,
    /// Halt before falcon (SEC2/ACR/FECS) boot.
    FalconBoot,
    /// Halt before GR engine register programming.
    GrInit,
    /// Halt before final VRAM/PTIMER verification.
    Verify,
}

/// Options controlling the sovereign init pipeline.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SovereignInitOptions {
    /// Halt the pipeline before this stage (for experiments).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub halt_before: Option<HaltBefore>,
    /// Golden register captures for differential HBM2 replay.
    #[serde(skip)]
    pub golden_state: Option<Vec<(usize, u32)>>,
    /// File path to a JSON golden-state capture (loaded by the RPC handler).
    /// Format: array of `[offset, value]` pairs, or a `TrainingRecipe` JSON.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub golden_state_path: Option<String>,
    /// Explicit VBIOS ROM bytes (otherwise read from PROM/sysfs).
    #[serde(skip)]
    pub vbios_rom: Option<Vec<u8>>,
    /// File path to a raw VBIOS ROM dump (loaded by the RPC handler).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vbios_rom_path: Option<String>,
    /// Number of FBPA partitions (auto-detected if None).
    pub fbpa_count: Option<usize>,
    /// SM version for GR init (70 = GV100, 75 = TU102, etc.).
    pub sm_version: Option<u32>,
    /// Skip GR init even if falcon boot succeeds.
    #[serde(default)]
    pub skip_gr_init: bool,
    /// DMA backend for system-memory ACR boot (IOMMU-mapped buffers).
    /// When provided, the ACR boot solver can use strategies that place
    /// the WPR in system memory rather than VRAM-only paths.
    #[serde(skip)]
    pub dma_backend: Option<crate::vfio::device::DmaBackend>,
}

/// Outcome of a single pipeline stage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StageResult {
    /// Stage identifier (e.g. `"bar0_probe"`, `"hbm2_training"`).
    pub name: String,
    /// Whether the stage passed, was skipped, or failed.
    pub status: StageStatus,
    /// Human-readable detail about the stage outcome.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// Wall-clock duration in milliseconds.
    pub duration_ms: u64,
}

/// Status of a sovereign init stage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StageStatus {
    /// Stage completed successfully.
    Ok,
    /// Stage was not needed or halted by request.
    Skipped,
    /// Stage failed (see `StageResult::detail`).
    Failed,
}

/// Full result of the sovereign init pipeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SovereignInitResult {
    /// PCI BDF address of the device.
    pub bdf: String,
    /// Decoded chip ID from BOOT0 (e.g. 0x140 for GV100).
    pub chip_id: u32,
    /// Raw BOOT0 register value.
    pub boot0: u32,
    /// True if every executed stage passed.
    pub all_ok: bool,
    /// True if the full pipeline completed and GPU is compute-ready.
    pub compute_ready: bool,
    /// Stage name at which the pipeline was halted (by request or failure).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub halted_at: Option<String>,
    /// Per-stage results in execution order.
    pub stages: Vec<StageResult>,
    /// Total pipeline wall-clock time in milliseconds.
    pub total_ms: u64,
    /// Number of HBM2 training register writes (if training ran).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hbm2_writes: Option<usize>,
    /// Whether the GPU was detected as warm (HBM2 training skipped/reduced).
    #[serde(default)]
    pub warm_detected: bool,
}

impl fmt::Display for SovereignInitResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let status = if self.compute_ready {
            "COMPUTE_READY"
        } else if let Some(h) = &self.halted_at {
            return write!(f, "HALTED@{h} ({}ms)", self.total_ms);
        } else {
            "INCOMPLETE"
        };
        write!(
            f,
            "{status} chip=0x{:03x} stages={}/{} ({}ms)",
            self.chip_id,
            self.stages.iter().filter(|s| s.status == StageStatus::Ok).count(),
            self.stages.len(),
            self.total_ms,
        )
    }
}

/// Run the full sovereign init pipeline on a VFIO-held GPU.
///
/// `bar0` must be a valid mapped BAR0 region from an active VFIO device.
/// All MMIO in the probe stage uses fork isolation; the HBM2 and GR stages
/// use direct BAR0 access (the controller's r/w helpers already have PRI
/// fault recovery).
pub fn sovereign_init(
    bar0: &MappedBar,
    bdf: &str,
    opts: &SovereignInitOptions,
) -> SovereignInitResult {
    let pipeline_start = Instant::now();
    let mut stages: Vec<StageResult> = Vec::new();
    let mut chip_id = 0u32;
    let mut boot0 = 0u32;
    let mut training_log: Option<TrainingLog> = None;
    let mut warm_detected = false;

    // ── Stage 1: BAR0 Probe ─────────────────────────────────────────────
    let t = Instant::now();
    match bar0_probe(bar0) {
        Ok((b0, cid)) => {
            boot0 = b0;
            chip_id = cid;
            stages.push(StageResult {
                name: "bar0_probe".into(),
                status: StageStatus::Ok,
                detail: Some(format!("boot0=0x{boot0:08x} chip=0x{chip_id:03x}")),
                duration_ms: t.elapsed().as_millis() as u64,
            });
        }
        Err(e) => {
            stages.push(StageResult {
                name: "bar0_probe".into(),
                status: StageStatus::Failed,
                detail: Some(e),
                duration_ms: t.elapsed().as_millis() as u64,
            });
            return finish(bdf, boot0, chip_id, stages, None, pipeline_start, false);
        }
    }

    // ── Stage 2: PMC Enable ─────────────────────────────────────────────
    if opts.halt_before == Some(HaltBefore::PmcEnable) {
        stages.push(StageResult {
            name: "pmc_enable".into(),
            status: StageStatus::Skipped,
            detail: Some("halt_before=pmc_enable".into()),
            duration_ms: 0,
        });
        return finish_halted(bdf, boot0, chip_id, "pmc_enable", stages, pipeline_start);
    }

    let t = Instant::now();
    match pmc_enable(bar0) {
        Ok(detail) => {
            stages.push(StageResult {
                name: "pmc_enable".into(),
                status: StageStatus::Ok,
                detail: Some(detail),
                duration_ms: t.elapsed().as_millis() as u64,
            });
        }
        Err(e) => {
            stages.push(StageResult {
                name: "pmc_enable".into(),
                status: StageStatus::Failed,
                detail: Some(e),
                duration_ms: t.elapsed().as_millis() as u64,
            });
            return finish(bdf, boot0, chip_id, stages, None, pipeline_start, false);
        }
    }

    // ── Stage 3: HBM2 Training ──────────────────────────────────────────
    if opts.halt_before == Some(HaltBefore::Hbm2Training) {
        stages.push(StageResult {
            name: "hbm2_training".into(),
            status: StageStatus::Skipped,
            detail: Some("halt_before=hbm2_training".into()),
            duration_ms: 0,
        });
        return finish_halted(bdf, boot0, chip_id, "hbm2_training", stages, pipeline_start);
    }

    // Warm detection: if PMC_ENABLE has many engines on AND PRAMIN is
    // accessible, the GPU was previously trained (e.g. by nouveau warm
    // handoff). Skip the full typestate pipeline.
    // Kepler uses GDDR5, not HBM2 — the typestate pipeline doesn't apply.
    let sm = opts.sm_version.unwrap_or(chip_id_to_sm(chip_id));
    let pmc_before = bar0.read_u32(PMC_ENABLE).unwrap_or(0);
    warm_detected = is_kepler(sm) || is_warm_gpu(pmc_before, bar0);

    let t = Instant::now();
    if warm_detected {
        tracing::info!(
            pmc_enable = format!("0x{pmc_before:08x}"),
            "warm GPU detected — skipping HBM2 training"
        );
        stages.push(StageResult {
            name: "hbm2_training".into(),
            status: StageStatus::Skipped,
            detail: Some(format!(
                "warm detected (pmc=0x{pmc_before:08x}, PRAMIN sentinel ok)"
            )),
            duration_ms: t.elapsed().as_millis() as u64,
        });
    } else {
        let fbpa_count = opts.fbpa_count.unwrap_or(4);
        match run_hbm2_training(bar0, bdf, fbpa_count, opts) {
            Ok(log) => {
                let writes = log.write_count();
                training_log = Some(log);
                stages.push(StageResult {
                    name: "hbm2_training".into(),
                    status: StageStatus::Ok,
                    detail: Some(format!("{writes} register writes")),
                    duration_ms: t.elapsed().as_millis() as u64,
                });
            }
            Err(e) => {
                stages.push(StageResult {
                    name: "hbm2_training".into(),
                    status: StageStatus::Failed,
                    detail: Some(e),
                    duration_ms: t.elapsed().as_millis() as u64,
                });
                return finish(bdf, boot0, chip_id, stages, training_log, pipeline_start, warm_detected);
            }
        }
    }

    // ── Stage 4: Falcon Boot ────────────────────────────────────────────
    if opts.halt_before == Some(HaltBefore::FalconBoot) {
        stages.push(StageResult {
            name: "falcon_boot".into(),
            status: StageStatus::Skipped,
            detail: Some("halt_before=falcon_boot".into()),
            duration_ms: 0,
        });
        return finish_halted(bdf, boot0, chip_id, "falcon_boot", stages, pipeline_start);
    }

    let t = Instant::now();
    match falcon_boot(bar0, sm, opts.dma_backend.as_ref()) {
        Ok(detail) => {
            stages.push(StageResult {
                name: "falcon_boot".into(),
                status: StageStatus::Ok,
                detail: Some(detail),
                duration_ms: t.elapsed().as_millis() as u64,
            });
        }
        Err(e) => {
            stages.push(StageResult {
                name: "falcon_boot".into(),
                status: StageStatus::Failed,
                detail: Some(e),
                duration_ms: t.elapsed().as_millis() as u64,
            });
            return finish(bdf, boot0, chip_id, stages, training_log, pipeline_start, warm_detected);
        }
    }

    // ── Stage 5: GR Init ────────────────────────────────────────────────
    // Kepler FECS was already booted in the falcon_boot stage (direct PIO).
    // The GV100+ gr_init path re-boots FECS with BL firmware that doesn't
    // exist for Kepler. Skip GR init for Kepler — the falcon_boot result
    // already confirms FECS is running.
    if opts.halt_before == Some(HaltBefore::GrInit) || opts.skip_gr_init || is_kepler(sm) {
        let reason = if is_kepler(sm) {
            "kepler: FECS already booted via PIO"
        } else if opts.skip_gr_init {
            "skip_gr_init=true"
        } else {
            "halt_before=gr_init"
        };
        stages.push(StageResult {
            name: "gr_init".into(),
            status: StageStatus::Skipped,
            detail: Some(reason.into()),
            duration_ms: 0,
        });
        if opts.halt_before == Some(HaltBefore::GrInit) {
            return finish_halted(bdf, boot0, chip_id, "gr_init", stages, pipeline_start);
        }
    } else {
        let t = Instant::now();
        match gr_init(bar0, sm) {
            Ok(detail) => {
                stages.push(StageResult {
                    name: "gr_init".into(),
                    status: StageStatus::Ok,
                    detail: Some(detail),
                    duration_ms: t.elapsed().as_millis() as u64,
                });
            }
            Err(e) => {
                stages.push(StageResult {
                    name: "gr_init".into(),
                    status: StageStatus::Failed,
                    detail: Some(e),
                    duration_ms: t.elapsed().as_millis() as u64,
                });
                return finish(bdf, boot0, chip_id, stages, training_log, pipeline_start, warm_detected);
            }
        }
    }

    // ── Stage 6: Verify ─────────────────────────────────────────────────
    if opts.halt_before == Some(HaltBefore::Verify) {
        stages.push(StageResult {
            name: "verify".into(),
            status: StageStatus::Skipped,
            detail: Some("halt_before=verify".into()),
            duration_ms: 0,
        });
        return finish_halted(bdf, boot0, chip_id, "verify", stages, pipeline_start);
    }

    let t = Instant::now();
    match verify(bar0) {
        Ok(detail) => {
            stages.push(StageResult {
                name: "verify".into(),
                status: StageStatus::Ok,
                detail: Some(detail),
                duration_ms: t.elapsed().as_millis() as u64,
            });
        }
        Err(e) => {
            stages.push(StageResult {
                name: "verify".into(),
                status: StageStatus::Failed,
                detail: Some(e),
                duration_ms: t.elapsed().as_millis() as u64,
            });
            return finish(bdf, boot0, chip_id, stages, training_log, pipeline_start, warm_detected);
        }
    }

    // All stages passed
    let hbm2_writes = training_log.as_ref().map(|l| l.write_count());
    SovereignInitResult {
        bdf: bdf.to_string(),
        chip_id,
        boot0,
        all_ok: true,
        compute_ready: true,
        halted_at: None,
        stages,
        total_ms: pipeline_start.elapsed().as_millis() as u64,
        hbm2_writes,
        warm_detected,
    }
}

// ── Stage implementations ───────────────────────────────────────────────

fn bar0_probe(bar0: &MappedBar) -> Result<(u32, u32), String> {
    let result = bar0.isolated_read_u32(PMC_BOOT_0 as u32, ISOLATE_TIMEOUT);
    let boot0 = match result {
        super::isolation::IsolationResult::Ok(v) => v,
        super::isolation::IsolationResult::Timeout => {
            return Err("BAR0 probe timed out — GPU unreachable".into());
        }
        super::isolation::IsolationResult::ChildFailed { status } => {
            return Err(format!("BAR0 probe child failed (status={status})"));
        }
        super::isolation::IsolationResult::ForkError(e) => {
            return Err(format!("BAR0 probe fork error: {e}"));
        }
    };

    if boot0 == 0 || boot0 == 0xFFFF_FFFF {
        return Err(format!("BAR0 returned {boot0:#010x} — device not responding"));
    }

    let chip_id = (boot0 >> 20) & 0x1FF;
    tracing::info!(boot0 = format!("0x{boot0:08x}"), chip_id = format!("0x{chip_id:03x}"), "BAR0 probe OK");
    Ok((boot0, chip_id))
}

fn pmc_enable(bar0: &MappedBar) -> Result<String, String> {
    let before = bar0.read_u32(PMC_ENABLE).unwrap_or(0xDEAD_DEAD);
    tracing::debug!(pmc_before = format!("0x{before:08x}"), "PMC_ENABLE before");

    let _ = bar0.write_u32(PMC_ENABLE, 0xFFFF_FFFF);
    std::thread::sleep(Duration::from_millis(50));

    let after = bar0.read_u32(PMC_ENABLE).unwrap_or(0xDEAD_DEAD);
    tracing::debug!(pmc_after = format!("0x{after:08x}"), "PMC_ENABLE after");

    if after == 0 || after == 0xDEAD_DEAD {
        return Err(format!("PMC_ENABLE stuck at 0x{after:08x} after write"));
    }

    // Enable interrupts
    let _ = bar0.write_u32(PMC_INTR_EN_0, 0xFFFF_FFFF);

    Ok(format!("before=0x{before:08x} after=0x{after:08x}"))
}

fn run_hbm2_training(
    bar0: &MappedBar,
    bdf: &str,
    fbpa_count: usize,
    opts: &SovereignInitOptions,
) -> Result<TrainingLog, String> {
    let mut ctrl = Hbm2Controller::new(bar0, Some(bdf), fbpa_count);

    if let Some(golden) = &opts.golden_state {
        ctrl = ctrl.with_backend(hbm2_training::TrainingBackend::DifferentialReplay {
            golden_state: golden.clone(),
        });
    } else if let Some(rom) = &opts.vbios_rom {
        ctrl = ctrl.with_backend(hbm2_training::TrainingBackend::VbiosInterpreter {
            rom: rom.clone(),
        });
    }

    let phy = ctrl.enable_phy().map_err(|e| format!("enable_phy: {e}"))?;
    let linked = phy.train_links().map_err(|e| format!("train_links: {e}"))?;
    let dram = linked.init_dram().map_err(|e| format!("init_dram: {e}"))?;

    match dram.verify_vram() {
        Ok(verified) => {
            let log = verified.training_log().clone();
            tracing::info!(writes = log.write_count(), "HBM2 training complete — VRAM verified");
            Ok(log)
        }
        Err(e) => Err(format!("verify_vram: {e}")),
    }
}

fn is_kepler(sm: u32) -> bool {
    (35..=37).contains(&sm)
}

fn kepler_falcon_boot(bar0: &MappedBar) -> Result<String, String> {
    use crate::nv::vfio_compute::fecs_boot::{falcon_upload_dmem, falcon_upload_imem};
    use crate::vfio::channel::registers::falcon;

    // Kepler PGRAPH clock gating: write 0x260=1 to enable register access
    // to PGRAPH subsystem (FECS/GPCCS/GPC). This mirrors nouveau's
    // pmc_unk260() call before falcon loading.
    let _ = bar0.write_u32(0x260, 1);
    std::thread::sleep(Duration::from_millis(10));

    // Also apply GR engine firmware init writes from sw_nonctx.bin etc.
    // to configure the PGRAPH register space for Kepler.
    crate::nv::vfio_compute::NvVfioComputeDevice::apply_gr_bar0_init(bar0, 35);

    let fw_dir = "/lib/firmware/nvidia/gk210";
    let load = |name: &str| -> Result<Vec<u8>, String> {
        let path = format!("{fw_dir}/{name}");
        std::fs::read(&path).map_err(|e| format!("{path}: {e}"))
    };

    let gpccs_inst = load("gpccs_inst.bin")?;
    let gpccs_data = load("gpccs_data.bin")?;
    let fecs_inst = load("fecs_inst.bin")?;
    let fecs_data = load("fecs_data.bin")?;

    tracing::info!(
        fecs_inst = fecs_inst.len(),
        fecs_data = fecs_data.len(),
        gpccs_inst = gpccs_inst.len(),
        gpccs_data = gpccs_data.len(),
        "Kepler falcon boot: firmware loaded from {fw_dir}"
    );

    let boot_falcon = |name: &'static str, base: usize, inst: &[u8], data: &[u8]| -> Result<(u32, u32), String> {
        let cpuctl = bar0.read_u32(base + falcon::CPUCTL).unwrap_or(0xDEAD);
        tracing::info!(
            name,
            cpuctl = format!("{cpuctl:#010x}"),
            "Kepler {name}: starting PIO upload"
        );

        let _ = bar0.write_u32(base + falcon::CPUCTL, falcon::CPUCTL_HRESET);
        std::thread::sleep(Duration::from_millis(5));

        falcon_upload_dmem(bar0, base, 0, data);
        falcon_upload_imem(bar0, base, 0, inst, false);

        let _ = bar0.write_u32(base + falcon::BOOTVEC, 0);
        let _ = bar0.write_u32(base + falcon::MAILBOX0, 0);
        let _ = bar0.write_u32(base + falcon::MAILBOX1, 0);
        let _ = bar0.write_u32(base + falcon::CPUCTL, falcon::CPUCTL_IINVAL);
        std::thread::sleep(Duration::from_millis(1));
        let _ = bar0.write_u32(base + falcon::CPUCTL, falcon::CPUCTL_STARTCPU);

        let start = Instant::now();
        let timeout = Duration::from_secs(2);
        loop {
            std::thread::sleep(Duration::from_millis(5));
            let ctl = bar0.read_u32(base + falcon::CPUCTL).unwrap_or(0xDEAD);
            let mb0 = bar0.read_u32(base + falcon::MAILBOX0).unwrap_or(0);

            if mb0 != 0 {
                tracing::info!(name, cpuctl = format!("{ctl:#010x}"), mb0 = format!("{mb0:#010x}"), "mailbox response");
                return Ok((ctl, mb0));
            }
            if ctl & falcon::CPUCTL_HALTED != 0 && ctl & falcon::CPUCTL_HRESET == 0 {
                tracing::warn!(name, cpuctl = format!("{ctl:#010x}"), "halted without mailbox");
                return Ok((ctl, 0));
            }
            if start.elapsed() > timeout {
                tracing::error!(name, cpuctl = format!("{ctl:#010x}"), "timeout");
                return Err(format!("{name}: boot timeout (cpuctl={ctl:#010x})"));
            }
        }
    };

    let (gpccs_ctl, gpccs_mb0) = boot_falcon("GPCCS", falcon::GPCCS_BASE, &gpccs_inst, &gpccs_data)?;
    let (fecs_ctl, fecs_mb0) = boot_falcon("FECS", falcon::FECS_BASE, &fecs_inst, &fecs_data)?;

    let fecs_running = fecs_ctl & falcon::CPUCTL_HALTED == 0 && fecs_mb0 != 0;

    let detail = format!(
        "Kepler PIO: FECS cpuctl={fecs_ctl:#010x} mb0={fecs_mb0:#010x} | \
         GPCCS cpuctl={gpccs_ctl:#010x} mb0={gpccs_mb0:#010x} | running={fecs_running}"
    );

    if fecs_running {
        Ok(detail)
    } else {
        Err(format!("Kepler falcon not running: {detail}"))
    }
}

fn falcon_boot(
    bar0: &MappedBar,
    sm_version: u32,
    dma: Option<&crate::vfio::device::DmaBackend>,
) -> Result<String, String> {
    use crate::vfio::channel::registers::falcon;

    if is_kepler(sm_version) {
        tracing::info!(sm = sm_version, "Kepler GPU detected — using direct PIO falcon boot (no ACR)");
        return kepler_falcon_boot(bar0);
    }

    let chip = crate::nv::identity::chip_name(sm_version);

    // Apply GR BAR0 init writes first (engine enable, nonctx, dynamic).
    crate::nv::vfio_compute::NvVfioComputeDevice::apply_gr_bar0_init(bar0, sm_version);

    // Exp 173 proved nvidia-535 closed does NOT configure WPR on GV100 (pre-GSP).
    // WPR is a Turing+/Ampere+ concept for GSP-RM protection. On Volta, the RM
    // runs on the CPU and doesn't use WPR hardware boundaries. The ACR chain's
    // requirement for WPR cannot be satisfied on GV100 through vendor drivers.
    // The SEC2→HS→FECS bootstrap path requires a different approach for Volta.

    let wpr1_beg = bar0.read_u32(0x100CE4).unwrap_or(0xDEAD);
    let wpr1_end = bar0.read_u32(0x100CE8).unwrap_or(0xDEAD);
    let wpr2_beg = bar0.read_u32(0x100CEC).unwrap_or(0xDEAD);
    let wpr2_end = bar0.read_u32(0x100CF0).unwrap_or(0xDEAD);
    let wpr_configured = wpr2_beg != 0 && wpr2_end != 0 && wpr2_end > wpr2_beg;
    tracing::info!(
        wpr1_beg = format!("{wpr1_beg:#x}"),
        wpr1_end = format!("{wpr1_end:#x}"),
        wpr2_beg = format!("{wpr2_beg:#x}"),
        wpr2_end = format!("{wpr2_end:#x}"),
        wpr_configured,
        "pre-ACR WPR state"
    );

    tracing::info!(chip, dma_available = dma.is_some(), "falcon boot: trying ACR boot solver...");

    let acr_detail = match crate::nv::vfio_compute::acr_boot::FalconBootSolver::boot_for_generation(
        bar0,
        sm_version,
        chip,
        dma.cloned(),
        None,
    ) {
        Ok(results) => {
            if results.iter().any(|r| r.success) {
                let cpuctl = bar0
                    .read_u32(falcon::FECS_BASE + falcon::CPUCTL)
                    .unwrap_or(0xDEAD_DEAD);
                let mb0 = bar0
                    .read_u32(falcon::FECS_BASE + falcon::MAILBOX0)
                    .unwrap_or(0);
                return Ok(format!(
                    "ACR boot OK: FECS cpuctl=0x{cpuctl:08x} mb0=0x{mb0:08x} ({} strategies)",
                    results.len()
                ));
            }
            let summary: Vec<String> = results
                .iter()
                .enumerate()
                .map(|(i, r)| {
                    let tail: Vec<&str> = r.notes.iter().rev().take(40).map(|s| s.as_str()).collect();
                    format!("S{i}:{} [{}]", r.strategy, tail.join("; "))
                })
                .collect();
            summary.join(" | ")
        }
        Err(e) => format!("solver_err:{e}"),
    };

    tracing::info!(chip, "ACR failed, trying direct PIO upload...");
    match crate::nv::vfio_compute::fecs_boot::boot_gr_falcons(bar0, chip) {
        Ok(result) => {
            let detail = format!(
                "direct boot: FECS cpuctl=0x{:08x} mb0=0x{:08x} running={} | acr:[{}]",
                result.cpuctl_after, result.mailbox0, result.running, acr_detail,
            );
            if result.running {
                Ok(detail)
            } else {
                Err(format!("falcon not running: {detail}"))
            }
        }
        Err(e) => Err(format!("all boot paths failed: {e} | acr:[{acr_detail}]")),
    }
}

fn gr_init(bar0: &MappedBar, sm_version: u32) -> Result<String, String> {
    // apply_gr_bar0_init is idempotent — safe to call again, but we already
    // called it in falcon_boot. The FECS channel init is what matters here.
    let chip = crate::nv::identity::chip_name(sm_version);

    match crate::nv::vfio_compute::fecs_boot::boot_fecs(bar0, chip) {
        Ok(result) if result.running => {
            Ok(format!(
                "GR ready: FECS mb0=0x{:08x} mb1=0x{:08x}",
                result.mailbox0, result.mailbox1,
            ))
        }
        Ok(result) => Err(format!(
            "GR FECS not running: cpuctl=0x{:08x}",
            result.cpuctl_after,
        )),
        Err(e) => Err(format!("GR init: {e}")),
    }
}

fn verify(bar0: &MappedBar) -> Result<String, String> {
    // PTIMER liveness: both low and high timer registers should be non-zero
    // on a running GPU.
    let ops = vec![
        (PTIMER_TIME_0 as u32, None),
        (PTIMER_TIME_1 as u32, None),
        (PMC_ENABLE as u32, None),
    ];

    let result = bar0.isolated_batch(&ops, ISOLATE_TIMEOUT);
    match result {
        super::isolation::IsolationResult::Ok(vals) => {
            let timer_lo = vals.first().copied().unwrap_or(0);
            let timer_hi = vals.get(1).copied().unwrap_or(0);
            let pmc = vals.get(2).copied().unwrap_or(0);

            if timer_lo == 0 && timer_hi == 0 {
                return Err(format!(
                    "PTIMER dead (lo=0 hi=0), PMC=0x{pmc:08x}"
                ));
            }

            // VRAM sentinel via PRAMIN
            let vram_ok = pramin_sentinel_test(bar0);

            let detail = format!(
                "ptimer=0x{timer_hi:08x}_{timer_lo:08x} pmc=0x{pmc:08x} vram={}",
                if vram_ok { "ok" } else { "FAILED" },
            );

            if vram_ok {
                tracing::info!("sovereign verify: {detail}");
                Ok(detail)
            } else {
                tracing::warn!("sovereign verify: VRAM sentinel failed but PTIMER alive");
                Err(detail)
            }
        }
        super::isolation::IsolationResult::Timeout => {
            Err("verify timed out — GPU D-state".into())
        }
        super::isolation::IsolationResult::ChildFailed { status } => {
            Err(format!("verify child failed (status={status})"))
        }
        super::isolation::IsolationResult::ForkError(e) => {
            Err(format!("verify fork error: {e}"))
        }
    }
}

fn pramin_sentinel_test(bar0: &MappedBar) -> bool {
    use crate::vfio::memory::{MemoryRegion, PraminRegion};

    match PraminRegion::new(bar0, 0x0002_6000, 8) {
        Ok(mut region) => region.probe_sentinel(0, 0xCAFE_BEEF).is_working(),
        Err(_) => false,
    }
}

/// Map chip_id → SM version (approximate, for Volta/Kepler we care about).
fn chip_id_to_sm(chip_id: u32) -> u32 {
    match chip_id {
        0x140 => 70, // GV100 (Titan V)
        0x102 => 75, // TU102
        0x104 => 75, // TU104
        0x106 => 75, // TU106
        0x116 => 75, // TU116
        0x117 => 75, // TU117
        0x0E7 => 35, // GK110 (original Kepler mapping)
        0x0F0..=0x0FF => 35, // GK210 (Tesla K80) — Kepler, no ACR/WPR
        0x120..=0x12F => 50, // GM200 (Maxwell)
        _ => {
            tracing::warn!(chip_id = format!("0x{chip_id:03x}"), "unknown chip — assuming SM 70");
            70
        }
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────

fn finish(
    bdf: &str,
    boot0: u32,
    chip_id: u32,
    stages: Vec<StageResult>,
    training_log: Option<TrainingLog>,
    start: Instant,
    warm: bool,
) -> SovereignInitResult {
    SovereignInitResult {
        bdf: bdf.to_string(),
        chip_id,
        boot0,
        all_ok: false,
        compute_ready: false,
        halted_at: None,
        stages,
        total_ms: start.elapsed().as_millis() as u64,
        hbm2_writes: training_log.as_ref().map(|l| l.write_count()),
        warm_detected: warm,
    }
}

fn finish_halted(
    bdf: &str,
    boot0: u32,
    chip_id: u32,
    stage: &str,
    stages: Vec<StageResult>,
    start: Instant,
) -> SovereignInitResult {
    SovereignInitResult {
        bdf: bdf.to_string(),
        chip_id,
        boot0,
        all_ok: stages.iter().all(|s| s.status != StageStatus::Failed),
        compute_ready: false,
        halted_at: Some(stage.to_string()),
        stages,
        total_ms: start.elapsed().as_millis() as u64,
        hbm2_writes: None,
        warm_detected: false,
    }
}

/// Heuristic to detect if the GPU has already been trained.
///
/// A "warm" GPU has most engines enabled (high popcount in PMC_ENABLE)
/// and accessible VRAM (PRAMIN sentinel test passes). A cold GPU after
/// PCI reset typically has PMC_ENABLE = 0x0 or very few bits set.
fn is_warm_gpu(pmc_enable: u32, bar0: &MappedBar) -> bool {
    let popcount = pmc_enable.count_ones();
    if popcount < 8 {
        return false;
    }
    pramin_sentinel_test(bar0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chip_id_to_sm_covers_titan_v() {
        assert_eq!(chip_id_to_sm(0x140), 70);
    }

    #[test]
    fn chip_id_to_sm_covers_k80() {
        assert_eq!(chip_id_to_sm(0x0E7), 35);
    }

    #[test]
    fn chip_id_to_sm_unknown_defaults_to_70() {
        assert_eq!(chip_id_to_sm(0xFFF), 70);
    }

    #[test]
    fn stage_status_serde_roundtrip() {
        let json = serde_json::to_string(&StageStatus::Ok).unwrap();
        assert_eq!(json, "\"ok\"");
        let back: StageStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(back, StageStatus::Ok);
    }

    #[test]
    fn sovereign_init_result_display_halted() {
        let r = SovereignInitResult {
            bdf: "0000:03:00.0".into(),
            chip_id: 0x140,
            boot0: 0x140000A1,
            all_ok: true,
            compute_ready: false,
            halted_at: Some("hbm2_training".into()),
            stages: vec![],
            total_ms: 42,
            hbm2_writes: None,
            warm_detected: false,
        };
        let s = r.to_string();
        assert!(s.contains("HALTED@hbm2_training"));
        assert!(s.contains("42ms"));
    }

    #[test]
    fn sovereign_init_result_display_ready() {
        let r = SovereignInitResult {
            bdf: "0000:03:00.0".into(),
            chip_id: 0x140,
            boot0: 0x140000A1,
            all_ok: true,
            compute_ready: true,
            halted_at: None,
            stages: vec![
                StageResult {
                    name: "bar0_probe".into(),
                    status: StageStatus::Ok,
                    detail: None,
                    duration_ms: 1,
                },
            ],
            total_ms: 100,
            hbm2_writes: Some(42),
            warm_detected: true,
        };
        let s = r.to_string();
        assert!(s.contains("COMPUTE_READY"));
        assert!(s.contains("0x140"));
    }

    #[test]
    fn halt_before_serde_roundtrip() {
        let json = serde_json::to_string(&HaltBefore::Hbm2Training).unwrap();
        assert_eq!(json, "\"hbm2_training\"");
        let back: HaltBefore = serde_json::from_str(&json).unwrap();
        assert_eq!(back, HaltBefore::Hbm2Training);
    }

    #[test]
    fn options_default_has_no_halt() {
        let opts = SovereignInitOptions::default();
        assert!(opts.halt_before.is_none());
        assert!(opts.golden_state.is_none());
        assert!(opts.vbios_rom.is_none());
        assert!(!opts.skip_gr_init);
    }
}
