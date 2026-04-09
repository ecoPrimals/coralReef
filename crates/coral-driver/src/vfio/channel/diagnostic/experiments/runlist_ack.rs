// SPDX-License-Identifier: AGPL-3.0-or-later

use super::super::super::page_tables::write_u32_le;
use super::super::super::registers::*;
use super::context::ExperimentContext;
use crate::error::DriverResult;

/// W: Runlist ACK protocol — submit runlist, poll PFIFO_INTR for bit 30, ACK.
pub(super) fn runlist_ack_protocol(ctx: &mut ExperimentContext<'_>) -> DriverResult<()> {
    let pb = ctx.pb();
    let _ = ctx.w(pb + pbdma::CTX_USERD_LO, 0xDEAD_0008);
    let _ = ctx.w(pb + pbdma::CTX_SIGNATURE, 0xDEAD_0010);
    let _ = ctx.w(pb + pbdma::CTX_GP_BASE_LO, 0xDEAD_0048);

    let _ = ctx.w(pfifo::INTR, 0xFFFF_FFFF);
    std::thread::sleep(std::time::Duration::from_millis(2));
    let pfifo_pre = ctx.r(pfifo::INTR);
    tracing::info!("║   W: pre PFIFO_INTR={pfifo_pre:#010x}");

    let _ = ctx.w(pccsr::inst(ctx.channel_id), ctx.pccsr_inst_val);
    std::thread::sleep(std::time::Duration::from_millis(2));
    let _ = ctx.w(pccsr::channel(ctx.channel_id), pccsr::CHANNEL_ENABLE_SET);
    std::thread::sleep(std::time::Duration::from_millis(2));

    let gp_entry: u64 = (NOP_PB_IOVA & 0xFFFF_FFFC) | ((2_u64) << (32 + 10));
    ctx.gpfifo_ring[0..8].copy_from_slice(&gp_entry.to_le_bytes());
    write_u32_le(ctx.userd_page, ramuserd::GP_PUT, 1);
    write_u32_le(ctx.userd_page, ramuserd::GP_GET, 0);
    std::sync::atomic::fence(std::sync::atomic::Ordering::SeqCst);

    ctx.submit_runlist()?;

    let mut rl_completed = false;
    for poll in 0..20 {
        std::thread::sleep(std::time::Duration::from_millis(5));
        let intr = ctx.r(pfifo::INTR);
        if intr & 0x4000_0000 != 0 {
            rl_completed = true;
            tracing::info!("║   W: PFIFO_INTR bit30 SET after {poll}*5ms: {intr:#010x}");
            let ack_val = ctx.r(pfifo::RUNLIST_ACK);
            tracing::info!("║   W: RUNLIST_ACK={ack_val:#010x}");
            let _ = ctx.w(pfifo::RUNLIST_ACK, 1u32 << ctx.target_runlist);
            std::thread::sleep(std::time::Duration::from_millis(2));
            let _ = ctx.w(pfifo::INTR, 0x4000_0000);
            std::thread::sleep(std::time::Duration::from_millis(2));
            break;
        }
        if poll == 19 {
            tracing::info!("║   W: PFIFO_INTR bit30 NEVER SET — intr={intr:#010x}");
        }
    }
    let post_ack = ctx.r(pfifo::INTR);
    let pccsr_post = ctx.r(pccsr::channel(ctx.channel_id));
    let ctx_userd = ctx.r(pb + pbdma::CTX_USERD_LO);
    let ctx_sig = ctx.r(pb + pbdma::CTX_SIGNATURE);
    let sentinel_changed = ctx_userd != 0xDEAD_0008 || ctx_sig != 0xDEAD_0010;
    tracing::info!(
        "║   W: post-ack PFIFO={post_ack:#010x} PCCSR={pccsr_post:#010x} sched={} loaded={sentinel_changed} rl_done={rl_completed}",
        pccsr_post & 2 != 0
    );
    tracing::info!("║   W: CTX_USERD={ctx_userd:#010x} CTX_SIG={ctx_sig:#010x}");

    let _ = ctx.w(usermode::NOTIFY_CHANNEL_PENDING, ctx.channel_id);
    std::thread::sleep(std::time::Duration::from_millis(100));

    let post_db_pccsr = ctx.r(pccsr::channel(ctx.channel_id));
    let post_db_intr = ctx.r(pbdma::intr(ctx.target_pbdma));
    let ctx_fetch = ctx.r(pb + pbdma::CTX_GP_BASE_LO);
    let dir_fetch = ctx.r(pb + pbdma::GP_FETCH);
    tracing::info!(
        "║   W: post-db PCCSR={post_db_pccsr:#010x} PBDMA_INTR={post_db_intr:#010x} CTX_GP={ctx_fetch:#010x} FETCH={dir_fetch:#010x}"
    );
    Ok(())
}

