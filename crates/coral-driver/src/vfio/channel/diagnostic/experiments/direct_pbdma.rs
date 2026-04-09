// SPDX-License-Identifier: AGPL-3.0-or-later

use super::super::super::page_tables::write_u32_le;
use super::super::super::registers::*;
use super::super::types::ExperimentOrdering;
use super::context::ExperimentContext;
use crate::error::DriverResult;

/// E/F: Direct PBDMA register programming — bypass scheduler entirely.
pub(super) fn direct_pbdma_programming(ctx: &mut ExperimentContext<'_>) -> DriverResult<()> {
    let pb = ctx.pb();
    let _ = ctx.w(pccsr::inst(ctx.channel_id), ctx.pccsr_inst_val);
    std::thread::sleep(std::time::Duration::from_millis(5));
    let _ = ctx.w(pccsr::channel(ctx.channel_id), pccsr::CHANNEL_ENABLE_SET);
    std::thread::sleep(std::time::Duration::from_millis(5));

    let _ = ctx.w(pb + 0x40, ctx.gpfifo_iova as u32);
    let _ = ctx.w(
        pb + 0x44,
        (ctx.gpfifo_iova >> 32) as u32 | (ctx.limit2 << 16),
    );
    let _ = ctx.w(
        pb + 0xD0,
        (ctx.userd_iova as u32 & 0xFFFF_FE00) | PBDMA_TARGET_SYS_MEM_COHERENT,
    );
    let _ = ctx.w(pb + 0xD4, (ctx.userd_iova >> 32) as u32);
    let _ = ctx.w(pb + 0xC0, 0x0000_FACE);
    let _ = ctx.w(pb + 0xAC, 0x1000_3080);
    let _ = ctx.w(pb + 0xA8, 0x0000_1100);
    let _ = ctx.w(pb + 0x54, 0);
    Ok(())
}

/// G/H/I: Direct PBDMA + activate (reset GP_FETCH, write GPFIFO, GP_PUT, etc.)
pub(super) fn direct_pbdma_activate(ctx: &mut ExperimentContext<'_>) -> DriverResult<()> {
    let pb = ctx.pb();
    let _ = ctx.w(pccsr::inst(ctx.channel_id), ctx.pccsr_inst_val);
    std::thread::sleep(std::time::Duration::from_millis(5));
    let _ = ctx.w(pccsr::channel(ctx.channel_id), pccsr::CHANNEL_ENABLE_SET);
    std::thread::sleep(std::time::Duration::from_millis(5));

    let _ = ctx.w(pb + 0x40, ctx.gpfifo_iova as u32);
    let _ = ctx.w(
        pb + 0x44,
        (ctx.gpfifo_iova >> 32) as u32 | (ctx.limit2 << 16),
    );
    let _ = ctx.w(
        pb + 0xD0,
        (ctx.userd_iova as u32 & 0xFFFF_FE00) | PBDMA_TARGET_SYS_MEM_COHERENT,
    );
    let _ = ctx.w(pb + 0xD4, (ctx.userd_iova >> 32) as u32);
    let _ = ctx.w(pb + 0xC0, 0x0000_FACE);
    let _ = ctx.w(pb + 0xAC, 0x1000_3080);
    let _ = ctx.w(pb + 0xA8, 0x0000_1100);

    let _ = ctx.w(pb + 0x48, 0);
    let _ = ctx.w(pb + 0x4C, 0);

    let gp_entry: u64 = (NOP_PB_IOVA & 0xFFFF_FFFC) | ((2_u64) << (32 + 10));
    ctx.gpfifo_ring[0..8].copy_from_slice(&gp_entry.to_le_bytes());
    write_u32_le(ctx.userd_page, ramuserd::GP_PUT, 1);
    write_u32_le(ctx.userd_page, ramuserd::GP_GET, 0);
    std::sync::atomic::fence(std::sync::atomic::Ordering::SeqCst);

    let _ = ctx.w(pb + 0x54, 1);

    if matches!(
        ctx.cfg.ordering,
        ExperimentOrdering::DirectPbdmaActivateDoorbell
    ) {
        let _ = ctx.w(usermode::NOTIFY_CHANNEL_PENDING, ctx.channel_id);
    }
    if matches!(
        ctx.cfg.ordering,
        ExperimentOrdering::DirectPbdmaActivateScheduled
    ) {
        let _ = ctx.w(
            pccsr::channel(ctx.channel_id),
            pccsr::CHANNEL_ENABLE_SET | 0x2,
        );
    }
    Ok(())
}

