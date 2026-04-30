// SPDX-License-Identifier: AGPL-3.0-or-later
//! FECS falcon method interface — direct BAR0 register communication.
//!
//! After ACR bootstrap + falcon start, FECS accepts method commands via:
//!   - `0x409500` (FECS_FALCON_ADDR / method data)
//!   - `0x409504` (FECS_FALCON_METHOD / method ID)
//!   - `0x409800` / `0x409804` (completion polling)
//!
//! This matches nouveau's `gf100_gr_fecs_*` helpers in `gf100.c`.

use crate::error::{DriverError, DriverResult};
use crate::vfio::channel::registers::falcon;
use crate::vfio::device::MappedBar;

const FECS_MTHD_DATA: usize = falcon::FECS_BASE + falcon::MTHD_DATA;
const FECS_MTHD_CMD: usize = falcon::FECS_BASE + falcon::MTHD_CMD;
const FECS_MTHD_STATUS: usize = falcon::FECS_BASE + falcon::MTHD_STATUS;
const FECS_MTHD_STATUS2: usize = falcon::FECS_BASE + falcon::MTHD_STATUS2;

/// Submit a method to FECS and wait for completion.
///
/// Follows nouveau's `gf100_gr_fecs_ctrl_ctxsw`:
///   1. Write `0x409804 = 0x01` (set trigger flag for firmware)
///   2. Clear `0x409800 = 0`
///   3. Write data to `0x409500`
///   4. Write method to `0x409504`
///   5. Poll `0x409804` until firmware clears it to `0x00`
///   6. Read `0x409800`: `0x01` = success, `0x02` = error
fn fecs_ctrl_ctxsw(bar0: &MappedBar, method: u32, data: u32, timeout_ms: u64) -> DriverResult<u32> {
    let _ = bar0.write_u32(falcon::FECS_BASE + 0x840, 0x8000_0000);
    let _ = bar0.write_u32(FECS_MTHD_STATUS2, 0xFFFF_FFFF);
    let _ = bar0.write_u32(FECS_MTHD_STATUS, 0);
    let _ = bar0.write_u32(FECS_MTHD_DATA, data);
    let _ = bar0.write_u32(FECS_MTHD_CMD, method);

    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms);
    loop {
        std::thread::sleep(std::time::Duration::from_millis(1));
        let status2 = bar0.read_u32(FECS_MTHD_STATUS2).unwrap_or(0xDEAD);
        // Nouveau: status2 == 0x01 = success, 0x02 = error
        if status2 == 0x0000_0001 {
            let result = bar0.read_u32(FECS_MTHD_DATA).unwrap_or(0);
            return Ok(result);
        }
        if status2 == 0x0000_0002 {
            return Err(DriverError::SubmitFailed(
                format!("FECS method {method:#06x} error: status2={status2:#010x}").into(),
            ));
        }
        if std::time::Instant::now() > deadline {
            let status = bar0.read_u32(FECS_MTHD_STATUS).unwrap_or(0xDEAD);
            return Err(DriverError::OracleError(
                format!(
                    "FECS method {method:#06x} timeout ({timeout_ms}ms): status2={status2:#010x} status={status:#010x}"
                )
                .into(),
            ));
        }
    }
}

/// FECS method path using `0x409800` bit polling (Nouveau's grctx protocol).
///
/// Used by bind_pointer, wfi_golden_save. Matches Nouveau's
/// `gf100_grctx_generate_main` which triggers the falcon via 0x409840.
fn fecs_method_poll(
    bar0: &MappedBar,
    method: u32,
    data: u32,
    success_mask: u32,
    error_mask: u32,
    timeout_ms: u64,
) -> DriverResult<u32> {
    // Wake trigger: Nouveau writes 0x80000000 to CTXSW_MAILBOX(2) at 0x409840
    // before each method. Without this, a halted falcon won't process the method.
    let _ = bar0.write_u32(falcon::FECS_BASE + 0x840, 0x8000_0000);
    let _ = bar0.write_u32(FECS_MTHD_STATUS, 0);
    let _ = bar0.write_u32(FECS_MTHD_DATA, data);
    let _ = bar0.write_u32(FECS_MTHD_CMD, method);

    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms);
    loop {
        std::thread::sleep(std::time::Duration::from_millis(1));
        let status = bar0.read_u32(FECS_MTHD_STATUS).unwrap_or(0);
        if status & success_mask != 0 {
            return Ok(status);
        }
        if error_mask != 0 && status & error_mask != 0 {
            return Err(DriverError::SubmitFailed(
                format!(
                    "FECS method {method:#06x} error: status={status:#010x} (error_mask={error_mask:#010x})"
                )
                .into(),
            ));
        }
        if std::time::Instant::now() > deadline {
            return Err(DriverError::OracleError(
                format!(
                    "FECS method {method:#06x} timeout ({timeout_ms}ms): status={status:#010x}"
                )
                .into(),
            ));
        }
    }
}

