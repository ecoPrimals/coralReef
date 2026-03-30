// SPDX-License-Identifier: AGPL-3.0-only
//! GR engine diagnostic status from BAR0 registers.

/// GR engine diagnostic status from BAR0 registers.
#[derive(Debug)]
pub struct GrEngineStatus {
    /// BAR0 register value for PGRAPH status (offset 0x0040_0700).
    pub pgraph_status: u32,
    /// BAR0 register value for FECS CPU control (offset 0x0040_9100).
    pub fecs_cpuctl: u32,
    /// BAR0 register value for FECS mailbox 0 (offset 0x0040_9130).
    pub fecs_mailbox0: u32,
    /// BAR0 register value for FECS mailbox 1 (offset 0x0040_9134).
    pub fecs_mailbox1: u32,
    /// BAR0 register value for FECS hardware config (offset 0x0040_9800).
    pub fecs_hwcfg: u32,
    /// BAR0 register value for GPCCS CPU control (offset 0x0041_a100).
    pub gpccs_cpuctl: u32,
    /// BAR0 register value for PMC enable (offset 0x0000_0200).
    pub pmc_enable: u32,
    /// BAR0 register value for PFIFO enable (offset 0x0000_2504).
    pub pfifo_enable: u32,
}

impl GrEngineStatus {
    /// Returns `true` if the FECS (Firmware Engine Control Subsystem) is halted.
    #[must_use]
    pub fn fecs_halted(&self) -> bool {
        self.fecs_cpuctl & 0x20 != 0 || self.fecs_cpuctl == 0xDEAD_DEAD
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
            "GR: pmc={:#010x} pfifo={:#010x} pgraph={:#010x} fecs_cpu={:#010x} fecs_mb0={:#010x} fecs_mb1={:#010x} fecs_hw={:#010x} gpccs={:#010x} [fecs_halted={} gr_en={}]",
            self.pmc_enable,
            self.pfifo_enable,
            self.pgraph_status,
            self.fecs_cpuctl,
            self.fecs_mailbox0,
            self.fecs_mailbox1,
            self.fecs_hwcfg,
            self.gpccs_cpuctl,
            self.fecs_halted(),
            self.gr_enabled()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::GrEngineStatus;

    #[test]
    fn gr_engine_status_fecs_halted_bit5() {
        let s = GrEngineStatus {
            pgraph_status: 0,
            fecs_cpuctl: 0x20,
            fecs_mailbox0: 0,
            fecs_mailbox1: 0,
            fecs_hwcfg: 0,
            gpccs_cpuctl: 0,
            pmc_enable: 0,
            pfifo_enable: 0,
        };
        assert!(s.fecs_halted());
    }

    #[test]
    fn gr_engine_status_fecs_halted_dead_pattern() {
        let s = GrEngineStatus {
            pgraph_status: 0,
            fecs_cpuctl: 0xDEAD_DEAD,
            fecs_mailbox0: 0,
            fecs_mailbox1: 0,
            fecs_hwcfg: 0,
            gpccs_cpuctl: 0,
            pmc_enable: 0,
            pfifo_enable: 0,
        };
        assert!(s.fecs_halted());
    }

    #[test]
    fn gr_engine_status_fecs_not_halted() {
        let s = GrEngineStatus {
            pgraph_status: 0,
            fecs_cpuctl: 0x10,
            fecs_mailbox0: 0,
            fecs_mailbox1: 0,
            fecs_hwcfg: 0,
            gpccs_cpuctl: 0,
            pmc_enable: 0,
            pfifo_enable: 0,
        };
        assert!(!s.fecs_halted());
    }

    #[test]
    fn gr_engine_status_gr_enabled_pmc_bit12() {
        let off = GrEngineStatus {
            pgraph_status: 0,
            fecs_cpuctl: 0,
            fecs_mailbox0: 0,
            fecs_mailbox1: 0,
            fecs_hwcfg: 0,
            gpccs_cpuctl: 0,
            pmc_enable: 0,
            pfifo_enable: 0,
        };
        let on = GrEngineStatus {
            pmc_enable: 1 << 12,
            ..off
        };
        assert!(!off.gr_enabled());
        assert!(on.gr_enabled());
    }

    #[test]
    fn gr_engine_status_display_substrings() {
        let s = GrEngineStatus {
            pgraph_status: 0xA,
            fecs_cpuctl: 0x20,
            fecs_mailbox0: 0xB,
            fecs_mailbox1: 0xC,
            fecs_hwcfg: 0xD,
            gpccs_cpuctl: 0xE,
            pmc_enable: 0x1000,
            pfifo_enable: 0xF,
        };
        let text = s.to_string();
        assert!(text.contains("pmc=0x00001000"));
        assert!(text.contains("fecs_halted=true"));
        assert!(text.contains("gr_en=true"));
    }

    #[test]
    fn gr_engine_status_cold_silicon_badf_bad0() {
        let badf = GrEngineStatus {
            pgraph_status: 0xBADF_CAFE,
            fecs_cpuctl: 0x10,
            fecs_mailbox0: 0,
            fecs_mailbox1: 0,
            fecs_hwcfg: 0,
            gpccs_cpuctl: 0,
            pmc_enable: 1 << 12,
            pfifo_enable: 0,
        };
        let bad0 = GrEngineStatus {
            pgraph_status: 0xBAD0_1234,
            fecs_cpuctl: 0x10,
            fecs_mailbox0: 0,
            fecs_mailbox1: 0,
            fecs_hwcfg: 0,
            gpccs_cpuctl: 0,
            pmc_enable: 1 << 12,
            pfifo_enable: 0,
        };
        let t_badf = badf.to_string();
        let t_bad0 = bad0.to_string();
        assert!(t_badf.contains("pgraph=0xbadfcafe"));
        assert!(t_bad0.contains("pgraph=0xbad01234"));
        assert!(t_badf.contains("gr_en=true"));
    }
}
