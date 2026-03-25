// SPDX-License-Identifier: AGPL-3.0-only

//! Shared [`AcrBootResult`] type and helpers for strategy modules.

use std::fmt;

use crate::vfio::channel::registers::falcon;
use crate::vfio::device::MappedBar;

use super::sec2_hal::Sec2Probe;

// ── Boot attempts ────────────────────────────────────────────────────

/// Result of a boot chain attempt.
#[derive(Debug)]
pub struct AcrBootResult {
    /// Name of the boot strategy (EMEM, IMEM, mailbox, etc.).
    pub strategy: &'static str,
    /// SEC2 probe before the attempt.
    pub sec2_before: Sec2Probe,
    /// SEC2 probe after the attempt.
    pub sec2_after: Sec2Probe,
    /// FECS `CPUCTL` after the attempt.
    pub fecs_cpuctl_after: u32,
    /// FECS `MAILBOX0` after the attempt.
    pub fecs_mailbox0_after: u32,
    /// GPCCS `CPUCTL` after the attempt.
    pub gpccs_cpuctl_after: u32,
    /// FECS program counter after the attempt.
    pub fecs_pc_after: u32,
    /// FECS exception info after the attempt.
    pub fecs_exci_after: u32,
    /// GPCCS program counter after the attempt.
    pub gpccs_pc_after: u32,
    /// GPCCS exception info after the attempt.
    pub gpccs_exci_after: u32,
    /// Whether the chain reported success (FECS ready / expected mailbox).
    pub success: bool,
    /// Human-readable diagnostic lines from the attempt.
    pub notes: Vec<String>,
}

impl fmt::Display for AcrBootResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "╔══ ACR Boot: {} ═══════════════════════════════════╗",
            self.strategy
        )?;
        writeln!(f, "  success: {}", self.success)?;
        writeln!(f, "  SEC2 before: {}", self.sec2_before)?;
        writeln!(f, "  SEC2 after:  {}", self.sec2_after)?;
        writeln!(
            f,
            "  FECS after:  cpuctl={:#010x} pc={:#06x} exci={:#010x} mb0={:#010x}",
            self.fecs_cpuctl_after, self.fecs_pc_after, self.fecs_exci_after, self.fecs_mailbox0_after
        )?;
        writeln!(
            f,
            "  GPCCS after: cpuctl={:#010x} pc={:#06x} exci={:#010x}",
            self.gpccs_cpuctl_after, self.gpccs_pc_after, self.gpccs_exci_after
        )?;
        for note in &self.notes {
            writeln!(f, "  note: {note}")?;
        }
        write!(
            f,
            "╚═══════════════════════════════════════════════════════╝"
        )
    }
}

/// Evaluate whether falcons are genuinely running (not just cpuctl==0).
///
/// Requires: FECS not in HRESET, GPCCS EXCI == 0, and GPCCS PC != 0.
pub(crate) fn evaluate_boot_success(
    fecs_cpuctl: u32,
    gpccs_pc: u32,
    gpccs_exci: u32,
) -> bool {
    let fecs_not_reset = fecs_cpuctl & falcon::CPUCTL_HRESET == 0;
    let gpccs_healthy = gpccs_exci == 0 && gpccs_pc != 0;
    fecs_not_reset && gpccs_healthy
}

/// Capture FECS/GPCCS PC+EXCI for post-boot result population.
pub(crate) struct PostBootCapture {
    pub fecs_cpuctl: u32,
    pub fecs_mailbox0: u32,
    pub fecs_pc: u32,
    pub fecs_exci: u32,
    pub gpccs_cpuctl: u32,
    pub gpccs_pc: u32,
    pub gpccs_exci: u32,
}

impl PostBootCapture {
    pub fn capture(bar0: &MappedBar) -> Self {
        let fecs_r = |off: usize| bar0.read_u32(falcon::FECS_BASE + off).unwrap_or(0xDEAD);
        let gpccs_r = |off: usize| bar0.read_u32(falcon::GPCCS_BASE + off).unwrap_or(0xDEAD);
        Self {
            fecs_cpuctl: fecs_r(falcon::CPUCTL),
            fecs_mailbox0: fecs_r(falcon::MAILBOX0),
            fecs_pc: fecs_r(falcon::PC),
            fecs_exci: fecs_r(falcon::EXCI),
            gpccs_cpuctl: gpccs_r(falcon::CPUCTL),
            gpccs_pc: gpccs_r(falcon::PC),
            gpccs_exci: gpccs_r(falcon::EXCI),
        }
    }

    pub fn success(&self) -> bool {
        evaluate_boot_success(self.fecs_cpuctl, self.gpccs_pc, self.gpccs_exci)
    }

    pub fn into_result(
        self,
        strategy: &'static str,
        sec2_before: Sec2Probe,
        sec2_after: Sec2Probe,
        notes: Vec<String>,
    ) -> AcrBootResult {
        let success = self.success();
        AcrBootResult {
            strategy,
            sec2_before,
            sec2_after,
            fecs_cpuctl_after: self.fecs_cpuctl,
            fecs_mailbox0_after: self.fecs_mailbox0,
            gpccs_cpuctl_after: self.gpccs_cpuctl,
            fecs_pc_after: self.fecs_pc,
            fecs_exci_after: self.fecs_exci,
            gpccs_pc_after: self.gpccs_pc,
            gpccs_exci_after: self.gpccs_exci,
            success,
            notes,
        }
    }
}

pub(crate) fn make_fail_result(
    strategy: &'static str,
    sec2_before: Sec2Probe,
    bar0: &MappedBar,
    notes: Vec<String>,
) -> AcrBootResult {
    let sec2_after = Sec2Probe::capture(bar0);
    let fecs_r = |off: usize| bar0.read_u32(falcon::FECS_BASE + off).unwrap_or(0xDEAD);
    let gpccs_r = |off: usize| bar0.read_u32(falcon::GPCCS_BASE + off).unwrap_or(0xDEAD);
    AcrBootResult {
        strategy,
        sec2_before,
        sec2_after,
        fecs_cpuctl_after: fecs_r(falcon::CPUCTL),
        fecs_mailbox0_after: fecs_r(falcon::MAILBOX0),
        gpccs_cpuctl_after: gpccs_r(falcon::CPUCTL),
        fecs_pc_after: fecs_r(falcon::PC),
        fecs_exci_after: fecs_r(falcon::EXCI),
        gpccs_pc_after: gpccs_r(falcon::PC),
        gpccs_exci_after: gpccs_r(falcon::EXCI),
        success: false,
        notes,
    }
}
