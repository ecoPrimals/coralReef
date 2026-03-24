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
            "  FECS after:  cpuctl={:#010x} mailbox0={:#010x}",
            self.fecs_cpuctl_after, self.fecs_mailbox0_after
        )?;
        writeln!(f, "  GPCCS after: cpuctl={:#010x}", self.gpccs_cpuctl_after)?;
        for note in &self.notes {
            writeln!(f, "  note: {note}")?;
        }
        write!(
            f,
            "╚═══════════════════════════════════════════════════════╝"
        )
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
    AcrBootResult {
        strategy,
        sec2_before,
        sec2_after,
        fecs_cpuctl_after: fecs_r(falcon::CPUCTL),
        fecs_mailbox0_after: fecs_r(falcon::MAILBOX0),
        gpccs_cpuctl_after: bar0
            .read_u32(falcon::GPCCS_BASE + falcon::CPUCTL)
            .unwrap_or(0xDEAD),
        success: false,
        notes,
    }
}
