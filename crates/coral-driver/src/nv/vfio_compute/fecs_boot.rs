// SPDX-License-Identifier: AGPL-3.0-only
//! Sovereign FECS/GPCCS falcon boot — direct IMEM/DMEM firmware upload.
//!
//! Loads FECS (and optionally GPCCS) firmware from `/lib/firmware/nvidia/{chip}/gr/`
//! directly into the falcon IMEM/DMEM ports, bypassing ACR secure boot.
//!
//! Two interfaces are provided:
//!
//! - **Legacy**: `falcon_upload_imem`/`falcon_upload_dmem` use hardcoded BIT(24)/BIT(25)
//!   (correct for all GM200+ falcons). Kept for backward compatibility.
//! - **Capability-based**: `falcon_boot_probed` uses [`FalconCapabilities`] discovered
//!   at runtime, with readback verification and HWCFG-driven secure flag.
//!
//! The firmware files are the same ones that nouveau/ACR loads:
//! - `fecs_bl.bin` / `gpccs_bl.bin` — IMEM bootloader
//! - `fecs_inst.bin` / `gpccs_inst.bin` — main IMEM code
//! - `fecs_data.bin` / `gpccs_data.bin` — DMEM data

use std::path::Path;

use crate::error::{DriverError, DriverResult};
use crate::vfio::channel::registers::falcon;
use crate::vfio::device::MappedBar;

use super::acr_boot::GrBlFirmware;
use super::falcon_capability::{self, FalconCapabilities, FalconPio};

/// FECS firmware blobs loaded from disk.
///
/// Bootloader is parsed through [`GrBlFirmware`] to strip `nvfw_bin_hdr` +
/// `nvfw_hs_bl_desc` headers (the W1 fix from Exp 087/091). The `bl_imem_off`
/// field indicates where the BL expects to be loaded in IMEM — the BL code is
/// position-dependent and will fault if loaded at the wrong address.
#[derive(Debug)]
pub struct FecsFirmware {
    /// Extracted BL code bytes (headers stripped via `GrBlFirmware`).
    pub bootloader: Vec<u8>,
    /// IMEM byte offset where the BL must be loaded (from `start_tag << 8`).
    /// For GV100 FECS this is typically `0x7E00`.
    pub bl_imem_off: u32,
    /// `fecs_inst.bin`: main IMEM image placed after the 256-byte-aligned bootloader block.
    pub inst: Vec<u8>,
    /// `fecs_data.bin`: DMEM image loaded at DMEM offset 0.
    pub data: Vec<u8>,
}

/// GPCCS firmware blobs loaded from disk.
///
/// Same header-stripping treatment as [`FecsFirmware`].
#[derive(Debug)]
pub struct GpccsFirmware {
    /// Extracted BL code bytes (headers stripped via `GrBlFirmware`).
    pub bootloader: Vec<u8>,
    /// IMEM byte offset where the BL must be loaded (from `start_tag << 8`).
    /// For GV100 GPCCS this is typically `0x3400`.
    pub bl_imem_off: u32,
    /// `gpccs_inst.bin`: main IMEM image after the bootloader block.
    pub inst: Vec<u8>,
    /// `gpccs_data.bin`: DMEM image at offset 0.
    pub data: Vec<u8>,
}

impl FecsFirmware {
    /// Load FECS firmware from the standard Linux firmware path.
    ///
    /// The BL file is parsed through [`GrBlFirmware`] to extract the code
    /// section and `bl_imem_off`. Raw inst/data files are loaded as-is.
    pub fn load(chip: &str) -> DriverResult<Self> {
        let base = format!("/lib/firmware/nvidia/{chip}/gr");
        let bl_raw = read_firmware(&base, "fecs_bl.bin")?;
        let bl = GrBlFirmware::parse(&bl_raw, "fecs_bl")?;
        let bl_imem_off = bl.bl_imem_off();
        Ok(Self {
            bootloader: bl.code,
            bl_imem_off,
            inst: read_firmware(&base, "fecs_inst.bin")?,
            data: read_firmware(&base, "fecs_data.bin")?,
        })
    }
}

