// SPDX-License-Identifier: AGPL-3.0-or-later

use std::fmt;

/// Opaque IMEMC/DMEMC/EMEMC control word — can only be constructed through
/// `FalconCapabilities`, preventing wrong-format bugs at compile time.
#[derive(Clone, Copy)]
pub struct PioCtrl(u32);

impl PioCtrl {
    /// Raw u32 value for writing to the hardware control register.
    #[must_use]
    pub const fn raw(self) -> u32 {
        self.0
    }
}

impl fmt::Debug for PioCtrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PioCtrl({:#010x})", self.0)
    }
}

/// Falcon security mode as decoded from SCTL/SEC_MODE register.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecurityMode {
    /// Non-secure: host has full control, unsigned firmware accepted.
    NonSecure,
    /// Light Secure: fuse-enforced, PIO works but firmware auth required for HS ops.
    LightSecure,
    /// Heavy Secure: ACR-managed, host PIO may have restrictions.
    HeavySecure,
    /// Unknown security level — SCTL bits decode to an undocumented value.
    Unknown(u32),
}

impl fmt::Display for SecurityMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonSecure => write!(f, "NS"),
            Self::LightSecure => write!(f, "LS"),
            Self::HeavySecure => write!(f, "HS"),
            Self::Unknown(v) => write!(f, "UNKNOWN({v:#x})"),
        }
    }
}

/// Discovered falcon version from HWCFG.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FalconVersion {
    /// Major version (v0..v5+). Determines CPUCTL layout and PIO protocol.
    pub major: u8,
    /// Raw HWCFG register value for reference.
    pub hwcfg_raw: u32,
}

/// CPUCTL bit layout — varies between falcon versions.
#[derive(Debug, Clone, Copy)]
pub struct CpuCtlLayout {
    /// Bit mask for STARTCPU (release from HRESET).
    pub startcpu: u32,
    /// Bit mask for IINVAL (instruction cache invalidate). Zero if not available.
    pub iinval: u32,
    /// Bit mask for HRESET (hard reset state indicator).
    pub hreset: u32,
    /// Bit mask for HALTED state indicator.
    pub halted: u32,
}

/// PIO (Programmed I/O) bit layout for IMEM/DMEM/EMEM control registers.
#[derive(Debug, Clone, Copy)]
pub struct PioLayout {
    /// Bit for auto-increment on write. Expected: BIT(24) = 0x0100_0000.
    pub write_autoinc: u32,
    /// Bit for auto-increment on read. Expected: BIT(25) = 0x0200_0000.
    pub read_autoinc: u32,
    /// Bit for marking IMEM pages as secure. Expected: BIT(28) = 0x1000_0000.
    pub secure_flag: u32,
    /// Whether PIO write+readback was validated on hardware.
    pub write_validated: bool,
    /// Whether PIO read mode was validated on hardware.
    pub read_validated: bool,
}

/// Complete discovered capabilities for a falcon instance.
#[derive(Debug, Clone)]
pub struct FalconCapabilities {
    /// Human-readable falcon name (FECS, GPCCS, SEC2, PMU).
    pub name: String,
    /// BAR0 base address of this falcon.
    pub base: usize,
    /// Falcon version.
    pub version: FalconVersion,
    /// CPUCTL register bit layout.
    pub cpuctl: CpuCtlLayout,
    /// PIO register bit layout.
    pub pio: PioLayout,
    /// Current security mode from SCTL.
    pub security: SecurityMode,
    /// Raw SCTL register value.
    pub sctl_raw: u32,
    /// IMEM size in bytes (from HWCFG).
    pub imem_size: u32,
    /// DMEM size in bytes (from HWCFG).
    pub dmem_size: u32,
    /// Whether HWCFG indicates signed firmware is required.
    pub requires_signed_fw: bool,
    /// Diagnostics: any anomalies found during probing.
    pub anomalies: Vec<String>,
}

impl FalconCapabilities {
    /// Construct a PIO control word for IMEM/DMEM **write** at `addr` with auto-increment.
    #[must_use]
    pub const fn imem_write_ctrl(&self, addr: u32) -> PioCtrl {
        PioCtrl(self.pio.write_autoinc | addr)
    }

    /// Construct a PIO control word for secure IMEM **write** at `addr`.
    #[must_use]
    pub const fn imem_write_secure_ctrl(&self, addr: u32) -> PioCtrl {
        PioCtrl(self.pio.write_autoinc | self.pio.secure_flag | addr)
    }

    /// Construct a PIO control word for IMEM/DMEM **read** at `addr`.
    #[must_use]
    pub const fn imem_read_ctrl(&self, addr: u32) -> PioCtrl {
        PioCtrl(self.pio.read_autoinc | addr)
    }