/// Set FECS watchdog timeout.
///
/// Nouveau: method `0x21`, data = timeout value (typically `0x7fffffff`).
pub fn fecs_set_watchdog_timeout(bar0: &MappedBar, timeout: u32) -> DriverResult<()> {
    tracing::info!(
        timeout = format!("{timeout:#010x}"),
        "FECS: set watchdog timeout"
    );
    fecs_ctrl_ctxsw(bar0, 0x21, timeout, 2000)?;
    Ok(())
}

/// Discover GR context image size.
///
/// Nouveau: method `0x10`, returns the context size in `0x409500`.
/// This is the first method called after FECS starts and confirms
/// the falcon firmware is responsive.
pub fn fecs_discover_image_size(bar0: &MappedBar) -> DriverResult<u32> {
    tracing::info!("FECS: discover context image size (method 0x10)");
    fecs_ctrl_ctxsw(bar0, 0x10, 0, 2000)?;
    let size = bar0.read_u32(FECS_MTHD_DATA).unwrap_or(0);
    tracing::info!(
        size,
        size_hex = format!("{size:#010x}"),
        "FECS: context image size"
    );
    Ok(size)
}

/// Discover zcull context image size.
///
/// Nouveau: method `0x16`.
pub fn fecs_discover_zcull_image_size(bar0: &MappedBar) -> DriverResult<u32> {
    tracing::info!("FECS: discover zcull image size (method 0x16)");
    fecs_ctrl_ctxsw(bar0, 0x16, 0, 2000)?;
    let size = bar0.read_u32(FECS_MTHD_DATA).unwrap_or(0);
    tracing::info!(size, "FECS: zcull image size");
    Ok(size)
}

/// Discover PM context image size.
///
/// Nouveau: method `0x25`.
pub fn fecs_discover_pm_image_size(bar0: &MappedBar) -> DriverResult<u32> {
    tracing::info!("FECS: discover PM image size (method 0x25)");
    fecs_ctrl_ctxsw(bar0, 0x25, 0, 2000)?;
    let size = bar0.read_u32(FECS_MTHD_DATA).unwrap_or(0);
    tracing::info!(size, "FECS: PM image size");
    Ok(size)
}

/// Bind a context pointer to FECS (for golden context generation).
///
/// Nouveau's `gf100_gr_fecs_bind_pointer`: method `0x03`, data = inst addr.
/// The address is `0x80000000 | (inst_addr >> 12)` for the firmware path.
pub fn fecs_bind_pointer(bar0: &MappedBar, inst_addr: u64) -> DriverResult<()> {
    let data = 0x8000_0000 | (inst_addr >> 12) as u32;
    tracing::info!(
        inst_addr = format!("{inst_addr:#010x}"),
        data = format!("{data:#010x}"),
        "FECS: bind context pointer (method 0x03)"
    );
    fecs_method_poll(bar0, 0x03, data, 0x10, 0x20, 2000)?;
    Ok(())
}

/// WFI + save golden context image.
///
/// Nouveau's `gf100_gr_fecs_wfi_golden_save`: method `0x09`.
pub fn fecs_wfi_golden_save(bar0: &MappedBar, inst_addr: u64) -> DriverResult<()> {
    let data = 0x8000_0000 | (inst_addr >> 12) as u32;
    tracing::info!(
        data = format!("{data:#010x}"),
        "FECS: WFI golden save (method 0x09)"
    );
    fecs_method_poll(bar0, 0x09, data, 0x10, 0x20, 2000)?;
    Ok(())
}