impl GpccsFirmware {
    /// Load GPCCS firmware from the standard Linux firmware path.
    ///
    /// Same [`GrBlFirmware`] parsing as [`FecsFirmware`].
    pub fn load(chip: &str) -> DriverResult<Self> {
        let base = format!("/lib/firmware/nvidia/{chip}/gr");
        let bl_raw = read_firmware(&base, "gpccs_bl.bin")?;
        let bl = GrBlFirmware::parse(&bl_raw, "gpccs_bl")?;
        let bl_imem_off = bl.bl_imem_off();
        Ok(Self {
            bootloader: bl.code,
            bl_imem_off,
            inst: read_firmware(&base, "gpccs_inst.bin")?,
            data: read_firmware(&base, "gpccs_data.bin")?,
        })
    }
}

fn read_firmware(base_dir: &str, filename: &str) -> DriverResult<Vec<u8>> {
    let path = format!("{base_dir}/{filename}");
    std::fs::read(&path).map_err(|e| DriverError::DeviceNotFound(format!("{path}: {e}").into()))
}

/// Result of a falcon boot attempt.
#[derive(Debug)]
pub struct FalconBootResult {
    /// Falcon label (`FECS` or `GPCCS`).
    pub name: &'static str,
    /// `CPUCTL` after boot polling (halt/reset/start state).
    pub cpuctl_after: u32,
    /// `MAILBOX0` at completion (non-zero signals firmware handshake / ready).
    pub mailbox0: u32,
    /// `MAILBOX1` at completion (extended status from firmware).
    pub mailbox1: u32,
    /// True if mailbox indicates a running falcon (not halted while holding reset).
    pub running: bool,
    /// Time from `STARTCPU` to first mailbox response or timeout, in microseconds.
    pub boot_time_us: u64,
}

impl std::fmt::Display for FalconBootResult {
    /// Single-line summary of post-boot falcon registers and timing.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}: cpuctl={:#010x} mb0={:#010x} mb1={:#010x} running={} ({}us)",
            self.name,
            self.cpuctl_after,
            self.mailbox0,
            self.mailbox1,
            self.running,
            self.boot_time_us
        )
    }
}

