// SPDX-License-Identifier: AGPL-3.0-only
//! Kepler Falcon PIO loader — direct IMEM/DMEM upload for FECS/GPCCS.
//!
//! On Kepler (GK110/GK210), FECS and GPCCS can be loaded via programmed I/O
//! without any ACR/WPR2/FWSEC requirements. This module implements the
//! Falcon v1 PIO upload protocol as documented in nouveau's `nvkm/falcon/v1.c`.
//!
//! Works with any [`RegisterAccess`] backend (sysfs BAR0, VFIO MappedBar, etc).

use crate::gsp::{ApplyError, RegisterAccess};

/// FECS Falcon base address (BAR0 offset).
pub const FECS_BASE: u32 = 0x0040_9000;
/// GPCCS Falcon base address (BAR0 offset).
pub const GPCCS_BASE: u32 = 0x0041_A000;

/// Falcon mailbox register 0 offset (host↔falcon communication).
pub const FALCON_MAILBOX0: u32 = 0x040;
/// Falcon mailbox register 1 offset (host↔falcon communication).
pub const FALCON_MAILBOX1: u32 = 0x044;
/// Falcon CPU control register offset.
pub const FALCON_CPUCTL: u32 = 0x100;
/// Falcon CPU control alias register offset (alternate start trigger).
pub const FALCON_CPUCTL_ALIAS: u32 = 0x130;
/// Falcon IMEM control register offset (port 0).
pub const FALCON_IMEM_CTRL: u32 = 0x180;
/// Falcon IMEM data register offset (port 0, auto-increment).
pub const FALCON_IMEM_DATA: u32 = 0x184;
/// Falcon IMEM tag register offset (one tag per 256-byte line).
pub const FALCON_IMEM_TAG: u32 = 0x188;
/// Falcon DMEM control register offset (port 0).
pub const FALCON_DMEM_CTRL: u32 = 0x1C0;
/// Falcon DMEM data register offset (port 0, auto-increment).
pub const FALCON_DMEM_DATA: u32 = 0x1C4;

/// FECS scratch register 0 (absolute BAR0 address).
pub const FECS_SCRATCH0: u32 = 0x0040_9500;
/// FECS scratch register 1 (absolute BAR0 address).
pub const FECS_SCRATCH1: u32 = 0x0040_9504;
/// FECS status register — bit 0 indicates boot completion.
pub const FECS_STATUS: u32 = 0x0040_9800;
/// FECS secondary status register (absolute BAR0 address).
pub const FECS_STATUS2: u32 = 0x0040_9804;

/// IMEM tag alignment: one tag per 256 bytes (64 u32 words).
const IMEM_TAG_WORD_INTERVAL: usize = 64;

/// CPUCTL bit 6 — determines which start register to use.
const CPUCTL_ALIAS_BIT: u32 = 1 << 6;

/// Start command value.
const CPUCTL_START: u32 = 0x2;

/// Falcon PIO upload result.
pub type FalconResult<T> = Result<T, ApplyError>;

/// Upload DMEM (data memory) to a Falcon engine.
///
/// Protocol: write start address | BIT(24) to DMEM_CTRL, then stream
/// each u32 word to DMEM_DATA. Hardware auto-increments the address.
pub fn upload_dmem(
    regs: &mut dyn RegisterAccess,
    base: u32,
    start_addr: u32,
    data: &[u8],
) -> FalconResult<()> {
    let ctrl_reg = base + FALCON_DMEM_CTRL;
    let data_reg = base + FALCON_DMEM_DATA;

    regs.write_u32(ctrl_reg, start_addr | (1 << 24))?;

    let words = data.len() / 4;
    for i in 0..words {
        let word = u32::from_le_bytes([
            data[i * 4],
            data[i * 4 + 1],
            data[i * 4 + 2],
            data[i * 4 + 3],
        ]);
        regs.write_u32(data_reg, word)?;
    }

    let remainder = data.len() % 4;
    if remainder > 0 {
        let mut last = [0u8; 4];
        last[..remainder].copy_from_slice(&data[words * 4..]);
        regs.write_u32(data_reg, u32::from_le_bytes(last))?;
    }

    Ok(())
}

