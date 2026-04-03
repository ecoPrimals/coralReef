// SPDX-License-Identifier: AGPL-3.0-only

//! Falcon boot solver — probes hardware and runs strategies in order.

use std::fmt;

use crate::error::DriverResult;
use crate::vfio::channel::registers::falcon;
use crate::vfio::device::{DmaBackend, MappedBar};

use super::boot_result::{AcrBootResult, BootJournal};
use super::fecs_method;
use super::firmware::AcrFirmwareSet;
use super::sec2_hal::Sec2Probe;
use super::strategy_chain::{
    attempt_acr_chain, attempt_direct_acr_load, attempt_pio_acr_with_sysmem_wpr,
    attempt_pio_acr_with_vram_wpr,
};
use super::strategy_hybrid::attempt_hybrid_acr_boot;
use super::strategy_mailbox::{
    FalconBootvecOffsets, attempt_acr_mailbox_command, attempt_direct_falcon_upload,
    attempt_direct_fecs_boot, attempt_direct_hreset, attempt_emem_boot, attempt_nouveau_boot,
    attempt_physical_first_boot,
};
use super::strategy_sysmem::attempt_sysmem_acr_boot;
use super::strategy_vram::{
    DualPhaseConfig, attempt_dual_phase_boot_cfg, attempt_vram_acr_boot,
};

// ── Falcon Boot Solver (top-level orchestrator) ──────────────────────

/// Classified FECS state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FecsState {
    /// FECS executing: mailbox non-zero and `HRESET` clear.
    Running,
    /// FECS firmware halted (`CPUCTL_HALTED` set).
    InHreset,
    /// FECS idle: not in `HRESET`, mailbox not indicating run.
    Halted,
    /// BAR read returned PRI error (power-gated or inaccessible).
    Inaccessible,
}

/// Probe all falcon states relevant to boot strategy selection.
#[derive(Debug)]
pub struct FalconProbe {
    /// FECS `CPUCTL` register snapshot.
    pub fecs_cpuctl: u32,
    /// FECS `MAILBOX0` register snapshot.
    pub fecs_mailbox0: u32,
    /// FECS `HWCFG` register snapshot.
    pub fecs_hwcfg: u32,
    /// FECS program counter (offset 0x030).
    pub fecs_pc: u32,
    /// FECS exception info register (offset 0x148).
    pub fecs_exci: u32,
    /// GPCCS `CPUCTL` register snapshot.
    pub gpccs_cpuctl: u32,
    /// GPCCS program counter (offset 0x030).
    pub gpccs_pc: u32,
    /// GPCCS exception info register (offset 0x148).
    pub gpccs_exci: u32,
    /// GPCCS `BOOTVEC` register (offset 0x104).
    pub gpccs_bootvec: u32,
    /// SEC2 falcon probe (same BAR0 window as boot strategy code).
    pub sec2: Sec2Probe,
    /// Classified FECS runtime state.
    pub fecs_state: FecsState,
}

impl FalconProbe {
    /// Samples FECS, GPCCS, and SEC2 falcon registers and classifies FECS state.
    pub fn capture(bar0: &MappedBar) -> Self {
        let fecs_r = |off: usize| {
            bar0.read_u32(falcon::FECS_BASE + off)
                .unwrap_or(0xDEAD_DEAD)
        };
        let gpccs_r = |off: usize| {
            bar0.read_u32(falcon::GPCCS_BASE + off)
                .unwrap_or(0xDEAD_DEAD)
        };
        let fecs_cpuctl = fecs_r(falcon::CPUCTL);
        let fecs_mailbox0 = fecs_r(falcon::MAILBOX0);
        let fecs_hwcfg = fecs_r(falcon::HWCFG);
        let fecs_pc = fecs_r(falcon::PC);
        let fecs_exci = fecs_r(falcon::EXCI);
        let gpccs_cpuctl = gpccs_r(falcon::CPUCTL);
        let gpccs_pc = gpccs_r(falcon::PC);
        let gpccs_exci = gpccs_r(falcon::EXCI);
        let gpccs_bootvec = gpccs_r(falcon::BOOTVEC);
        let sec2 = Sec2Probe::capture(bar0);

        let fecs_state = if crate::vfio::channel::registers::pri::is_pri_error(fecs_cpuctl) {
            FecsState::Inaccessible
        } else if fecs_mailbox0 != 0 && fecs_cpuctl & falcon::CPUCTL_HALTED == 0 {
            FecsState::Running
        } else if fecs_cpuctl & falcon::CPUCTL_HALTED != 0 {
            FecsState::InHreset
        } else {
            FecsState::Halted
        };

        Self {
            fecs_cpuctl,
            fecs_mailbox0,
            fecs_hwcfg,
            fecs_pc,
            fecs_exci,
            gpccs_cpuctl,
            gpccs_pc,
            gpccs_exci,
            gpccs_bootvec,
            sec2,
            fecs_state,
        }
    }
}

