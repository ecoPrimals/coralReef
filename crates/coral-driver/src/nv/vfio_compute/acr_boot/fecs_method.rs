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
///   1. Write `0x409804 = status2_val` (pre-set expected status)
///   2. Clear `0x409800`
///   3. Write data to `0x409500`
///   4. Write method to `0x409504`
///   5. Poll `0x409804` for `0x01` (success) or `0x02` (error)
fn fecs_ctrl_ctxsw(bar0: &MappedBar, method: u32, data: u32, timeout_ms: u64) -> DriverResult<u32> {
    let _ = bar0.write_u32(FECS_MTHD_STATUS2, 0);
    let _ = bar0.write_u32(FECS_MTHD_STATUS, 0);
    let _ = bar0.write_u32(FECS_MTHD_DATA, data);
    let _ = bar0.write_u32(FECS_MTHD_CMD, method);

    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms);
    loop {
        std::thread::sleep(std::time::Duration::from_millis(1));
        let status2 = bar0.read_u32(FECS_MTHD_STATUS2).unwrap_or(0);
        if status2 == 0x01 {
            let result = bar0.read_u32(FECS_MTHD_DATA).unwrap_or(0);
            return Ok(result);
        }
        if status2 == 0x02 {
            return Err(DriverError::SubmitFailed(
                format!(
                    "FECS method {method:#06x} error: status2=0x02 status={:#010x}",
                    bar0.read_u32(FECS_MTHD_STATUS).unwrap_or(0xDEAD)
                )
                .into(),
            ));
        }
        if std::time::Instant::now() > deadline {
            return Err(DriverError::OracleError(
                format!(
                    "FECS method {method:#06x} timeout ({timeout_ms}ms): status2={status2:#010x} status={:#010x}",
                    bar0.read_u32(FECS_MTHD_STATUS).unwrap_or(0xDEAD)
                )
                .into(),
            ));
        }
    }
}

/// Simpler FECS method path for commands that use `0x409800` bit polling.
///
/// Used by discover_image_size, bind_pointer, wfi_golden_save.
/// Nouveau's pattern: clear `0x409800`, write `0x409500`/`0x409504`, poll bits.
fn fecs_method_poll(
    bar0: &MappedBar,
    method: u32,
    data: u32,
    success_mask: u32,
    error_mask: u32,
    timeout_ms: u64,
) -> DriverResult<u32> {
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
