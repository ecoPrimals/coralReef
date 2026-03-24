// SPDX-License-Identifier: AGPL-3.0-only
//! Sovereign FECS/GPCCS falcon boot — direct IMEM/DMEM firmware upload.
//!
//! Loads FECS (and optionally GPCCS) firmware from `/lib/firmware/nvidia/{chip}/gr/`
//! directly into the falcon IMEM/DMEM ports, bypassing ACR secure boot.
//!
//! This works on GV100 where FECS `HWCFG.SECURITY_MODE = 0` (unsigned firmware
//! accepted). The firmware files are the same ones that nouveau/ACR loads:
//!
//! - `fecs_bl.bin` — bootloader (IMEM at offset 0)
//! - `fecs_inst.bin` — instruction memory (IMEM after bootloader)
//! - `fecs_data.bin` — data memory (DMEM)
//! - `fecs_sig.bin` — signature (unused when secure=false)
//!
//! The upload uses the same IMEMC/IMEMD/IMEMT and DMEMC/DMEMD port registers
//! as the existing PMU falcon boot code in `devinit/pmu.rs`.

use std::path::Path;

use crate::error::{DriverError, DriverResult};
use crate::vfio::channel::registers::falcon;
use crate::vfio::device::MappedBar;

/// FECS firmware blobs loaded from disk.
#[derive(Debug)]
pub struct FecsFirmware {
    pub bootloader: Vec<u8>,
    pub inst: Vec<u8>,
    pub data: Vec<u8>,
}

/// GPCCS firmware blobs loaded from disk.
#[derive(Debug)]
pub struct GpccsFirmware {
    pub bootloader: Vec<u8>,
    pub inst: Vec<u8>,
    pub data: Vec<u8>,
}

impl FecsFirmware {
    /// Load FECS firmware from the standard Linux firmware path.
    pub fn load(chip: &str) -> DriverResult<Self> {
        let base = format!("/lib/firmware/nvidia/{chip}/gr");
        Ok(Self {
            bootloader: read_firmware(&base, "fecs_bl.bin")?,
            inst: read_firmware(&base, "fecs_inst.bin")?,
            data: read_firmware(&base, "fecs_data.bin")?,
        })
    }
}

impl GpccsFirmware {
    /// Load GPCCS firmware from the standard Linux firmware path.
    pub fn load(chip: &str) -> DriverResult<Self> {
        let base = format!("/lib/firmware/nvidia/{chip}/gr");
        Ok(Self {
            bootloader: read_firmware(&base, "gpccs_bl.bin")?,
            inst: read_firmware(&base, "gpccs_inst.bin")?,
            data: read_firmware(&base, "gpccs_data.bin")?,
        })
    }
}

fn read_firmware(base_dir: &str, filename: &str) -> DriverResult<Vec<u8>> {
    let path = format!("{base_dir}/{filename}");
    std::fs::read(&path).map_err(|e| {
        DriverError::DeviceNotFound(format!("{path}: {e}").into())
    })
}

/// Result of a falcon boot attempt.
#[derive(Debug)]
pub struct FalconBootResult {
    pub name: &'static str,
    pub cpuctl_after: u32,
    pub mailbox0: u32,
    pub mailbox1: u32,
    pub running: bool,
    pub boot_time_us: u64,
}

impl std::fmt::Display for FalconBootResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f, "{}: cpuctl={:#010x} mb0={:#010x} mb1={:#010x} running={} ({}us)",
            self.name, self.cpuctl_after, self.mailbox0, self.mailbox1,
            self.running, self.boot_time_us
        )
    }
}

/// Upload firmware to a falcon's IMEM via the IMEMC/IMEMD/IMEMT port registers.
///
/// The upload protocol matches nouveau's `falcon_load_firmware()`:
/// 1. Write IMEMC with auto-increment and target address
/// 2. Set IMEMT tag for each 256-byte block
/// 3. Write IMEMD with 32-bit words of firmware data
pub(super) fn falcon_upload_imem(bar0: &MappedBar, base: usize, addr: u32, data: &[u8], secure: bool) {
    let w = |off: usize, val: u32| { let _ = bar0.write_u32(base + off, val); };

    let sec_flag: u32 = if secure { 0x1000_0000 } else { 0 };
    w(falcon::IMEMC, 0x0100_0000 | sec_flag | addr);

    for (i, chunk) in data.chunks(4).enumerate() {
        let byte_offset = (i * 4) as u32;
        if byte_offset & 0xFF == 0 {
            w(falcon::IMEMT, (addr + byte_offset) >> 8);
        }
        let word = le_word(chunk);
        w(falcon::IMEMD, word);
    }

    let total_bytes = (data.len().div_ceil(4)) * 4;
    let remainder = total_bytes & 0xFF;
    if remainder != 0 {
        let padding_words = (256 - remainder) / 4;
        for _ in 0..padding_words {
            w(falcon::IMEMD, 0);
        }
    }
}