/// Upload IMEM (instruction memory) to a Falcon engine.
///
/// Protocol: write start address | BIT(24) to IMEM_CTRL, then stream
/// each u32 word to IMEM_DATA. Every 64 words (256 bytes), write an
/// incrementing tag to IMEM_TAG. Pad final line to 64-word boundary with zeros.
pub fn upload_imem(
    regs: &mut dyn RegisterAccess,
    base: u32,
    start_addr: u32,
    code: &[u8],
) -> FalconResult<()> {
    let ctrl_reg = base + FALCON_IMEM_CTRL;
    let data_reg = base + FALCON_IMEM_DATA;
    let tag_reg = base + FALCON_IMEM_TAG;

    regs.write_u32(ctrl_reg, start_addr | (1 << 24))?;

    let total_words = code.len().div_ceil(4);
    let padded_words = total_words.div_ceil(IMEM_TAG_WORD_INTERVAL) * IMEM_TAG_WORD_INTERVAL;

    let mut tag = 0u32;

    for word_idx in 0..padded_words {
        if word_idx % IMEM_TAG_WORD_INTERVAL == 0 {
            regs.write_u32(tag_reg, tag)?;
            tag += 1;
        }

        let byte_off = word_idx * 4;
        let word = if byte_off + 4 <= code.len() {
            u32::from_le_bytes([
                code[byte_off],
                code[byte_off + 1],
                code[byte_off + 2],
                code[byte_off + 3],
            ])
        } else if byte_off < code.len() {
            let mut buf = [0u8; 4];
            buf[..code.len() - byte_off].copy_from_slice(&code[byte_off..]);
            u32::from_le_bytes(buf)
        } else {
            0 // padding
        };

        regs.write_u32(data_reg, word)?;
    }

    Ok(())
}

/// Start a Falcon engine after DMEM/IMEM upload.
///
/// Reads CPUCTL to determine the start register, then writes the start command.
pub fn start_falcon(regs: &mut dyn RegisterAccess, base: u32) -> FalconResult<()> {
    let cpuctl = regs.read_u32(base + FALCON_CPUCTL)?;

    let start_reg = if cpuctl & CPUCTL_ALIAS_BIT != 0 {
        base + FALCON_CPUCTL_ALIAS
    } else {
        base + FALCON_CPUCTL
    };

    regs.write_u32(start_reg, CPUCTL_START)
}

/// Read the FECS status register (0x409800) and check if bit 0 is set (booted).
pub fn fecs_booted(regs: &dyn RegisterAccess) -> FalconResult<bool> {
    let status = regs.read_u32(FECS_STATUS)?;
    Ok(status & 1 != 0)
}