impl FalconProbe {
    /// Classify GPCCS execution state from cpuctl, PC, and EXCI.
    pub fn gpccs_state_label(&self) -> &'static str {
        if self.gpccs_cpuctl == 0xDEAD_DEAD {
            "UNREACHABLE"
        } else if self.gpccs_cpuctl & falcon::CPUCTL_HALTED != 0 {
            "HRESET"
        } else if self.gpccs_cpuctl & falcon::CPUCTL_STOPPED != 0 {
            "HALTED"
        } else if self.gpccs_exci != 0 {
            "FAULTED"
        } else if self.gpccs_pc == 0 {
            "STALLED (PC=0)"
        } else {
            "RUNNING"
        }
    }

    /// Classify FECS execution state from cpuctl, PC, and EXCI.
    pub fn fecs_state_label(&self) -> &'static str {
        if self.fecs_cpuctl == 0xDEAD_DEAD {
            "UNREACHABLE"
        } else if self.fecs_cpuctl & falcon::CPUCTL_HALTED != 0 {
            "HRESET"
        } else if self.fecs_cpuctl & falcon::CPUCTL_STOPPED != 0 {
            "HALTED"
        } else if self.fecs_exci != 0 {
            "FAULTED"
        } else if self.fecs_pc == 0 && self.fecs_mailbox0 == 0 {
            "STALLED (PC=0)"
        } else {
            "RUNNING"
        }
    }
}

impl fmt::Display for FalconProbe {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Falcon Probe:")?;
        writeln!(
            f,
            "  FECS: {} cpuctl={:#010x} pc={:#06x} exci={:#010x} mb0={:#010x} hwcfg={:#010x}",
            self.fecs_state_label(),
            self.fecs_cpuctl,
            self.fecs_pc,
            self.fecs_exci,
            self.fecs_mailbox0,
            self.fecs_hwcfg
        )?;
        writeln!(
            f,
            "  GPCCS: {} cpuctl={:#010x} pc={:#06x} exci={:#010x} bootvec={:#010x}",
            self.gpccs_state_label(),
            self.gpccs_cpuctl,
            self.gpccs_pc,
            self.gpccs_exci,
            self.gpccs_bootvec
        )?;
        write!(f, "  {}", self.sec2)
    }
}

/// Boot strategy selected by the solver.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootStrategy {
    /// FECS is already running — no boot needed.
    AlreadyRunning,
    /// Direct HRESET experiments (low cost, may not work).
    DirectHreset,
    /// SEC2 EMEM-based ACR boot (works on HS-locked falcon).
    EmemBoot,
    /// SEC2 IMEM-based ACR boot (works on clean-reset falcon).
    ImemBoot,
    /// All strategies exhausted.
    NoViablePath,
}

/// The Falcon Boot Solver — probes GPU state and selects the best
/// strategy for getting FECS running.
pub struct FalconBootSolver;

