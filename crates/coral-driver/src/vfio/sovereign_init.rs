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

use std::time::Instant;

use crate::vfio::channel::hbm2_training::TrainingLog;
use crate::vfio::device::MappedBar;
use crate::vfio::sovereign_stages::{
    PMC_ENABLE, bar0_probe, chip_id_to_sm, falcon_boot, gr_init, is_kepler, is_warm_gpu,
    pmc_enable, run_hbm2_training, verify,
};

pub use crate::vfio::sovereign_types::{
    HaltBefore, SovereignInitOptions, SovereignInitResult, StageResult, StageStatus,
};

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
    let warm_detected = is_kepler(sm) || is_warm_gpu(pmc_before, bar0);

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
                return finish(
                    bdf,
                    boot0,
                    chip_id,
                    stages,
                    training_log,
                    pipeline_start,
                    warm_detected,
                );
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
            return finish(
                bdf,
                boot0,
                chip_id,
                stages,
                training_log,
                pipeline_start,
                warm_detected,
            );
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
                return finish(
                    bdf,
                    boot0,
                    chip_id,
                    stages,
                    training_log,
                    pipeline_start,
                    warm_detected,
                );
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
            return finish(
                bdf,
                boot0,
                chip_id,
                stages,
                training_log,
                pipeline_start,
                warm_detected,
            );
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vfio::sovereign_stages::chip_id_to_sm;

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
            stages: vec![StageResult {
                name: "bar0_probe".into(),
                status: StageStatus::Ok,
                detail: None,
                duration_ms: 1,
            }],
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
