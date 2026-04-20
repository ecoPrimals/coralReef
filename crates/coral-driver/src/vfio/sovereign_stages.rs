// SPDX-License-Identifier: AGPL-3.0-or-later
//! Per-stage implementations for [`crate::vfio::sovereign_init::sovereign_init`].

use std::time::{Duration, Instant};

use crate::vfio::channel::hbm2_training::{self, Hbm2Controller};
use crate::vfio::device::MappedBar;
use crate::vfio::sovereign_types::SovereignInitOptions;

pub(crate) const PMC_BOOT_0: usize = 0x0000_0000;
pub(crate) const PMC_ENABLE: usize = 0x0000_0200;
pub(crate) const PMC_INTR_EN_0: usize = 0x0000_0140;
pub(crate) const PTIMER_TIME_0: usize = 0x0000_9400;
pub(crate) const PTIMER_TIME_1: usize = 0x0000_9410;

pub(crate) const ISOLATE_TIMEOUT: Duration = Duration::from_secs(3);

pub(crate) fn bar0_probe(bar0: &MappedBar) -> Result<(u32, u32), String> {
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
        return Err(format!(
            "BAR0 returned {boot0:#010x} — device not responding"
        ));
    }

    let chip_id = (boot0 >> 20) & 0x1FF;
    tracing::info!(
        boot0 = format!("0x{boot0:08x}"),
        chip_id = format!("0x{chip_id:03x}"),
        "BAR0 probe OK"
    );
    Ok((boot0, chip_id))
}

pub(crate) fn pmc_enable(bar0: &MappedBar) -> Result<String, String> {
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

pub(crate) fn run_hbm2_training(
    bar0: &MappedBar,
    bdf: &str,
    fbpa_count: usize,
    opts: &SovereignInitOptions,
) -> Result<crate::vfio::channel::hbm2_training::TrainingLog, String> {
    let mut ctrl = Hbm2Controller::new(bar0, Some(bdf), fbpa_count);

    if let Some(golden) = &opts.golden_state {
        ctrl = ctrl.with_backend(hbm2_training::TrainingBackend::DifferentialReplay {
            golden_state: golden.clone(),
        });
    } else if let Some(rom) = &opts.vbios_rom {
        ctrl = ctrl
            .with_backend(hbm2_training::TrainingBackend::VbiosInterpreter { rom: rom.clone() });
    }

    let phy = ctrl.enable_phy().map_err(|e| format!("enable_phy: {e}"))?;
    let linked = phy.train_links().map_err(|e| format!("train_links: {e}"))?;
    let dram = linked.init_dram().map_err(|e| format!("init_dram: {e}"))?;

    match dram.verify_vram() {
        Ok(verified) => {
            let log = verified.training_log().clone();
            tracing::info!(
                writes = log.write_count(),
                "HBM2 training complete — VRAM verified"
            );
            Ok(log)
        }
        Err(e) => Err(format!("verify_vram: {e}")),
    }
}

pub(crate) fn is_kepler(sm: u32) -> bool {
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

    let boot_falcon =
        |name: &'static str, base: usize, inst: &[u8], data: &[u8]| -> Result<(u32, u32), String> {
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
                    tracing::info!(
                        name,
                        cpuctl = format!("{ctl:#010x}"),
                        mb0 = format!("{mb0:#010x}"),
                        "mailbox response"
                    );
                    return Ok((ctl, mb0));
                }
                if ctl & falcon::CPUCTL_HALTED != 0 && ctl & falcon::CPUCTL_HRESET == 0 {
                    tracing::warn!(
                        name,
                        cpuctl = format!("{ctl:#010x}"),
                        "halted without mailbox"
                    );
                    return Ok((ctl, 0));
                }
                if start.elapsed() > timeout {
                    tracing::error!(name, cpuctl = format!("{ctl:#010x}"), "timeout");
                    return Err(format!("{name}: boot timeout (cpuctl={ctl:#010x})"));
                }
            }
        };

    let (gpccs_ctl, gpccs_mb0) =
        boot_falcon("GPCCS", falcon::GPCCS_BASE, &gpccs_inst, &gpccs_data)?;
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

pub(crate) fn falcon_boot(
    bar0: &MappedBar,
    sm_version: u32,
    dma: Option<&crate::vfio::device::DmaBackend>,
) -> Result<String, String> {
    use crate::vfio::channel::registers::falcon;

    if is_kepler(sm_version) {
        tracing::info!(
            sm = sm_version,
            "Kepler GPU detected — using direct PIO falcon boot (no ACR)"
        );
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

    tracing::info!(
        chip,
        dma_available = dma.is_some(),
        "falcon boot: trying ACR boot solver..."
    );

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
                    let tail: Vec<&str> =
                        r.notes.iter().rev().take(40).map(|s| s.as_str()).collect();
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

pub(crate) fn gr_init(bar0: &MappedBar, sm_version: u32) -> Result<String, String> {
    // apply_gr_bar0_init is idempotent — safe to call again, but we already
    // called it in falcon_boot. The FECS channel init is what matters here.
    let chip = crate::nv::identity::chip_name(sm_version);

    match crate::nv::vfio_compute::fecs_boot::boot_fecs(bar0, chip) {
        Ok(result) if result.running => Ok(format!(
            "GR ready: FECS mb0=0x{:08x} mb1=0x{:08x}",
            result.mailbox0, result.mailbox1,
        )),
        Ok(result) => Err(format!(
            "GR FECS not running: cpuctl=0x{:08x}",
            result.cpuctl_after,
        )),
        Err(e) => Err(format!("GR init: {e}")),
    }
}

pub(crate) fn verify(bar0: &MappedBar) -> Result<String, String> {
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
                return Err(format!("PTIMER dead (lo=0 hi=0), PMC=0x{pmc:08x}"));
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
        super::isolation::IsolationResult::Timeout => Err("verify timed out — GPU D-state".into()),
        super::isolation::IsolationResult::ChildFailed { status } => {
            Err(format!("verify child failed (status={status})"))
        }
        super::isolation::IsolationResult::ForkError(e) => Err(format!("verify fork error: {e}")),
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
pub(crate) fn chip_id_to_sm(chip_id: u32) -> u32 {
    match chip_id {
        0x140 => 70,         // GV100 (Titan V)
        0x102 => 75,         // TU102
        0x104 => 75,         // TU104
        0x106 => 75,         // TU106
        0x116 => 75,         // TU116
        0x117 => 75,         // TU117
        0x0E7 => 35,         // GK110 (original Kepler mapping)
        0x0F0..=0x0FF => 35, // GK210 (Tesla K80) — Kepler, no ACR/WPR
        0x120..=0x12F => 50, // GM200 (Maxwell)
        _ => {
            tracing::warn!(
                chip_id = format!("0x{chip_id:03x}"),
                "unknown chip — assuming SM 70"
            );
            70
        }
    }
}

/// Heuristic to detect if the GPU has already been trained.
///
/// A "warm" GPU has most engines enabled (high popcount in PMC_ENABLE)
/// and accessible VRAM (PRAMIN sentinel test passes). A cold GPU after
/// PCI reset typically has PMC_ENABLE = 0x0 or very few bits set.
pub(crate) fn is_warm_gpu(pmc_enable: u32, bar0: &MappedBar) -> bool {
    let popcount = pmc_enable.count_ones();
    if popcount < 8 {
        return false;
    }
    pramin_sentinel_test(bar0)
}