impl FalconBootSolver {
    /// Probe and attempt to boot FECS using the best available strategy.
    ///
    /// Strategy ordering prioritizes the most faithful Nouveau reproduction:
    ///   0. Already running (free)
    ///   1. Nouveau-style SEC2 boot (corrected reset + IMEM/EMEM + ALIAS_EN)
    ///   2. VRAM-based ACR boot (PRAMIN → VRAM → falcon DMA)
    ///   3. System-memory ACR boot (IOMMU DMA — matches Nouveau arch)
    ///   4. Direct FECS boot (bypass ACR — if FECS in HRESET)
    ///   5. ACR mailbox command (if SEC2 still has live Nouveau ACR)
    ///   6. Direct HRESET experiments
    ///   7. Direct ACR IMEM load (canary test + full ACR firmware)
    ///   8. Full ACR chain with DMA (legacy physical addressing)
    ///   9. EMEM-based boot fallback
    pub fn boot(
        bar0: &MappedBar,
        chip: &str,
        container: Option<DmaBackend>,
        journal: Option<&dyn BootJournal>,
    ) -> DriverResult<Vec<AcrBootResult>> {
        Self::boot_inner(bar0, chip, container, journal, false)
    }

    /// PIO-only boot: skip all DMA-based strategies to avoid hangs on GPUs
    /// whose PRIV ring or FBHUB is not fully initialized.
    pub fn boot_pio_only(
        bar0: &MappedBar,
        chip: &str,
        journal: Option<&dyn BootJournal>,
    ) -> DriverResult<Vec<AcrBootResult>> {
        Self::boot_inner(bar0, chip, None, journal, true)
    }