/// Apply GP100+ FECS exception configuration.
///
/// Nouveau's `gp100_gr_init_fecs_exceptions` writes `0x409c24 = 0x000e0002`.
/// This enables FECS to handle exceptions from GR sub-units.
pub fn fecs_init_exceptions(bar0: &MappedBar) {
    const FECS_EXCEPTION_VAL: u32 = 0x000e_0002;
    let reg = falcon::FECS_BASE + falcon::EXCEPTION_REG;
    let _ = bar0.write_u32(reg, FECS_EXCEPTION_VAL);
    tracing::info!("FECS: exception config {reg:#08x} = {FECS_EXCEPTION_VAL:#010x}");
}

/// Probe FECS method interface — call discover sizes and report results.
///
/// Returns (ctx_size, zcull_size, pm_size) or error details.
pub fn fecs_probe_methods(bar0: &MappedBar) -> FecsMethodProbe {
    let pre_cpuctl = bar0
        .read_u32(falcon::FECS_BASE + falcon::CPUCTL)
        .unwrap_or(0xDEAD);
    let pre_status = bar0.read_u32(FECS_MTHD_STATUS).unwrap_or(0xDEAD);

    let ctx_size = fecs_discover_image_size(bar0);
    let zcull_size = fecs_discover_zcull_image_size(bar0);
    let pm_size = fecs_discover_pm_image_size(bar0);
    let watchdog = fecs_set_watchdog_timeout(bar0, 0x7fff_ffff);

    let post_cpuctl = bar0
        .read_u32(falcon::FECS_BASE + falcon::CPUCTL)
        .unwrap_or(0xDEAD);
    let post_status = bar0.read_u32(FECS_MTHD_STATUS).unwrap_or(0xDEAD);

    FecsMethodProbe {
        pre_cpuctl,
        pre_status,
        ctx_size,
        zcull_size,
        pm_size,
        watchdog,
        post_cpuctl,
        post_status,
    }
}

// ---- Internal firmware method interface ----
//
// When using Nouveau's internal (open-source) FECS firmware, the method protocol
// differs from the external (NVIDIA-signed) firmware:
//
// Internal: trigger=0x409840, data=0x409500, cmd=0x409504, completion=0x409800 bit 31
// External: trigger=0x409804, data=0x409500, cmd=0x409504, completion=0x409804→0x01
//
// Matches `gf100_grctx_generate()` in ctxgf100.c.