/// Upload data to a falcon's DMEM via the DMEMC/DMEMD port registers.
pub(super) fn falcon_upload_dmem(bar0: &MappedBar, base: usize, addr: u32, data: &[u8]) {
    let w = |off: usize, val: u32| { let _ = bar0.write_u32(base + off, val); };

    w(falcon::DMEMC, 0x0100_0000 | addr);

    for chunk in data.chunks(4) {
        w(falcon::DMEMD, le_word(chunk));
    }
}

fn le_word(chunk: &[u8]) -> u32 {
    match chunk.len() {
        4 => u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]),
        3 => u32::from_le_bytes([chunk[0], chunk[1], chunk[2], 0]),
        2 => u32::from_le_bytes([chunk[0], chunk[1], 0, 0]),
        1 => u32::from_le_bytes([chunk[0], 0, 0, 0]),
        _ => 0,
    }
}

/// Boot a falcon by loading firmware into IMEM/DMEM and releasing HRESET.
///
/// Sequence:
/// 1. Verify falcon is in HRESET
/// 2. Upload bootloader to IMEM at address 0
/// 3. Upload main code to IMEM after bootloader (256-byte aligned)
/// 4. Upload data to DMEM at address 0
/// 5. Set BOOTVEC to 0
/// 6. Clear mailboxes
/// 7. Write CPUCTL = STARTCPU (bit 1) to release HRESET
/// 8. Poll for mailbox0 != 0 or HALTED state
pub fn falcon_boot(
    bar0: &MappedBar,
    name: &'static str,
    base: usize,
    bootloader: &[u8],
    inst: &[u8],
    data: &[u8],
) -> DriverResult<FalconBootResult> {
    let r = |off: usize| bar0.read_u32(base + off).unwrap_or(0xDEAD_DEAD);
    let w = |off: usize, val: u32| -> DriverResult<()> {
        bar0.write_u32(base + off, val).map_err(|e| {
            DriverError::SubmitFailed(format!("{name} falcon boot {off:#x}: {e}").into())
        })
    };

    let cpuctl = r(falcon::CPUCTL);
    tracing::info!(
        name,
        cpuctl = format!("{cpuctl:#010x}"),
        bl_size = bootloader.len(),
        inst_size = inst.len(),
        data_size = data.len(),
        "falcon boot: starting firmware upload"
    );

    if cpuctl & falcon::CPUCTL_HRESET == 0 && cpuctl != 0xDEAD_DEAD {
        tracing::warn!(
            name, cpuctl = format!("{cpuctl:#010x}"),
            "falcon not in HRESET — forcing halt before upload"
        );
        w(falcon::CPUCTL, falcon::CPUCTL_HRESET)?;
        std::thread::sleep(std::time::Duration::from_millis(5));
    }

    // Upload bootloader to IMEM starting at address 0.
    tracing::debug!(name, bytes = bootloader.len(), "uploading bootloader to IMEM");
    falcon_upload_imem(bar0, base, 0, bootloader, false);

    // Upload main instruction code aligned to 256-byte boundary after bootloader.
    let inst_offset = bootloader.len().div_ceil(256) * 256;
    tracing::debug!(
        name, bytes = inst.len(), offset = inst_offset,
        "uploading instruction code to IMEM"
    );
    falcon_upload_imem(bar0, base, inst_offset as u32, inst, false);

    // Upload data to DMEM at address 0.
    tracing::debug!(name, bytes = data.len(), "uploading data to DMEM");
    falcon_upload_dmem(bar0, base, 0, data);

    // Set boot vector to 0 (bootloader entry).
    w(falcon::BOOTVEC, 0)?;

    // Clear mailboxes before starting.
    w(falcon::MAILBOX0, 0)?;
    w(falcon::MAILBOX1, 0)?;

    // Invalidate IMEM tags to ensure the falcon sees our upload.
    // Falcon v4+: bit 0 = IINVAL.
    w(falcon::CPUCTL, falcon::CPUCTL_IINVAL)?;
    std::thread::sleep(std::time::Duration::from_millis(1));

    // Release HRESET and start CPU execution.
    // Falcon v4+: bit 1 = STARTCPU (nouveau: gm200_flcn_fw_boot writes 0x02).
    let start = std::time::Instant::now();
    w(falcon::CPUCTL, falcon::CPUCTL_STARTCPU)?;

    // Poll for boot completion: mailbox0 != 0 or HALTED.
    let timeout = std::time::Duration::from_secs(2);
    let mut result = FalconBootResult {
        name,
        cpuctl_after: 0,
        mailbox0: 0,
        mailbox1: 0,
        running: false,
        boot_time_us: 0,
    };

    loop {
        std::thread::sleep(std::time::Duration::from_millis(5));

        result.cpuctl_after = r(falcon::CPUCTL);
        result.mailbox0 = r(falcon::MAILBOX0);
        result.mailbox1 = r(falcon::MAILBOX1);
        result.boot_time_us = start.elapsed().as_micros() as u64;

        let halted = result.cpuctl_after & falcon::CPUCTL_HALTED != 0;
        let hreset = result.cpuctl_after & falcon::CPUCTL_HRESET != 0;

        if result.mailbox0 != 0 {
            result.running = !halted && !hreset;
            tracing::info!(
                name, boot_time_us = result.boot_time_us,
                mailbox0 = format!("{:#010x}", result.mailbox0),
                cpuctl = format!("{:#010x}", result.cpuctl_after),
                "falcon boot: mailbox response received"
            );
            break;
        }

        if halted && !hreset {
            result.running = false;
            tracing::warn!(
                name, boot_time_us = result.boot_time_us,
                cpuctl = format!("{:#010x}", result.cpuctl_after),
                "falcon boot: halted without mailbox response"
            );
            break;
        }

        if start.elapsed() > timeout {
            tracing::error!(
                name, boot_time_us = result.boot_time_us,
                cpuctl = format!("{:#010x}", result.cpuctl_after),
                mailbox0 = format!("{:#010x}", result.mailbox0),
                "falcon boot: timeout waiting for response"
            );
            break;
        }
    }

    Ok(result)
}