/// T: I config + doorbell AFTER SCHED bit set.
pub(super) fn direct_pbdma_sched_doorbell(ctx: &mut ExperimentContext<'_>) -> DriverResult<()> {
    let pb = ctx.pb();
    let _ = ctx.w(pccsr::inst(ctx.channel_id), ctx.pccsr_inst_val);
    std::thread::sleep(std::time::Duration::from_millis(5));
    let _ = ctx.w(pccsr::channel(ctx.channel_id), pccsr::CHANNEL_ENABLE_SET);
    std::thread::sleep(std::time::Duration::from_millis(5));

    let _ = ctx.w(pb + pbdma::GP_BASE_LO, ctx.gpfifo_iova as u32);
    let _ = ctx.w(
        pb + pbdma::GP_BASE_HI,
        (ctx.gpfifo_iova >> 32) as u32 | (ctx.limit2 << 16),
    );
    let _ = ctx.w(
        pb + pbdma::USERD_LO,
        (ctx.userd_iova as u32 & 0xFFFF_FE00) | PBDMA_TARGET_SYS_MEM_COHERENT,
    );
    let _ = ctx.w(pb + pbdma::USERD_HI, (ctx.userd_iova >> 32) as u32);
    let _ = ctx.w(pb + pbdma::SIGNATURE, 0x0000_FACE);
    let _ = ctx.w(pb + pbdma::CHANNEL_INFO, 0x1000_3080);
    let _ = ctx.w(pb + pbdma::CONFIG, 0x0000_1100);
    let _ = ctx.w(pb + pbdma::GP_FETCH, 0);
    let _ = ctx.w(pb + pbdma::GP_STATE, 0);

    let gp_entry: u64 = (NOP_PB_IOVA & 0xFFFF_FFFC) | ((2_u64) << (32 + 10));
    ctx.gpfifo_ring[0..8].copy_from_slice(&gp_entry.to_le_bytes());
    write_u32_le(ctx.userd_page, ramuserd::GP_PUT, 1);
    write_u32_le(ctx.userd_page, ramuserd::GP_GET, 0);
    std::sync::atomic::fence(std::sync::atomic::Ordering::SeqCst);

    let _ = ctx.w(pb + pbdma::GP_PUT, 1);
    let _ = ctx.w(
        pccsr::channel(ctx.channel_id),
        pccsr::CHANNEL_ENABLE_SET | 0x2,
    );
    std::thread::sleep(std::time::Duration::from_millis(20));

    let _ = ctx.w(usermode::NOTIFY_CHANNEL_PENDING, ctx.channel_id);
    std::thread::sleep(std::time::Duration::from_millis(100));

    let post = ctx.r(pccsr::channel(ctx.channel_id));
    let gp_fetch = ctx.r(pb + pbdma::GP_FETCH);
    let gp_put = ctx.r(pb + pbdma::GP_PUT);
    let userd_rd = ctx.r(pb + pbdma::USERD_LO);
    let sig = ctx.r(pb + pbdma::SIGNATURE);
    let ctx_userd = ctx.r(pb + pbdma::CTX_USERD_LO);
    let state = ctx.r(pb + pbdma::CHANNEL_STATE);
    let intr = ctx.r(pbdma::intr(ctx.target_pbdma));
    tracing::info!(
        "║   T: PCCSR={post:#010x} GP_PUT={gp_put} GP_FETCH={gp_fetch} USERD@D0={userd_rd:#010x} USERD@08={ctx_userd:#010x}"
    );
    tracing::info!(
        "║   T: SIG={sig:#010x} STATE={state:#010x} INTR={intr:#010x} sched={}",
        post & 2 != 0
    );
    Ok(())
}