    fn boot_inner(
        bar0: &MappedBar,
        chip: &str,
        container: Option<DmaBackend>,
        journal: Option<&dyn BootJournal>,
        pio_only: bool,
    ) -> DriverResult<Vec<AcrBootResult>> {
        let mut results = Vec::new();
        let probe = FalconProbe::capture(bar0);
        tracing::info!("{probe}");
        if pio_only {
            tracing::info!("PIO-only mode: skipping all DMA-based strategies");
        }

        let record = |r: &AcrBootResult| {
            if let Some(j) = journal {
                j.record_boot_attempt(r);
            }
        };

        // Strategy 0: Already running
        if probe.fecs_state == FecsState::Running {
            tracing::info!("FECS already running — no boot needed");
            return Ok(results);
        }

        let fw = match AcrFirmwareSet::load(chip) {
            Ok(fw) => {
                tracing::info!("{}", fw.summary());
                fw
            }
            Err(e) => {
                tracing::error!("Failed to load firmware: {e}");
                return Ok(results);
            }
        };

        // ── Strategies 1–3d: DMA-based SEC2/ACR boot paths ──
        // Skipped entirely in PIO-only mode to avoid DMA hangs on GPUs
        // with uninitialized PRIV ring / FBHUB.
        if pio_only {
            tracing::info!("PIO-only: skipping strategies 1–3d (DMA-based)");
        } else {
            // ── Strategy 1: Nouveau-style SEC2 boot ──
            tracing::info!("Strategy 1: Nouveau-style SEC2 boot (corrected sequence)...");
            let nouveau_result = attempt_nouveau_boot(bar0, &fw);
            tracing::info!("{nouveau_result}");
            record(&nouveau_result);
            let nouveau_success = nouveau_result.success;
            results.push(nouveau_result);
            if nouveau_success {
                return Ok(results);
            }

            // ── Strategy 1b: Physical-first SEC2 boot ──
            tracing::info!("Strategy 1b: Physical-first SEC2 boot (no instance block)...");
            let physical_result = attempt_physical_first_boot(bar0, &fw);
            tracing::info!("{physical_result}");
            record(&physical_result);
            let physical_success = physical_result.success;
            results.push(physical_result);
            if physical_success {
                return Ok(results);
            }

            // ── Strategy 2: VRAM-based ACR boot ──
            tracing::info!("Strategy 2: VRAM-based ACR boot (PRAMIN→VRAM→falcon DMA)...");
            let vram_result = attempt_vram_acr_boot(bar0, &fw);
            tracing::info!("{vram_result}");
            record(&vram_result);
            let vram_success = vram_result.success;
            results.push(vram_result);
            if vram_success {
                return Ok(results);
            }

            // ── Strategy 3: System-memory ACR boot ──
            if let Some(ref dma_backend) = container {
                tracing::info!("Strategy 3: System-memory ACR boot (IOMMU DMA)...");
                let sysmem_result = attempt_sysmem_acr_boot(bar0, &fw, dma_backend.clone());
                tracing::info!("{sysmem_result}");
                record(&sysmem_result);
                let sysmem_success = sysmem_result.success;
                results.push(sysmem_result);
                if sysmem_success {
                    return Ok(results);
                }
            } else {
                tracing::info!("No DMA backend — skipping system-memory ACR boot");
            }

            // ── Strategy 3b: Hybrid ACR boot (VRAM pages + sysmem data) ──
            if let Some(ref dma_backend) = container {
                tracing::info!("Strategy 3b: Hybrid ACR boot (VRAM pages + sysmem data)...");
                let hybrid_result = attempt_hybrid_acr_boot(bar0, &fw, dma_backend.clone());
                tracing::info!("{hybrid_result}");
                record(&hybrid_result);
                let hybrid_success = hybrid_result.success;
                results.push(hybrid_result);
                if hybrid_success {
                    return Ok(results);
                }
            }

            // ── Strategy 3c: Dual-phase VRAM ACR boot (default WPR2 off) ──
            tracing::info!("Strategy 3c: Dual-phase VRAM ACR boot (legacy PDEs)...");
            let dp_default = attempt_dual_phase_boot_cfg(bar0, &fw, &DualPhaseConfig::default());
            tracing::info!("{dp_default}");
            record(&dp_default);
            let dp_default_success = dp_default.success;
            results.push(dp_default);
            if dp_default_success {
                return Ok(results);
            }

            // ── Strategy 3d: Dual-phase with WPR2 hardware boundaries set ──
            tracing::info!("Strategy 3d: Dual-phase VRAM ACR boot + WPR2 set...");
            let dp_wpr2 = attempt_dual_phase_boot_cfg(
                bar0,
                &fw,
                &DualPhaseConfig {
                    set_wpr2: true,
                    ..DualPhaseConfig::default()
                },
            );
            tracing::info!("{dp_wpr2}");
            record(&dp_wpr2);
            let dp_wpr2_success = dp_wpr2.success;
            results.push(dp_wpr2);
            if dp_wpr2_success {
                return Ok(results);
            }
        }

        // ── Strategy 4: Direct FECS boot (bypass ACR) ──
        if probe.fecs_state == FecsState::InHreset {
            tracing::info!("Strategy 4: Direct FECS boot (bypass ACR)...");
            let fecs_result = attempt_direct_fecs_boot(bar0, &fw);
            tracing::info!("{fecs_result}");
            record(&fecs_result);
            let fecs_success = fecs_result.success;
            results.push(fecs_result);
            if fecs_success {
                return Ok(results);
            }
        }

        // ── Strategies 5, 7–9: SEC2 mailbox / DMA-heavy paths ──
        if pio_only {
            tracing::info!("PIO-only: skipping strategies 5, 7–9 (DMA / mailbox)");
        } else {
            // ── Strategy 5: ACR mailbox command ──
            tracing::info!("Strategy 5: ACR mailbox command (live SEC2)...");
            let bootvec = FalconBootvecOffsets {
                gpccs: fw.gpccs_bl.bl_imem_off(),
                fecs: fw.fecs_bl.bl_imem_off(),
            };
            let mailbox_result = attempt_acr_mailbox_command(bar0, &bootvec);
            tracing::info!("{mailbox_result}");
            record(&mailbox_result);
            let mailbox_success = mailbox_result.success;
            results.push(mailbox_result);
            if mailbox_success {
                return Ok(results);
            }

            // Check if FECS is running even without the ready signal.
            let post5_probe = FalconProbe::capture(bar0);
            if post5_probe.fecs_cpuctl & falcon::CPUCTL_HALTED == 0
                && post5_probe.fecs_cpuctl != 0xDEAD_DEAD
            {
                tracing::info!(
                    pc = format!("{:#06x}", post5_probe.fecs_pc),
                    cpuctl = format!("{:#010x}", post5_probe.fecs_cpuctl),
                    "FECS is RUNNING after Strategy 5 — probing method interface"
                );
                let method_probe = fecs_method::fecs_probe_methods(bar0);
                tracing::info!("{method_probe}");

                if method_probe.ctx_size.is_ok() {
                    tracing::info!("*** FECS METHOD INTERFACE ALIVE — GR ENGINE ACCESSIBLE ***");
                    return Ok(results);
                }
            }
        }

        // ── Strategy 6: Direct HRESET experiments ── (PIO-safe)
        tracing::info!("Strategy 6: Direct HRESET experiments...");
        let direct_result = attempt_direct_hreset(bar0);
        tracing::info!("{direct_result}");
        record(&direct_result);
        let direct_success = direct_result.success;
        results.push(direct_result);
        if direct_success {
            return Ok(results);
        }

        // ── Strategy 7c: PIO ACR + system memory WPR (DMA-backed WPR in host RAM) ──
        // Runs BEFORE 7b to avoid state pollution from 7b's falcon_prepare_physical_dma.
        if let Some(ref dma_backend) = container {
            tracing::info!(
                "Strategy 7c: PIO ACR + sysmem WPR (DMA buffer, unmodified ACR code)..."
            );
            let sysmem_wpr_result =
                attempt_pio_acr_with_sysmem_wpr(bar0, &fw, dma_backend.clone());
            tracing::info!("{sysmem_wpr_result}");
            record(&sysmem_wpr_result);
            let sysmem_wpr_success = sysmem_wpr_result.success;
            results.push(sysmem_wpr_result);
            if sysmem_wpr_success {
                return Ok(results);
            }
        } else {
            tracing::info!("Strategy 7c: skipped (no DMA backend)");
        }

        // ── Strategy 7b: PIO ACR + VRAM WPR (safe — all PIO, pre-populated WPR) ──
        // Runs in both normal and PIO-only mode since it avoids BL DMA entirely.
        tracing::info!("Strategy 7b: PIO ACR + VRAM WPR (no BL DMA, pre-populated WPR)...");
        let pio_wpr_result = attempt_pio_acr_with_vram_wpr(bar0, &fw);
        tracing::info!("{pio_wpr_result}");
        record(&pio_wpr_result);
        let pio_wpr_success = pio_wpr_result.success;
        results.push(pio_wpr_result);
        if pio_wpr_success {
            return Ok(results);
        }

        if pio_only {
            tracing::info!("PIO-only: skipping strategies 7–9 (DMA / EMEM)");
        } else {
            // ── Strategy 7: Direct ACR IMEM load ──
            tracing::info!("Strategy 7: Direct ACR IMEM load (canary + firmware)...");
            let direct_acr_result = attempt_direct_acr_load(bar0, &fw);
            tracing::info!("{direct_acr_result}");
            record(&direct_acr_result);
            let direct_acr_success = direct_acr_result.success;
            results.push(direct_acr_result);
            if direct_acr_success {
                return Ok(results);
            }

            // ── Strategy 8: Full ACR chain with DMA ──
            if let Some(dma_backend) = container {
                tracing::info!("Strategy 8: Full ACR chain boot (DMA-backed, legacy)...");
                let chain_result = attempt_acr_chain(bar0, &fw, dma_backend);
                tracing::info!("{chain_result}");
                record(&chain_result);
                let chain_success = chain_result.success;
                results.push(chain_result);
                if chain_success {
                    return Ok(results);
                }
            } else {
                tracing::info!("No DMA backend — skipping ACR chain");
            }

            // ── Strategy 9: EMEM-based boot fallback ──
            tracing::info!("Strategy 9: EMEM-based SEC2 boot (fallback)...");
            let emem_result = attempt_emem_boot(bar0, &fw);
            tracing::info!("{emem_result}");
            record(&emem_result);
            results.push(emem_result);
        }

        // ── Strategy 10: Direct host IMEM/DMEM upload (bypass ACR DMA) ── (PIO-safe)
        tracing::info!("Strategy 10: Direct host GPCCS/FECS firmware upload (Exp 091b)...");
        let direct_upload_result = attempt_direct_falcon_upload(bar0, &fw);
        tracing::info!("{direct_upload_result}");
        record(&direct_upload_result);
        let direct_upload_success = direct_upload_result.success;
        results.push(direct_upload_result);
        if direct_upload_success {
            tracing::info!("*** DIRECT FALCON UPLOAD SUCCEEDED ***");
            return Ok(results);
        }

        Ok(results)
    }
}