/// X: Full nouveau-style — INST_BIND + enable + runlist + ACK + doorbell.
pub(super) fn inst_bind_with_runlist_ack(ctx: &mut ExperimentContext<'_>) -> DriverResult<()> {
    let pb = ctx.pb();
    let _ = ctx.w(pb + pbdma::CTX_USERD_LO, 0xDEAD_0008);
    let _ = ctx.w(pb + pbdma::CTX_SIGNATURE, 0xDEAD_0010);
    let _ = ctx.w(pb + pbdma::CTX_GP_BASE_LO, 0xDEAD_0048);
    let _ = ctx.w(pfifo::INTR, 0xFFFF_FFFF);
    std::thread::sleep(std::time::Duration::from_millis(2));

    let _ = ctx.w(
        pccsr::inst(ctx.channel_id),
        ctx.pccsr_inst_val | pccsr::INST_BIND_TRUE,
    );
    std::thread::sleep(std::time::Duration::from_millis(2));
    let _ = ctx.w(pccsr::channel(ctx.channel_id), pccsr::CHANNEL_ENABLE_SET);
    std::thread::sleep(std::time::Duration::from_millis(10));

    let bind_pccsr = ctx.r(pccsr::channel(ctx.channel_id));
    let bind_ctx_userd = ctx.r(pb + pbdma::CTX_USERD_LO);
    let bind_ctx_sig = ctx.r(pb + pbdma::CTX_SIGNATURE);
    tracing::info!(
        "║   X: post-bind PCCSR={bind_pccsr:#010x} CTX_USERD={bind_ctx_userd:#010x} CTX_SIG={bind_ctx_sig:#010x}"
    );

    let gp_entry: u64 = (NOP_PB_IOVA & 0xFFFF_FFFC) | ((2_u64) << (32 + 10));
    ctx.gpfifo_ring[0..8].copy_from_slice(&gp_entry.to_le_bytes());
    write_u32_le(ctx.userd_page, ramuserd::GP_PUT, 1);
    write_u32_le(ctx.userd_page, ramuserd::GP_GET, 0);
    std::sync::atomic::fence(std::sync::atomic::Ordering::SeqCst);

    ctx.submit_runlist()?;

    let mut rl_completed = false;
    for poll in 0..20 {
        std::thread::sleep(std::time::Duration::from_millis(5));
        let intr = ctx.r(pfifo::INTR);
        if intr & 0x4000_0000 != 0 {
            rl_completed = true;
            tracing::info!("║   X: PFIFO_INTR bit30 SET after {poll}*5ms: {intr:#010x}");
            let ack_val = ctx.r(pfifo::RUNLIST_ACK);
            tracing::info!("║   X: RUNLIST_ACK={ack_val:#010x}");
            let _ = ctx.w(pfifo::RUNLIST_ACK, 1u32 << ctx.target_runlist);
            let _ = ctx.w(pfifo::INTR, 0x4000_0000);
            std::thread::sleep(std::time::Duration::from_millis(2));
            break;
        }
        if poll == 19 {
            tracing::info!("║   X: PFIFO_INTR bit30 NEVER SET — intr={intr:#010x}");
        }
    }

    let pccsr_post = ctx.r(pccsr::channel(ctx.channel_id));
    let ctx_userd_post = ctx.r(pb + pbdma::CTX_USERD_LO);
    let ctx_sig_post = ctx.r(pb + pbdma::CTX_SIGNATURE);
    let sentinel_changed = ctx_userd_post != 0xDEAD_0008 || ctx_sig_post != 0xDEAD_0010;
    tracing::info!(
        "║   X: post-ack PCCSR={pccsr_post:#010x} sched={} loaded={sentinel_changed} rl_done={rl_completed}",
        pccsr_post & 2 != 0
    );

    let _ = ctx.w(usermode::NOTIFY_CHANNEL_PENDING, ctx.channel_id);
    std::thread::sleep(std::time::Duration::from_millis(100));

    let post_db_pccsr = ctx.r(pccsr::channel(ctx.channel_id));
    let post_db_intr = ctx.r(pbdma::intr(ctx.target_pbdma));
    let dir_gp_put = ctx.r(pb + pbdma::GP_PUT);
    let dir_gp_fetch = ctx.r(pb + pbdma::GP_FETCH);
    tracing::info!(
        "║   X: post-db PCCSR={post_db_pccsr:#010x} PBDMA_INTR={post_db_intr:#010x} GP_PUT={dir_gp_put} GP_FETCH={dir_gp_fetch}"
    );
    Ok(())
}

