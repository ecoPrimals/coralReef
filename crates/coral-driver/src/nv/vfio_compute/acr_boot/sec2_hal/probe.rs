// SPDX-License-Identifier: AGPL-3.0-or-later

//! SEC2 falcon state probing (`Sec2Probe`, `Sec2State`).

use std::fmt;

use crate::vfio::channel::registers::falcon;
use crate::vfio::device::MappedBar;

/// Classified SEC2 falcon state.
///
/// NOTE: SCTL (security mode) does NOT block host PIO to IMEM/DMEM/EMEM.
/// The IMEMC BIT(24) format discovery (Exp 091) proved PIO works normally
/// regardless of SCTL value. Security mode affects firmware authentication
/// and DMA behavior, not PIO access.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sec2State {
    /// HS-locked (BIOS POST state): SCTL bit 0 set, firmware authentication active.
    /// PIO to IMEM/DMEM still works — use correct IMEMC format (BIT(24) write, BIT(25) read).
    HsLocked,
    /// Clean reset (post-PMC or post-unbind): SCTL bit 0 clear, no firmware loaded.
    CleanReset,
    /// Already running (mailbox active, firmware loaded).
    Running,
    /// Powered off or clock-gated (registers return PRI error).
    Inaccessible,
}

/// Detailed SEC2 probe result.
#[derive(Debug, Clone)]
pub struct Sec2Probe {
    /// SEC2 falcon `CPUCTL` register snapshot.
    pub cpuctl: u32,
    /// SEC2 `SCTL` (security mode): informational — does NOT gate PIO access.
    /// `Bits[13:12]` encode SEC_MODE (0=NS, 1=LS, 2=HS). Value 0x3000 on GV100
    /// indicates LS mode (fuse-enforced). PIO works regardless of this value.
    pub sctl: u32,
    /// SEC2 `BOOTVEC` — entry address for IMEM boot.
    pub bootvec: u32,
    /// SEC2 `HWCFG` — falcon hardware configuration.
    pub hwcfg: u32,
    /// SEC2 `MAILBOX0` — host/falcon command or status.
    pub mailbox0: u32,
    /// SEC2 `MAILBOX1` — command parameter or secondary status.
    pub mailbox1: u32,
    /// SEC2 program counter.
    pub pc: u32,
    /// SEC2 exception info register (trap/fault details).
    pub exci: u32,
    /// Classified SEC2 state from `cpuctl` / `sctl` / `mailbox0`.
    pub state: Sec2State,
}

impl Sec2Probe {
    /// Reads SEC2 falcon registers from BAR0 and classifies runtime state.
    pub fn capture(bar0: &MappedBar) -> Self {
        let base = falcon::SEC2_BASE;
        let r = |off: usize| bar0.read_u32(base + off).unwrap_or(0xBADF_DEAD);

        let cpuctl = r(falcon::CPUCTL);
        let sctl = r(falcon::SCTL);
        let bootvec = r(falcon::BOOTVEC);
        let hwcfg = r(falcon::HWCFG);
        let mailbox0 = r(falcon::MAILBOX0);
        let mailbox1 = r(falcon::MAILBOX1);
        let pc = r(falcon::PC);
        let exci = r(falcon::EXCI);

        let state = classify_sec2(cpuctl, sctl, mailbox0);

        Self {
            cpuctl,
            sctl,
            bootvec,
            hwcfg,
            mailbox0,
            mailbox1,
            pc,
            exci,
            state,
        }
    }
}

impl fmt::Display for Sec2Probe {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "SEC2 @ {:#010x}: {:?} cpuctl={:#010x} sctl={:#010x} bootvec={:#010x} \
             hwcfg={:#010x} mb0={:#010x} mb1={:#010x} pc={:#06x} exci={:#010x}",
            falcon::SEC2_BASE,
            self.state,
            self.cpuctl,
            self.sctl,
            self.bootvec,
            self.hwcfg,
            self.mailbox0,
            self.mailbox1,
            self.pc,
            self.exci
        )
    }
}

pub(crate) fn classify_sec2(cpuctl: u32, sctl: u32, mailbox0: u32) -> Sec2State {
    use crate::vfio::channel::registers::pri;
    if pri::is_pri_error(cpuctl) || cpuctl == 0xBADF_DEAD {
        return Sec2State::Inaccessible;
    }
    if mailbox0 != 0 && (cpuctl & falcon::CPUCTL_HALTED == 0) {
        return Sec2State::Running;
    }
    // SCTL bit 0 indicates HS authentication state. This is informational —
    // it does NOT block PIO access. The distinction matters for whether
    // host-loaded firmware will be accepted for HS operations.
    if sctl & 1 != 0 {
        Sec2State::HsLocked
    } else {
        Sec2State::CleanReset
    }
}