/// Upload firmware to a falcon's IMEM via the IMEMC/IMEMD/IMEMT port registers.
///
/// The upload protocol matches nouveau's `falcon_load_firmware()`:
/// 1. Write IMEMC with auto-increment and target address
/// 2. Set IMEMT tag for each 256-byte block
/// 3. Write IMEMD with 32-bit words of firmware data
pub fn falcon_upload_imem(bar0: &MappedBar, base: usize, addr: u32, data: &[u8], secure: bool) {
    let w = |off: usize, val: u32| {
        let _ = bar0.write_u32(base + off, val);
    };

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
pub fn falcon_upload_dmem(bar0: &MappedBar, base: usize, addr: u32, data: &[u8]) {
    let w = |off: usize, val: u32| {
        let _ = bar0.write_u32(base + off, val);
    };

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
/// 2. Upload bootloader to IMEM at `bl_imem_off` (BL code is position-dependent)
/// 3. Upload main code to IMEM at offset 0
/// 4. Upload data to DMEM at address 0
/// 5. Set BOOTVEC to `bl_imem_off` so the falcon starts at the BL entry point
/// 6. Clear mailboxes
/// 7. Write CPUCTL = STARTCPU (bit 1) to release HRESET
/// 8. Poll for mailbox0 != 0 or HALTED state
///
/// The `bl_imem_off` parameter comes from [`field@GrBlFirmware::start_tag`] (`start_tag << 8`) — the
/// `start_tag << 8` value parsed from the BL file headers. For GV100:
/// GPCCS=`0x3400`, FECS=`0x7E00`. Uploading the BL at the wrong IMEM address
/// or setting BOOTVEC to the wrong value causes an immediate exception
/// (Exp 091 root cause — `0x0307` at PC=0).
pub fn falcon_boot(
    bar0: &MappedBar,
    name: &'static str,
    base: usize,
    bootloader: &[u8],
    bl_imem_off: u32,
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
        bl_imem_off = format!("{bl_imem_off:#06x}"),
        bl_size = bootloader.len(),
        inst_size = inst.len(),
        data_size = data.len(),
        "falcon boot: starting firmware upload"
    );

    if cpuctl & falcon::CPUCTL_HRESET == 0 && cpuctl != 0xDEAD_DEAD {
        tracing::warn!(
            name,
            cpuctl = format!("{cpuctl:#010x}"),
            "falcon not in HRESET — forcing halt before upload"
        );
        w(falcon::CPUCTL, falcon::CPUCTL_HRESET)?;
        std::thread::sleep(std::time::Duration::from_millis(5));
    }

    // Upload inst code to IMEM at address 0 (application code base).
    tracing::debug!(
        name,
        bytes = inst.len(),
        "uploading instruction code to IMEM[0]"
    );
    falcon_upload_imem(bar0, base, 0, inst, false);

    // Upload bootloader to IMEM at bl_imem_off (BL is position-dependent).
    tracing::debug!(
        name,
        bytes = bootloader.len(),
        offset = format!("{bl_imem_off:#06x}"),
        "uploading bootloader to IMEM[bl_imem_off]"
    );
    falcon_upload_imem(bar0, base, bl_imem_off, bootloader, false);

    // Upload data to DMEM at address 0.
    tracing::debug!(name, bytes = data.len(), "uploading data to DMEM");
    falcon_upload_dmem(bar0, base, 0, data);

    // Set boot vector to bl_imem_off — falcon starts executing the BL.
    w(falcon::BOOTVEC, bl_imem_off)?;

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
                name,
                boot_time_us = result.boot_time_us,
                mailbox0 = format!("{:#010x}", result.mailbox0),
                cpuctl = format!("{:#010x}", result.cpuctl_after),
                "falcon boot: mailbox response received"
            );
            break;
        }

        if halted && !hreset {
            result.running = false;
            tracing::warn!(
                name,
                boot_time_us = result.boot_time_us,
                cpuctl = format!("{:#010x}", result.cpuctl_after),
                "falcon boot: halted without mailbox response"
            );
            break;
        }

        if start.elapsed() > timeout {
            tracing::error!(
                name,
                boot_time_us = result.boot_time_us,
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
    let hwcfg = bar0
        .read_u32(falcon::FECS_BASE + falcon::HWCFG)
        .unwrap_or(0);
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
        fw.bl_imem_off,
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
        fw.bl_imem_off,
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

// ── Capability-based boot (uses FalconCapabilityProbe) ──────────────────

/// Boot a falcon using discovered capabilities — validates PIO format,
/// uses HWCFG-driven secure flag, and optionally verifies uploads.
///
/// This is the evolved boot path that replaces hardcoded bit assumptions
/// with runtime-discovered register layouts.
///
/// `bl_imem_off` is the IMEM byte offset where the BL expects to run
/// (from [`field@GrBlFirmware::start_tag`] as `start_tag << 8`). Inst code goes to `IMEM[0]`,
/// BL code goes to `IMEM[bl_imem_off]`, BOOTVEC is set to bl_imem_off.
pub fn falcon_boot_probed(
    bar0: &MappedBar,
    caps: &FalconCapabilities,
    bootloader: &[u8],
    bl_imem_off: u32,
    inst: &[u8],
    data: &[u8],
    verify: bool,
) -> DriverResult<FalconBootResult> {
    let name = caps.name.as_str();
    let pio = FalconPio::new(bar0, caps);

    let r = |off: usize| bar0.read_u32(caps.base + off).unwrap_or(0xDEAD_DEAD);
    let w = |off: usize, val: u32| -> DriverResult<()> {
        bar0.write_u32(caps.base + off, val).map_err(|e| {
            DriverError::SubmitFailed(format!("{name} falcon boot {off:#x}: {e}").into())
        })
    };

    let cpuctl = r(falcon::CPUCTL);
    tracing::info!(
        %name,
        cpuctl = format!("{cpuctl:#010x}"),
        bl_imem_off = format!("{bl_imem_off:#06x}"),
        security = %caps.security,
        pio_validated = caps.pio_accessible(),
        bl_size = bootloader.len(),
        inst_size = inst.len(),
        data_size = data.len(),
        "probed falcon boot: starting firmware upload"
    );

    if caps.has_anomalies() {
        for anomaly in &caps.anomalies {
            tracing::warn!(%name, anomaly, "falcon probe anomaly");
        }
    }

    // Halt falcon if running
    if cpuctl & caps.cpuctl.hreset == 0 && cpuctl != 0xDEAD_DEAD {
        tracing::warn!(%name, cpuctl = format!("{cpuctl:#010x}"), "falcon not in HRESET — forcing halt");
        w(falcon::CPUCTL, caps.cpuctl.hreset)?;
        std::thread::sleep(std::time::Duration::from_millis(5));
    }

    let secure = caps.requires_signed_fw;
    if secure {
        tracing::warn!(%name, "HWCFG indicates signed firmware required — marking pages secure");
    }

    // Upload inst code to IMEM[0] (application code base)
    if verify {
        let mismatches = pio.upload_imem_verified(0, inst, secure);
        if mismatches > 0 {
            tracing::error!(%name, mismatches, "inst IMEM verify failed");
        }
    } else {
        pio.upload_imem(0, inst, secure);
    }

    // Upload bootloader to IMEM[bl_imem_off] (BL is position-dependent)
    if verify {
        let mismatches = pio.upload_imem_verified(bl_imem_off, bootloader, secure);
        if mismatches > 0 {
            tracing::error!(%name, mismatches, offset = bl_imem_off, "bootloader IMEM verify failed");
        }
    } else {
        pio.upload_imem(bl_imem_off, bootloader, secure);
    }

    // Upload data to DMEM[0]
    if verify {
        let mismatches = pio.upload_dmem_verified(0, data);
        if mismatches > 0 {
            tracing::error!(%name, mismatches, "data DMEM verify failed");
        }
    } else {
        pio.upload_dmem(0, data);
    }

    // Set BOOTVEC to bl_imem_off — falcon starts executing the BL entry point
    w(falcon::BOOTVEC, bl_imem_off)?;
    w(falcon::MAILBOX0, 0)?;
    w(falcon::MAILBOX1, 0)?;

    // Invalidate IMEM cache if supported
    if caps.cpuctl.iinval != 0 {
        w(falcon::CPUCTL, caps.cpuctl.iinval)?;
        std::thread::sleep(std::time::Duration::from_millis(1));
    }

    // Start CPU using discovered CPUCTL layout
    let start = std::time::Instant::now();
    w(falcon::CPUCTL, caps.startcpu_value())?;

    // Poll for boot completion
    let timeout = std::time::Duration::from_secs(2);
    let mut result = FalconBootResult {
        name: leak_name(name),
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

        let halted = result.cpuctl_after & caps.cpuctl.halted != 0;
        let hreset = result.cpuctl_after & caps.cpuctl.hreset != 0;

        if result.mailbox0 != 0 {
            result.running = !halted && !hreset;
            break;
        }

        if halted && !hreset {
            result.running = false;
            break;
        }

        if start.elapsed() > timeout {
            tracing::error!(%name, "falcon boot: timeout waiting for response");
            break;
        }
    }

    Ok(result)
}

/// Probe falcon capabilities and boot with verification.
pub fn boot_fecs_probed(bar0: &MappedBar, chip: &str) -> DriverResult<FalconBootResult> {
    let caps = falcon_capability::probe_falcon(bar0, "FECS", falcon::FECS_BASE)?;
    tracing::info!("FECS capabilities: {caps}");
    let fw = FecsFirmware::load(chip)?;
    falcon_boot_probed(
        bar0,
        &caps,
        &fw.bootloader,
        fw.bl_imem_off,
        &fw.inst,
        &fw.data,
        true,
    )
}

/// Probe falcon capabilities and boot GPCCS with verification.
pub fn boot_gpccs_probed(bar0: &MappedBar, chip: &str) -> DriverResult<FalconBootResult> {
    let caps = falcon_capability::probe_falcon(bar0, "GPCCS", falcon::GPCCS_BASE)?;
    tracing::info!("GPCCS capabilities: {caps}");
    let fw = GpccsFirmware::load(chip)?;
    falcon_boot_probed(
        bar0,
        &caps,
        &fw.bootloader,
        fw.bl_imem_off,
        &fw.inst,
        &fw.data,
        true,
    )
}

/// Convert a dynamic string to a `&'static str` for `FalconBootResult::name`.
fn leak_name(name: &str) -> &'static str {
    match name {
        "FECS" => "FECS",
        "GPCCS" => "GPCCS",
        "SEC2" => "SEC2",
        "PMU" => "PMU",
        other => Box::leak(other.to_string().into_boxed_str()),
    }
}