/// Y: GV100 preempt + INST_BIND + ACK — evict stale contexts first.
pub(super) fn preempt_inst_bind_ack(ctx: &mut ExperimentContext<'_>) -> DriverResult<()> {
    let pb = ctx.pb();
    let _ = ctx.w(pb + pbdma::CTX_USERD_LO, 0xDEAD_0008);
    let _ = ctx.w(pb + pbdma::CTX_SIGNATURE, 0xDEAD_0010);
    let _ = ctx.w(pb + pbdma::CTX_GP_BASE_LO, 0xDEAD_0048);
    let _ = ctx.w(pfifo::INTR, 0xFFFF_FFFF);
    std::thread::sleep(std::time::Duration::from_millis(2));

    let _ = ctx.w(pfifo::GV100_PREEMPT, 1u32 << ctx.target_runlist);
    std::thread::sleep(std::time::Duration::from_millis(10));
    let preempt_intr = ctx.r(pfifo::INTR);
    tracing::info!("║   Y: post-preempt PFIFO_INTR={preempt_intr:#010x}");

    if preempt_intr & 0x4000_0000 != 0 {
        let ack = ctx.r(pfifo::RUNLIST_ACK);
        let _ = ctx.w(pfifo::RUNLIST_ACK, 1u32 << ctx.target_runlist);
        let _ = ctx.w(pfifo::INTR, 0x4000_0000);
        tracing::info!("║   Y: preempt ACK'd (ack={ack:#010x})");
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let _ = ctx.w(pfifo::INTR, 0xFFFF_FFFF);

    let _ = ctx.w(
        pccsr::inst(ctx.channel_id),
        ctx.pccsr_inst_val | pccsr::INST_BIND_TRUE,
    );
    std::thread::sleep(std::time::Duration::from_millis(2));
    let _ = ctx.w(pccsr::channel(ctx.channel_id), pccsr::CHANNEL_ENABLE_SET);
    std::thread::sleep(std::time::Duration::from_millis(10));

    let bind_pccsr = ctx.r(pccsr::channel(ctx.channel_id));
    tracing::info!("║   Y: post-bind PCCSR={bind_pccsr:#010x}");

    let gp_entry: u64 = (NOP_PB_IOVA & 0xFFFF_FFFC) | ((2_u64) << (32 + 10));
    ctx.gpfifo_ring[0..8].copy_from_slice(&gp_entry.to_le_bytes());
    write_u32_le(ctx.userd_page, ramuserd::GP_PUT, 1);
    write_u32_le(ctx.userd_page, ramuserd::GP_GET, 0);
    std::sync::atomic::fence(std::sync::atomic::Ordering::SeqCst);

    ctx.submit_runlist()?;

    let mut rl_completed = false;
    for poll in 0..20 {
        std::thread::sleep(std::time::Duration::from_millis(5));
        let intr = ctx.r(pfifo::INTR);
        if intr & 0x4000_0000 != 0 {
            rl_completed = true;
            let ack_val = ctx.r(pfifo::RUNLIST_ACK);
            tracing::info!(
                "║   Y: PFIFO_INTR bit30 SET after {poll}*5ms: {intr:#010x} ACK={ack_val:#010x}"
            );
            let _ = ctx.w(pfifo::RUNLIST_ACK, 1u32 << ctx.target_runlist);
            let _ = ctx.w(pfifo::INTR, 0x4000_0000);
            std::thread::sleep(std::time::Duration::from_millis(2));
            break;
        }
        if poll == 19 {
            tracing::info!("║   Y: PFIFO_INTR bit30 NEVER SET — intr={intr:#010x}");
        }
    }

    let pccsr_post = ctx.r(pccsr::channel(ctx.channel_id));
    let ctx_userd_post = ctx.r(pb + pbdma::CTX_USERD_LO);
    let ctx_sig_post = ctx.r(pb + pbdma::CTX_SIGNATURE);
    let sentinel_changed = ctx_userd_post != 0xDEAD_0008 || ctx_sig_post != 0xDEAD_0010;
    tracing::info!(
        "║   Y: post-ack PCCSR={pccsr_post:#010x} sched={} loaded={sentinel_changed} rl_done={rl_completed}",
        pccsr_post & 2 != 0
    );
    tracing::info!("║   Y: CTX_USERD={ctx_userd_post:#010x} CTX_SIG={ctx_sig_post:#010x}");

    let _ = ctx.w(usermode::NOTIFY_CHANNEL_PENDING, ctx.channel_id);
    std::thread::sleep(std::time::Duration::from_millis(100));

    let post_db_pccsr = ctx.r(pccsr::channel(ctx.channel_id));
    let post_db_intr = ctx.r(pbdma::intr(ctx.target_pbdma));
    let dir_gp_put = ctx.r(pb + pbdma::GP_PUT);
    let dir_gp_fetch = ctx.r(pb + pbdma::GP_FETCH);
    let pfifo_post = ctx.r(pfifo::INTR);
    tracing::info!(
        "║   Y: post-db PCCSR={post_db_pccsr:#010x} INTR={post_db_intr:#010x} GP_PUT={dir_gp_put} FETCH={dir_gp_fetch} PFIFO={pfifo_post:#010x}"
    );
    Ok(())
}