/// Full FECS/GPCCS boot sequence for Kepler.
///
/// 1. Upload GPCCS DMEM + IMEM, start GPCCS
/// 2. Upload FECS DMEM + IMEM, start FECS
/// 3. Clear handshake registers
/// 4. Poll FECS_STATUS until bit 0 set (up to `timeout`)
///
/// `fecs_code`/`fecs_data`/`gpccs_code`/`gpccs_data` are raw firmware blobs.
pub fn boot_fecs_gpccs(
    regs: &mut dyn RegisterAccess,
    fecs_code: &[u8],
    fecs_data: &[u8],
    gpccs_code: &[u8],
    gpccs_data: &[u8],
    timeout: std::time::Duration,
) -> FalconResult<()> {
    // Step 1: GPCCS
    upload_dmem(regs, GPCCS_BASE, 0, gpccs_data)?;
    upload_imem(regs, GPCCS_BASE, 0, gpccs_code)?;
    start_falcon(regs, GPCCS_BASE)?;

    // Step 2: FECS
    upload_dmem(regs, FECS_BASE, 0, fecs_data)?;
    upload_imem(regs, FECS_BASE, 0, fecs_code)?;
    start_falcon(regs, FECS_BASE)?;

    // Step 3: Clear handshake registers
    regs.write_u32(FECS_STATUS, 0)?;
    regs.write_u32(FECS_BASE + 0x10C, 0)?; // FECS interrupt
    regs.write_u32(GPCCS_BASE + 0x10C, 0)?; // GPCCS interrupt

    // Step 4: Poll for FECS boot confirmation
    let start = std::time::Instant::now();
    loop {
        if fecs_booted(regs)? {
            return Ok(());
        }
        if start.elapsed() >= timeout {
            let status = regs.read_u32(FECS_STATUS)?;
            return Err(ApplyError::MmioFailed {
                offset: FECS_STATUS,
                detail: format!("FECS boot timeout after {timeout:?} (status={status:#010x})"),
            });
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

/// PMC unk260 toggle — nouveau does this around falcon load for clock gating.
pub fn pmc_unk260(regs: &mut dyn RegisterAccess, enable: bool) -> FalconResult<()> {
    regs.write_u32(0x260, u32::from(enable))
}

/// Diagnostic: read key falcon registers for status reporting.
pub fn read_falcon_diag(regs: &dyn RegisterAccess, base: u32) -> FalconResult<FalconDiagnostic> {
    Ok(FalconDiagnostic {
        cpuctl: regs.read_u32(base + FALCON_CPUCTL)?,
        mailbox0: regs.read_u32(base + FALCON_MAILBOX0)?,
        mailbox1: regs.read_u32(base + FALCON_MAILBOX1)?,
        sctl: regs.read_u32(base + 0x240)?,
        status_method: regs.read_u32(base + 0x800)?,
    })
}

/// Falcon diagnostic register snapshot.
#[derive(Debug, Clone)]
pub struct FalconDiagnostic {
    /// CPU control register (run/halt state).
    pub cpuctl: u32,
    /// Mailbox register 0 (host↔falcon communication).
    pub mailbox0: u32,
    /// Mailbox register 1 (host↔falcon communication).
    pub mailbox1: u32,
    /// Secure control register (authentication mode).
    pub sctl: u32,
    /// Method status register (host command interface).
    pub status_method: u32,
}

impl std::fmt::Display for FalconDiagnostic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "CPUCTL={:#010x} MB0={:#010x} MB1={:#010x} SCTL={:#010x} STATUS={:#010x}",
            self.cpuctl, self.mailbox0, self.mailbox1, self.sctl, self.status_method
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Mock register file for testing PIO upload sequences.
    struct MockRegs {
        writes: Vec<(u32, u32)>,
        cpuctl_val: u32,
    }

    impl MockRegs {
        fn new(cpuctl_val: u32) -> Self {
            Self {
                writes: Vec::new(),
                cpuctl_val,
            }
        }
    }

    impl RegisterAccess for MockRegs {
        fn read_u32(&self, offset: u32) -> Result<u32, ApplyError> {
            if offset & 0xFFF == FALCON_CPUCTL {
                Ok(self.cpuctl_val)
            } else if offset == FECS_STATUS {
                Ok(1) // booted
            } else {
                Ok(0)
            }
        }

        fn write_u32(&mut self, offset: u32, value: u32) -> Result<(), ApplyError> {
            self.writes.push((offset, value));
            Ok(())
        }
    }

    #[test]
    fn dmem_upload_basic() {
        let mut regs = MockRegs::new(0);
        let data = [0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08];
        upload_dmem(&mut regs, FECS_BASE, 0, &data).unwrap();

        assert_eq!(regs.writes[0], (FECS_BASE + FALCON_DMEM_CTRL, 1 << 24));
        assert_eq!(regs.writes[1], (FECS_BASE + FALCON_DMEM_DATA, 0x04030201));
        assert_eq!(regs.writes[2], (FECS_BASE + FALCON_DMEM_DATA, 0x08070605));
        assert_eq!(regs.writes.len(), 3);
    }

    #[test]
    fn imem_upload_tags_every_64_words() {
        let mut regs = MockRegs::new(0);
        // 256 bytes = 64 words = exactly one tag interval
        let code = vec![0xABu8; 256];
        upload_imem(&mut regs, FECS_BASE, 0, &code).unwrap();

        let tag_writes: Vec<_> = regs
            .writes
            .iter()
            .filter(|(off, _)| *off == FECS_BASE + FALCON_IMEM_TAG)
            .collect();
        assert_eq!(tag_writes.len(), 1);
        assert_eq!(*tag_writes[0], (FECS_BASE + FALCON_IMEM_TAG, 0));
    }

    #[test]
    fn imem_upload_pads_to_64_word_boundary() {
        let mut regs = MockRegs::new(0);
        let code = vec![0x42u8; 4]; // 1 word → should pad to 64 words
        upload_imem(&mut regs, FECS_BASE, 0, &code).unwrap();

        let data_writes = regs
            .writes
            .iter()
            .filter(|(off, _)| *off == FECS_BASE + FALCON_IMEM_DATA)
            .count();
        assert_eq!(data_writes, 64);
    }

    #[test]
    fn start_falcon_cpuctl_alias_bit() {
        let mut regs = MockRegs::new(CPUCTL_ALIAS_BIT);
        start_falcon(&mut regs, FECS_BASE).unwrap();
        assert_eq!(
            regs.writes.last().unwrap(),
            &(FECS_BASE + FALCON_CPUCTL_ALIAS, CPUCTL_START)
        );

        let mut regs2 = MockRegs::new(0);
        start_falcon(&mut regs2, FECS_BASE).unwrap();
        assert_eq!(
            regs2.writes.last().unwrap(),
            &(FECS_BASE + FALCON_CPUCTL, CPUCTL_START)
        );
    }

    #[test]
    fn fecs_booted_decodes_bit0() {
        struct StatusRegs(u32);
        impl RegisterAccess for StatusRegs {
            fn read_u32(&self, offset: u32) -> Result<u32, ApplyError> {
                if offset == FECS_STATUS {
                    Ok(self.0)
                } else {
                    Ok(0)
                }
            }
            fn write_u32(&mut self, _offset: u32, _value: u32) -> Result<(), ApplyError> {
                Ok(())
            }
        }
        assert!(!fecs_booted(&StatusRegs(0)).expect("read"));
        assert!(!fecs_booted(&StatusRegs(2)).expect("read"));
        assert!(fecs_booted(&StatusRegs(1)).expect("read"));
        assert!(fecs_booted(&StatusRegs(0xFFFF_FFFF)).expect("read"));
    }

    #[test]
    fn dmem_upload_remainder_partial_word() {
        let mut regs = MockRegs::new(0);
        let data = [0x01u8, 0x02, 0x03]; // 3 bytes → one padded u32 write
        upload_dmem(&mut regs, FECS_BASE, 0, &data).unwrap();
        assert_eq!(regs.writes[0], (FECS_BASE + FALCON_DMEM_CTRL, 1 << 24));
        assert_eq!(regs.writes[1], (FECS_BASE + FALCON_DMEM_DATA, 0x0003_0201));
        assert_eq!(regs.writes.len(), 2);
    }

    #[test]
    fn pmc_unk260_writes_boolean() {
        let mut regs = MockRegs::new(0);
        pmc_unk260(&mut regs, true).unwrap();
        assert_eq!(regs.writes.last().unwrap(), &(0x260, 1));
        pmc_unk260(&mut regs, false).unwrap();
        assert_eq!(regs.writes.last().unwrap(), &(0x260, 0));
    }

    #[test]
    fn falcon_diagnostic_display_format() {
        let d = FalconDiagnostic {
            cpuctl: 0x0000_0001,
            mailbox0: 0x0000_0002,
            mailbox1: 0x0000_0003,
            sctl: 0x0000_0004,
            status_method: 0x0000_0005,
        };
        let s = d.to_string();
        assert!(s.contains("CPUCTL=0x00000001"));
        assert!(s.contains("MB0=0x00000002"));
        assert!(s.contains("STATUS=0x00000005"));
    }
}
