// SPDX-License-Identifier: AGPL-3.0-or-later
//! GR engine diagnostic status from BAR0 registers.

/// GR engine diagnostic status from BAR0 registers.
#[derive(Debug)]
pub struct GrEngineStatus {
    /// PGRAPH idle status (BAR0 0x0040_0700).
    pub pgraph_status: u32,
    /// FECS falcon CPUCTL (BAR0 0x0040_9100).
    pub fecs_cpuctl: u32,
    /// FECS falcon MAILBOX0 (BAR0 0x0040_9040).
    pub fecs_mailbox0: u32,
    /// FECS falcon MAILBOX1 (BAR0 0x0040_9044).
    pub fecs_mailbox1: u32,
    /// FECS falcon HWCFG — IMEM/DMEM sizes, security (BAR0 0x0040_9108).
    pub fecs_hwcfg: u32,
    /// FECS context-switch mailbox 0 (BAR0 0x0040_9800).
    /// Bit 31 set = internal firmware booted; bit 0 set = external firmware booted.
    pub ctxsw_mailbox0: u32,
    /// GPCCS falcon CPUCTL (BAR0 0x0041_a100).
    pub gpccs_cpuctl: u32,
    /// PMC engine enable mask (BAR0 0x0000_0200).
    pub pmc_enable: u32,
    /// PFIFO scheduler enable (BAR0 0x0000_2504).
    pub pfifo_enable: u32,
}

impl GrEngineStatus {
    /// Returns `true` if the FECS falcon is non-functional.
    ///
    /// After firmware boots and processes methods, it halts (CPUCTL_HALTED=0x20)
    /// to wait for the next host method. This is the normal ready state.
    /// CTXSW_MAILBOX0 may be cleared by method processing, so we cannot
    /// rely on it for post-boot status.
    ///
    /// Non-functional states: read failed (0xDEAD_DEAD), PRI fault (0xBADFxxxx),
    /// or hardware reset (HRESET=0x10, falcon never started).
    /// HALTED (0x20) without HRESET means firmware ran and is idle — healthy.
    #[must_use]
    pub fn fecs_halted(&self) -> bool {
        if self.fecs_cpuctl == 0xDEAD_DEAD || self.fecs_cpuctl & 0xBAD0_0000 == 0xBAD0_0000 {
            return true;
        }
        // HRESET (0x10) set means falcon is in hardware reset — never started.
        // HALTED (0x20) WITHOUT HRESET means firmware ran and is idle-waiting.
        self.fecs_cpuctl & 0x10 != 0
    }

    /// Returns `true` if the GR (Graphics) engine is enabled in PMC.
    #[must_use]
    pub fn gr_enabled(&self) -> bool {
        self.pmc_enable & (1 << 12) != 0
    }
}

impl std::fmt::Display for GrEngineStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "GR: pmc={:#010x} pfifo={:#010x} pgraph={:#010x} fecs_cpu={:#010x} ctxsw_mb0={:#010x} gpccs={:#010x} [fecs_halted={} gr_en={}]",
            self.pmc_enable,
            self.pfifo_enable,
            self.pgraph_status,
            self.fecs_cpuctl,
            self.ctxsw_mailbox0,
            self.gpccs_cpuctl,
            self.fecs_halted(),
            self.gr_enabled()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::GrEngineStatus;

    fn status(cpuctl: u32, ctxsw_mb0: u32) -> GrEngineStatus {
        GrEngineStatus {
            pgraph_status: 0,
            fecs_cpuctl: cpuctl,
            fecs_mailbox0: 0,
            fecs_mailbox1: 0,
            fecs_hwcfg: 0,
            ctxsw_mailbox0: ctxsw_mb0,
            gpccs_cpuctl: 0,
            pmc_enable: 0,
            pfifo_enable: 0,
        }
    }

    #[test]
    fn halted_idle_without_hreset_is_healthy() {
        assert!(!status(0x20, 0x0000_0000).fecs_halted());
    }

    #[test]
    fn halted_idle_with_mailbox_is_healthy() {
        assert!(!status(0x20, 0x8000_0000).fecs_halted());
    }

    #[test]
    fn pri_fault_is_halted() {
        assert!(status(0xBADF_1002, 0).fecs_halted());
    }

    #[test]
    fn hreset_is_halted() {
        assert!(status(0x10, 0x0000_0000).fecs_halted());
    }

    #[test]
    fn dead_read_is_halted() {
        assert!(status(0xDEAD_DEAD, 0).fecs_halted());
    }

    #[test]
    fn running_cpuctl_zero_is_not_halted() {
        assert!(!status(0x00, 0).fecs_halted());
    }

    #[test]
    fn gr_enabled_pmc_bit12() {
        let off = status(0, 0);
        let on = GrEngineStatus {
            pmc_enable: 1 << 12,
            ..off
        };
        assert!(!off.gr_enabled());
        assert!(on.gr_enabled());
    }

    #[test]
    fn display_booted_shows_not_halted() {
        let s = GrEngineStatus {
            pgraph_status: 0,
            fecs_cpuctl: 0x20,
            fecs_mailbox0: 0,
            fecs_mailbox1: 0,
            fecs_hwcfg: 0,
            ctxsw_mailbox0: 0x8000_0000,
            gpccs_cpuctl: 0x20,
            pmc_enable: 0x1000,
            pfifo_enable: 0,
        };
        let text = s.to_string();
        assert!(text.contains("fecs_halted=false"));
        assert!(text.contains("gr_en=true"));
    }

    #[test]
    fn cold_silicon_badf_bad0() {
        let badf = GrEngineStatus {
            pgraph_status: 0xBADF_CAFE,
            fecs_cpuctl: 0x10,
            ctxsw_mailbox0: 0,
            ..status(0, 0)
        };
        let bad0 = GrEngineStatus {
            pgraph_status: 0xBAD0_1234,
            fecs_cpuctl: 0x10,
            ctxsw_mailbox0: 0,
            pmc_enable: 1 << 12,
            ..status(0, 0)
        };
        assert!(badf.to_string().contains("pgraph=0xbadfcafe"));
        assert!(bad0.to_string().contains("pgraph=0xbad01234"));
        assert!(bad0.to_string().contains("gr_en=true"));
    }
}