    /// Construct DMEMC control for write at `addr`.
    #[must_use]
    pub const fn dmem_write_ctrl(&self, addr: u32) -> PioCtrl {
        PioCtrl(self.pio.write_autoinc | addr)
    }

    /// Construct DMEMC control for read at `addr`.
    #[must_use]
    pub const fn dmem_read_ctrl(&self, addr: u32) -> PioCtrl {
        PioCtrl(self.pio.read_autoinc | addr)
    }

    /// Construct EMEMC control for write at `offset`.
    #[must_use]
    pub const fn emem_write_ctrl(&self, offset: u32) -> PioCtrl {
        PioCtrl(self.pio.write_autoinc | offset)
    }

    /// Construct EMEMC control for read at `offset`.
    #[must_use]
    pub const fn emem_read_ctrl(&self, offset: u32) -> PioCtrl {
        PioCtrl(self.pio.read_autoinc | offset)
    }

    /// CPUCTL value to start the falcon CPU.
    #[must_use]
    pub const fn startcpu_value(&self) -> u32 {
        self.cpuctl.startcpu
    }

    /// CPUCTL value to invalidate the instruction cache.
    #[must_use]
    pub const fn iinval_value(&self) -> u32 {
        self.cpuctl.iinval
    }

    /// Whether the falcon is in a state where host PIO is expected to work.
    #[must_use]
    pub const fn pio_accessible(&self) -> bool {
        self.pio.write_validated || self.pio.read_validated
    }

    /// Whether any anomalies were detected during probing.
    #[must_use]
    pub fn has_anomalies(&self) -> bool {
        !self.anomalies.is_empty()
    }
}

impl fmt::Display for FalconCapabilities {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} @ {:#08x}: v{} {} imem={}B dmem={}B signed={} \
             pio_w={:#010x}{} pio_r={:#010x}{} cpuctl_start={:#x}",
            self.name,
            self.base,
            self.version.major,
            self.security,
            self.imem_size,
            self.dmem_size,
            self.requires_signed_fw,
            self.pio.write_autoinc,
            if self.pio.write_validated {
                "(OK)"
            } else {
                "(?)"
            },
            self.pio.read_autoinc,
            if self.pio.read_validated {
                "(OK)"
            } else {
                "(?)"
            },
            self.cpuctl.startcpu,
        )?;
        if !self.anomalies.is_empty() {
            write!(f, " anomalies={}", self.anomalies.join("; "))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{CpuCtlLayout, FalconCapabilities, FalconVersion, PioLayout, SecurityMode};

    #[test]
    fn pio_ctrl_raw_roundtrip() {
        let caps = FalconCapabilities {
            name: "test".into(),
            base: 0,
            version: FalconVersion {
                major: 4,
                hwcfg_raw: 0,
            },
            cpuctl: CpuCtlLayout {
                startcpu: 1 << 1,
                iinval: 1 << 0,
                hreset: 1 << 4,
                halted: 1 << 5,
            },
            pio: PioLayout {
                write_autoinc: 1 << 24,
                read_autoinc: 1 << 25,
                secure_flag: 1 << 28,
                write_validated: true,
                read_validated: true,
            },
            security: SecurityMode::NonSecure,
            sctl_raw: 0,
            imem_size: 65536,
            dmem_size: 5120,
            requires_signed_fw: false,
            anomalies: vec![],
        };

        assert_eq!(caps.imem_write_ctrl(0x3400).raw(), 0x0100_3400);
        assert_eq!(caps.imem_write_secure_ctrl(0).raw(), 0x1100_0000);
        assert_eq!(caps.imem_read_ctrl(0x3400).raw(), 0x0200_3400);
        assert_eq!(caps.dmem_write_ctrl(0).raw(), 0x0100_0000);
        assert_eq!(caps.dmem_read_ctrl(0x100).raw(), 0x0200_0100);
    }

    #[test]
    fn display_capabilities() {
        let caps = FalconCapabilities {
            name: "FECS".into(),
            base: 0x409000,
            version: FalconVersion {
                major: 4,
                hwcfg_raw: 0x04000140,
            },
            cpuctl: CpuCtlLayout {
                startcpu: 1 << 1,
                iinval: 1 << 0,
                hreset: 1 << 4,
                halted: 1 << 5,
            },
            pio: PioLayout {
                write_autoinc: 1 << 24,
                read_autoinc: 1 << 25,
                secure_flag: 1 << 28,
                write_validated: true,
                read_validated: true,
            },
            security: SecurityMode::LightSecure,
            sctl_raw: 0x1000,
            imem_size: 65536,
            dmem_size: 5120,
            requires_signed_fw: false,
            anomalies: vec![],
        };
        let s = caps.to_string();
        assert!(s.contains("FECS"));
        assert!(s.contains("LS"));
        assert!(s.contains("(OK)"));
    }
}