/// Submit a method to internal FECS firmware.
///
/// Protocol (from Nouveau's `gf100_grctx_generate`):
///   1. Write `0x409840 = 0x80000000` (wake trigger / context switch mailbox)
///   2. Write data to `0x409500`
///   3. Write method to `0x409504`
///   4. Poll `0x409800` for bit 31 set
///
/// IMPORTANT: Nouveau does NOT clear 0x409800 before method 0x01. The boot
/// flag (bit 31) is still set from FECS init, so the poll succeeds immediately.
/// The "method" is really just writing scratch registers that FECS reads
/// asynchronously after waking from halt.
fn fecs_internal_method(
    bar0: &MappedBar,
    method: u32,
    data: u32,
    timeout_ms: u64,
) -> DriverResult<()> {
    let _ = bar0.write_u32(falcon::FECS_BASE + 0x840, 0x8000_0000);
    let _ = bar0.write_u32(FECS_MTHD_DATA, data);
    let _ = bar0.write_u32(FECS_MTHD_CMD, method);

    // Nouveau polls IMMEDIATELY after the writes — the first read catches the
    // stale boot flag in 0x409800 before FECS has time to wake and clear it.
    // This is by design: the "bind" method just writes scratch registers that
    // FECS reads asynchronously after waking.
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms);
    loop {
        let status = bar0.read_u32(FECS_MTHD_STATUS).unwrap_or(0);
        if status & 0x8000_0000 != 0 {
            return Ok(());
        }
        if std::time::Instant::now() > deadline {
            let cpuctl = bar0
                .read_u32(falcon::FECS_BASE + falcon::CPUCTL)
                .unwrap_or(0xDEAD);
            return Err(DriverError::OracleError(
                format!(
                    "FECS internal method {method:#06x} timeout ({timeout_ms}ms): \
                     status={status:#010x} cpuctl={cpuctl:#010x}"
                )
                .into(),
            ));
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

/// Bind a channel to internal FECS firmware (method 0x01).
///
/// Nouveau's `gf100_grctx_generate` internal path:
///   - data = `0x80000000 | (inst_addr >> 12)`
///   - method = 0x01
///
/// `inst_addr` is the physical/IOVA of the channel instance block.
pub fn fecs_internal_bind_channel(bar0: &MappedBar, inst_iova: u64) -> DriverResult<()> {
    let data = 0x8000_0000 | (inst_iova >> 12) as u32;
    let pre_status = bar0.read_u32(FECS_MTHD_STATUS).unwrap_or(0xDEAD);
    tracing::info!(
        inst_iova = format_args!("{inst_iova:#x}"),
        data = format_args!("{data:#010x}"),
        pre_status = format_args!("{pre_status:#010x}"),
        "FECS internal: bind channel (method 0x01)"
    );
    // DO NOT clear 0x409800 — Nouveau relies on the boot flag (bit 31) being
    // already set. The poll succeeds immediately from the boot-completion flag.
    fecs_internal_method(bar0, 0x0000_0001, data, 2000)
}

/// Trigger context save/unload for internal FECS firmware.
///
/// Nouveau's `gf100_grctx_generate` internal unload path:
///   1. Clear "next channel valid" bit in 0x409b04
///   2. Write 0x100 to 0x409000 (fake context switch interrupt)
///   3. Poll 0x409b00 bit 31 cleared
pub fn fecs_internal_save_context(bar0: &MappedBar) -> DriverResult<()> {
    tracing::info!("FECS internal: triggering context save (fake ctxsw interrupt)");

    // Clear "next channel valid"
    let cur = bar0.read_u32(0x0040_9b04).unwrap_or(0);
    let _ = bar0.write_u32(0x0040_9b04, cur & !0x8000_0000);

    // Trigger context switch interrupt
    let _ = bar0.write_u32(0x0040_9000, 0x0000_0100);

    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(2000);
    loop {
        std::thread::sleep(std::time::Duration::from_millis(1));
        let val = bar0.read_u32(0x0040_9b00).unwrap_or(0xDEAD);
        if val & 0x8000_0000 == 0 {
            tracing::info!(
                val = format_args!("{val:#010x}"),
                "FECS internal: context saved"
            );
            return Ok(());
        }
        if std::time::Instant::now() > deadline {
            return Err(DriverError::OracleError(
                format!("FECS internal context save timeout: 0x409b00={val:#010x}").into(),
            ));
        }
    }
}

/// Results of probing the FECS method interface.
pub struct FecsMethodProbe {
    /// FECS cpuctl before method calls.
    pub pre_cpuctl: u32,
    /// FECS 0x409800 status before method calls.
    pub pre_status: u32,
    /// Context image size from method 0x10.
    pub ctx_size: DriverResult<u32>,
    /// Zcull image size from method 0x16.
    pub zcull_size: DriverResult<u32>,
    /// PM image size from method 0x25.
    pub pm_size: DriverResult<u32>,
    /// Watchdog timeout set result (method 0x21).
    pub watchdog: DriverResult<()>,
    /// FECS cpuctl after method calls.
    pub post_cpuctl: u32,
    /// FECS 0x409800 status after method calls.
    pub post_status: u32,
}

impl std::fmt::Display for FecsMethodProbe {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(
            f,
            "FECS Method Probe: cpuctl {:#010x} → {:#010x}, status {:#010x} → {:#010x}",
            self.pre_cpuctl, self.post_cpuctl, self.pre_status, self.post_status
        )?;
        match &self.ctx_size {
            Ok(s) => writeln!(f, "  Context image size: {s} bytes ({s:#010x})")?,
            Err(e) => writeln!(f, "  Context image size: FAILED — {e}")?,
        }
        match &self.zcull_size {
            Ok(s) => writeln!(f, "  Zcull image size:   {s} bytes ({s:#010x})")?,
            Err(e) => writeln!(f, "  Zcull image size:   FAILED — {e}")?,
        }
        match &self.pm_size {
            Ok(s) => writeln!(f, "  PM image size:      {s} bytes ({s:#010x})")?,
            Err(e) => writeln!(f, "  PM image size:      FAILED — {e}")?,
        }
        match &self.watchdog {
            Ok(()) => writeln!(f, "  Watchdog:           set OK")?,
            Err(e) => writeln!(f, "  Watchdog:           FAILED — {e}")?,
        }
        Ok(())
    }
}