/// Boot FECS falcon from firmware files on disk.
pub fn boot_fecs(bar0: &MappedBar, chip: &str) -> DriverResult<FalconBootResult> {
    let fw = FecsFirmware::load(chip)?;
    let hwcfg = bar0.read_u32(falcon::FECS_BASE + falcon::HWCFG).unwrap_or(0);
    let secure = hwcfg & falcon::HWCFG_SECURITY_MODE != 0;

    if secure {
        tracing::warn!(
            "FECS requires signed firmware (HWCFG secure=true) — direct upload may fail. \
             Consider ACR boot path instead."
        );
    }

    falcon_boot(
        bar0,
        "FECS",
        falcon::FECS_BASE,
        &fw.bootloader,
        &fw.inst,
        &fw.data,
    )
}

/// Boot GPCCS falcon from firmware files on disk.
pub fn boot_gpccs(bar0: &MappedBar, chip: &str) -> DriverResult<FalconBootResult> {
    let fw = GpccsFirmware::load(chip)?;
    falcon_boot(
        bar0,
        "GPCCS",
        falcon::GPCCS_BASE,
        &fw.bootloader,
        &fw.inst,
        &fw.data,
    )
}

/// Full GR falcon boot sequence: FECS + GPCCS.
///
/// Returns the FECS boot result. If GPCCS boot fails, logs a warning
/// but returns the FECS result (FECS is the primary scheduler).
pub fn boot_gr_falcons(bar0: &MappedBar, chip: &str) -> DriverResult<FalconBootResult> {
    let fecs_result = boot_fecs(bar0, chip)?;
    tracing::info!("FECS: {fecs_result}");

    match boot_gpccs(bar0, chip) {
        Ok(gpccs_result) => tracing::info!("GPCCS: {gpccs_result}"),
        Err(e) => tracing::warn!("GPCCS boot failed (non-fatal): {e}"),
    }

    Ok(fecs_result)
}

/// Check if the GR firmware directory exists for a given chip.
pub fn firmware_available(chip: &str) -> bool {
    Path::new(&format!("/lib/firmware/nvidia/{chip}/gr/fecs_inst.bin")).exists()
}
