// SPDX-License-Identifier: AGPL-3.0-only

use super::super::super::registers::*;
use super::context::ExperimentContext;
use crate::error::DriverResult;

/// A: bind → enable → runlist (current production path)
pub(super) fn bind_enable_runlist(ctx: &mut ExperimentContext<'_>) -> DriverResult<()> {
    let _ = ctx.w(pccsr::inst(ctx.channel_id), ctx.pccsr_inst_val);
    std::thread::sleep(std::time::Duration::from_millis(2));
    let _ = ctx.w(pccsr::channel(ctx.channel_id), pccsr::CHANNEL_ENABLE_SET);
    std::thread::sleep(std::time::Duration::from_millis(2));
    ctx.submit_runlist()?;
    Ok(())
}

/// B: bind → runlist → enable
pub(super) fn bind_runlist_enable(ctx: &mut ExperimentContext<'_>) -> DriverResult<()> {
    let _ = ctx.w(pccsr::inst(ctx.channel_id), ctx.pccsr_inst_val);
    std::thread::sleep(std::time::Duration::from_millis(2));
    ctx.submit_runlist()?;
    std::thread::sleep(std::time::Duration::from_millis(2));
    let _ = ctx.w(pccsr::channel(ctx.channel_id), pccsr::CHANNEL_ENABLE_SET);
    Ok(())
}

/// C: runlist → bind → enable
pub(super) fn runlist_bind_enable(ctx: &mut ExperimentContext<'_>) -> DriverResult<()> {
    ctx.submit_runlist()?;
    std::thread::sleep(std::time::Duration::from_millis(2));
    let _ = ctx.w(pccsr::inst(ctx.channel_id), ctx.pccsr_inst_val);
    std::thread::sleep(std::time::Duration::from_millis(2));
    let _ = ctx.w(pccsr::channel(ctx.channel_id), pccsr::CHANNEL_ENABLE_SET);
    Ok(())
}

/// D: bind_with_INST_BIND → enable → runlist (force immediate context load)
pub(super) fn bind_with_inst_bind_enable_runlist(
    ctx: &mut ExperimentContext<'_>,
) -> DriverResult<()> {
    let pb = ctx.pb();
    // Clear RAMFC-mapped PBDMA registers to sentinels so we can
    // detect which ones the INST_BIND actually loads from RAMFC.
    let _ = ctx.w(pb + 0x08, 0xBEEF_0008); // USERD_LO
    let _ = ctx.w(pb + 0x0C, 0xBEEF_000C); // USERD_HI
    let _ = ctx.w(pb + 0x10, 0xBEEF_0010); // SIGNATURE
    let _ = ctx.w(pb + 0x30, 0xBEEF_0030); // ACQUIRE
    let _ = ctx.w(pb + 0x48, 0xBEEF_0048); // GP_BASE_LO
    let _ = ctx.w(pb + 0x4C, 0xBEEF_004C); // GP_BASE_HI

    let _ = ctx.w(pccsr::inst(ctx.channel_id), ctx.pccsr_inst_val);
    std::thread::sleep(std::time::Duration::from_millis(10));

    // Snapshot immediately after INST_BIND (before enable/runlist)
    let ib_userd_lo = ctx.r(pb + 0x08);
    let ib_sig = ctx.r(pb + 0x10);
    let ib_gpb = ctx.r(pb + 0x48);
    tracing::info!(
        ib_userd_lo = format_args!("{:#010x}", ib_userd_lo),
        ib_sig = format_args!("{:#010x}", ib_sig),
        ib_gpb = format_args!("{:#010x}", ib_gpb),
        "║   D INST_BIND (BEEF=sentinel)"
    );

    let _ = ctx.w(pccsr::channel(ctx.channel_id), pccsr::CHANNEL_ENABLE_SET);
    std::thread::sleep(std::time::Duration::from_millis(2));
    ctx.submit_runlist()?;
    Ok(())
}
