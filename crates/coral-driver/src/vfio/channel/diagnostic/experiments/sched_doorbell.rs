// SPDX-License-Identifier: AGPL-3.0-only

use super::super::super::page_tables::write_u32_le;
use super::super::super::registers::*;
use super::context::ExperimentContext;
use crate::error::DriverResult;

/// R: RAMFC-mirror path — write USERD/GP_BASE/SIGNATURE to RAMFC-mapped PBDMA context offsets.
pub(super) fn ramfc_mirror_sched_doorbell(ctx: &mut ExperimentContext<'_>) -> DriverResult<()> {
    let pb = ctx.pb();
    let _ = ctx.w(pccsr::inst(ctx.channel_id), ctx.pccsr_inst_val);
    std::thread::sleep(std::time::Duration::from_millis(5));
    let _ = ctx.w(pccsr::channel(ctx.channel_id), pccsr::CHANNEL_ENABLE_SET);
    std::thread::sleep(std::time::Duration::from_millis(5));

    let _ = ctx.w(
        pb + pbdma::CTX_USERD_LO,
        (ctx.userd_iova as u32 & 0xFFFF_FE00) | PBDMA_TARGET_SYS_MEM_COHERENT,
    );
    let _ = ctx.w(pb + pbdma::CTX_USERD_HI, (ctx.userd_iova >> 32) as u32);
    let _ = ctx.w(pb + pbdma::CTX_SIGNATURE, 0x0000_FACE);
    let _ = ctx.w(pb + pbdma::CTX_ACQUIRE, 0x7FFF_F902);
    let _ = ctx.w(pb + pbdma::CTX_GP_BASE_LO, ctx.gpfifo_iova as u32);
    let _ = ctx.w(
        pb + pbdma::CTX_GP_BASE_HI,
        (ctx.gpfifo_iova >> 32) as u32 | (ctx.limit2 << 16),
    );
    let _ = ctx.w(pb + pbdma::CTX_GP_FETCH, 0);
    let _ = ctx.w(pb + pbdma::CTX_GP_PUT, 0);

    let gp_entry: u64 = (NOP_PB_IOVA & 0xFFFF_FFFC) | ((2_u64) << (32 + 10));
    ctx.gpfifo_ring[0..8].copy_from_slice(&gp_entry.to_le_bytes());
    write_u32_le(ctx.userd_page, ramuserd::GP_PUT, 1);
    write_u32_le(ctx.userd_page, ramuserd::GP_GET, 0);
    std::sync::atomic::fence(std::sync::atomic::Ordering::SeqCst);

    let _ = ctx.w(pb + pbdma::CTX_GP_PUT, 1);
    let _ = ctx.w(
        pccsr::channel(ctx.channel_id),
        pccsr::CHANNEL_ENABLE_SET | 0x2,
    );
    std::thread::sleep(std::time::Duration::from_millis(20));
    let _ = ctx.w(usermode::NOTIFY_CHANNEL_PENDING, ctx.channel_id);
    std::thread::sleep(std::time::Duration::from_millis(100));

    let post = ctx.r(pccsr::channel(ctx.channel_id));
    let ctx_fetch = ctx.r(pb + pbdma::CTX_GP_FETCH);
    let ctx_put = ctx.r(pb + pbdma::CTX_GP_PUT);
    let ctx_userd = ctx.r(pb + pbdma::CTX_USERD_LO);
    let ctx_sig = ctx.r(pb + pbdma::CTX_SIGNATURE);
    let direct_fetch = ctx.r(pb + pbdma::GP_FETCH);
    let direct_userd = ctx.r(pb + pbdma::USERD_LO);
    let intr = ctx.r(pbdma::intr(ctx.target_pbdma));
    let method0 = ctx.r(ctx.pb() + pbdma::METHOD0);
    let data0 = ctx.r(ctx.pb() + pbdma::DATA0);
    eprintln!(
        "║   R: PCCSR={post:#010x} CTX: PUT={ctx_put} FETCH={ctx_fetch} USERD={ctx_userd:#010x} SIG={ctx_sig:#010x}"
    );
    eprintln!(
        "║   R: DIRECT: FETCH={direct_fetch} USERD={direct_userd:#010x} INTR={intr:#010x} METHOD={method0:#010x} DATA={data0:#010x}"
    );
    Ok(())
}

/// S: Write to BOTH RAMFC-mirror AND direct PBDMA offsets.
pub(super) fn both_paths_sched_doorbell(ctx: &mut ExperimentContext<'_>) -> DriverResult<()> {
    let pb = ctx.pb();
    let _ = ctx.w(pccsr::inst(ctx.channel_id), ctx.pccsr_inst_val);
    std::thread::sleep(std::time::Duration::from_millis(5));
    let _ = ctx.w(pccsr::channel(ctx.channel_id), pccsr::CHANNEL_ENABLE_SET);
    std::thread::sleep(std::time::Duration::from_millis(5));

    let userd_val = (ctx.userd_iova as u32 & 0xFFFF_FE00) | PBDMA_TARGET_SYS_MEM_COHERENT;
    let gpbase_hi_val = (ctx.gpfifo_iova >> 32) as u32 | (ctx.limit2 << 16);

    let _ = ctx.w(pb + pbdma::GP_BASE_LO, ctx.gpfifo_iova as u32);
    let _ = ctx.w(pb + pbdma::GP_BASE_HI, gpbase_hi_val);
    let _ = ctx.w(pb + pbdma::USERD_LO, userd_val);
    let _ = ctx.w(pb + pbdma::USERD_HI, (ctx.userd_iova >> 32) as u32);
    let _ = ctx.w(pb + pbdma::SIGNATURE, 0x0000_FACE);
    let _ = ctx.w(pb + pbdma::CHANNEL_INFO, 0x1000_3080);
    let _ = ctx.w(pb + pbdma::CONFIG, 0x0000_1100);
    let _ = ctx.w(pb + pbdma::GP_FETCH, 0);
    let _ = ctx.w(pb + pbdma::GP_STATE, 0);

    let _ = ctx.w(pb + pbdma::CTX_USERD_LO, userd_val);
    let _ = ctx.w(pb + pbdma::CTX_USERD_HI, (ctx.userd_iova >> 32) as u32);
    let _ = ctx.w(pb + pbdma::CTX_SIGNATURE, 0x0000_FACE);
    let _ = ctx.w(pb + pbdma::CTX_ACQUIRE, 0x7FFF_F902);
    let _ = ctx.w(pb + pbdma::CTX_GP_BASE_LO, ctx.gpfifo_iova as u32);
    let _ = ctx.w(pb + pbdma::CTX_GP_BASE_HI, gpbase_hi_val);
    let _ = ctx.w(pb + pbdma::CTX_GP_FETCH, 0);

    let gp_entry: u64 = (NOP_PB_IOVA & 0xFFFF_FFFC) | 1 | ((2_u64) << (32 + 10));
    ctx.gpfifo_ring[0..8].copy_from_slice(&gp_entry.to_le_bytes());
    write_u32_le(ctx.userd_page, ramuserd::GP_PUT, 1);
    write_u32_le(ctx.userd_page, ramuserd::GP_GET, 0);
    std::sync::atomic::fence(std::sync::atomic::Ordering::SeqCst);
    #[cfg(target_arch = "x86_64")]
    ctx.flush_dma();

    let _ = ctx.w(pb + pbdma::GP_PUT, 1);
    let _ = ctx.w(pb + pbdma::CTX_GP_PUT, 1);
    let _ = ctx.w(
        pccsr::channel(ctx.channel_id),
        pccsr::CHANNEL_ENABLE_SET | 0x2,
    );
    std::thread::sleep(std::time::Duration::from_millis(20));
    let _ = ctx.w(usermode::NOTIFY_CHANNEL_PENDING, ctx.channel_id);
    std::thread::sleep(std::time::Duration::from_millis(100));

    let post = ctx.r(pccsr::channel(ctx.channel_id));
    let ctx_fetch = ctx.r(pb + pbdma::CTX_GP_FETCH);
    let direct_fetch = ctx.r(pb + pbdma::GP_FETCH);
    let ctx_userd = ctx.r(pb + pbdma::CTX_USERD_LO);
    let direct_userd = ctx.r(pb + pbdma::USERD_LO);
    let intr = ctx.r(pbdma::intr(ctx.target_pbdma));
    let state = ctx.r(pb + pbdma::CHANNEL_STATE);
    let method0 = ctx.r(pb + pbdma::METHOD0);
    let data0 = ctx.r(pb + pbdma::DATA0);
    eprintln!(
        "║   S: PCCSR={post:#010x} CTX_FETCH={ctx_fetch} DIR_FETCH={direct_fetch} CTX_USERD={ctx_userd:#010x} DIR_USERD={direct_userd:#010x}"
    );
    eprintln!(
        "║   S: STATE={state:#010x} INTR={intr:#010x} METHOD={method0:#010x} DATA={data0:#010x} sched={}",
        post & 2 != 0
    );
    Ok(())
}

/// U: Same as R but with GP_PUT=0, GP_GET=0 — no GPFIFO entries.
pub(super) fn clean_sched_no_work(ctx: &mut ExperimentContext<'_>) -> DriverResult<()> {
    let pb = ctx.pb();
    let _ = ctx.w(pccsr::inst(ctx.channel_id), ctx.pccsr_inst_val);
    std::thread::sleep(std::time::Duration::from_millis(5));
    let _ = ctx.w(pccsr::channel(ctx.channel_id), pccsr::CHANNEL_ENABLE_SET);
    std::thread::sleep(std::time::Duration::from_millis(5));

    let _ = ctx.w(
        pb + pbdma::CTX_USERD_LO,
        (ctx.userd_iova as u32 & 0xFFFF_FE00) | PBDMA_TARGET_SYS_MEM_COHERENT,
    );
    let _ = ctx.w(pb + pbdma::CTX_USERD_HI, (ctx.userd_iova >> 32) as u32);
    let _ = ctx.w(pb + pbdma::CTX_SIGNATURE, 0x0000_FACE);
    let _ = ctx.w(pb + pbdma::CTX_ACQUIRE, 0x7FFF_F902);
    let _ = ctx.w(pb + pbdma::CTX_GP_BASE_LO, ctx.gpfifo_iova as u32);
    let _ = ctx.w(
        pb + pbdma::CTX_GP_BASE_HI,
        (ctx.gpfifo_iova >> 32) as u32 | (ctx.limit2 << 16),
    );
    let _ = ctx.w(pb + pbdma::CTX_GP_FETCH, 0);
    let _ = ctx.w(pb + pbdma::CTX_GP_PUT, 0);

    write_u32_le(ctx.userd_page, ramuserd::GP_PUT, 0);
    write_u32_le(ctx.userd_page, ramuserd::GP_GET, 0);
    ctx.gpfifo_ring[0..8].fill(0);
    std::sync::atomic::fence(std::sync::atomic::Ordering::SeqCst);

    let _ = ctx.w(pfifo::RUNLIST_BASE, ctx.rl_base);
    let _ = ctx.w(pfifo::RUNLIST_SUBMIT, ctx.rl_submit);
    std::thread::sleep(std::time::Duration::from_millis(30));

    let _ = ctx.w(
        pccsr::channel(ctx.channel_id),
        pccsr::CHANNEL_ENABLE_SET | 0x2,
    );
    std::thread::sleep(std::time::Duration::from_millis(20));
    let _ = ctx.w(usermode::NOTIFY_CHANNEL_PENDING, ctx.channel_id);
    std::thread::sleep(std::time::Duration::from_millis(100));

    let post = ctx.r(pccsr::channel(ctx.channel_id));
    let ctx_put = ctx.r(pb + pbdma::CTX_GP_PUT);
    let ctx_fetch = ctx.r(pb + pbdma::CTX_GP_FETCH);
    let intr = ctx.r(pbdma::intr(ctx.target_pbdma));
    let state = ctx.r(pb + pbdma::CHANNEL_STATE);
    let status = pccsr::status_name(post);
    eprintln!(
        "║   U: PCCSR={post:#010x} STATUS={status} PUT={ctx_put} FETCH={ctx_fetch} STATE={state:#010x} INTR={intr:#010x}"
    );
    eprintln!(
        "║   U: faulted={} sched={}",
        post & (pccsr::PBDMA_FAULTED_RESET | pccsr::ENG_FAULTED_RESET) != 0,
        post & 2 != 0
    );
    Ok(())
}

/// U2: Same as R but GPFIFO slot 0 points to a NOP push buffer.
pub(super) fn sched_with_nop_pushbuf(ctx: &mut ExperimentContext<'_>) -> DriverResult<()> {
    let pb = ctx.pb();
    let _ = ctx.w(pccsr::inst(ctx.channel_id), ctx.pccsr_inst_val);
    std::thread::sleep(std::time::Duration::from_millis(5));
    let _ = ctx.w(pccsr::channel(ctx.channel_id), pccsr::CHANNEL_ENABLE_SET);
    std::thread::sleep(std::time::Duration::from_millis(5));

    let _ = ctx.w(
        pb + pbdma::CTX_USERD_LO,
        (ctx.userd_iova as u32 & 0xFFFF_FE00) | PBDMA_TARGET_SYS_MEM_COHERENT,
    );
    let _ = ctx.w(pb + pbdma::CTX_USERD_HI, (ctx.userd_iova >> 32) as u32);
    let _ = ctx.w(pb + pbdma::CTX_SIGNATURE, 0x0000_FACE);
    let _ = ctx.w(pb + pbdma::CTX_ACQUIRE, 0x7FFF_F902);
    let _ = ctx.w(pb + pbdma::CTX_GP_BASE_LO, ctx.gpfifo_iova as u32);
    let _ = ctx.w(
        pb + pbdma::CTX_GP_BASE_HI,
        (ctx.gpfifo_iova >> 32) as u32 | (ctx.limit2 << 16),
    );
    let _ = ctx.w(pb + pbdma::CTX_GP_FETCH, 0);
    let _ = ctx.w(pb + pbdma::CTX_GP_PUT, 0);

    let nop_header: u32 = (1 << 29) | (1 << 16) | 0x40;
    let nop_data: u32 = 0;
    let pushbuf_iova = RUNLIST_IOVA + 0x200;
    let pushbuf_slice = &mut ctx.runlist.as_mut_slice()[0x200..0x210];
    pushbuf_slice[0..4].copy_from_slice(&nop_header.to_le_bytes());
    pushbuf_slice[4..8].copy_from_slice(&nop_data.to_le_bytes());

    let gp_entry: u64 = (pushbuf_iova & 0xFFFF_FFFC) | 1 | ((2_u64) << (32 + 10));
    ctx.gpfifo_ring[0..8].copy_from_slice(&gp_entry.to_le_bytes());
    write_u32_le(ctx.userd_page, ramuserd::GP_PUT, 1);
    write_u32_le(ctx.userd_page, ramuserd::GP_GET, 0);
    std::sync::atomic::fence(std::sync::atomic::Ordering::SeqCst);
    #[cfg(target_arch = "x86_64")]
    ctx.flush_dma();

    let _ = ctx.w(pb + pbdma::CTX_GP_PUT, 1);

    let _ = ctx.w(pfifo::RUNLIST_BASE, ctx.rl_base);
    let _ = ctx.w(pfifo::RUNLIST_SUBMIT, ctx.rl_submit);
    std::thread::sleep(std::time::Duration::from_millis(30));

    let _ = ctx.w(
        pccsr::channel(ctx.channel_id),
        pccsr::CHANNEL_ENABLE_SET | 0x2,
    );
    std::thread::sleep(std::time::Duration::from_millis(20));
    let _ = ctx.w(usermode::NOTIFY_CHANNEL_PENDING, ctx.channel_id);
    std::thread::sleep(std::time::Duration::from_millis(200));

    let post = ctx.r(pccsr::channel(ctx.channel_id));
    let ctx_put = ctx.r(pb + pbdma::CTX_GP_PUT);
    let ctx_fetch = ctx.r(pb + pbdma::CTX_GP_FETCH);
    let ctx_get = ctx.r(pb + pbdma::CTX_GP_BASE_LO);
    let dir_put = ctx.r(pb + pbdma::GP_PUT);
    let dir_fetch = ctx.r(pb + pbdma::GP_FETCH);
    let intr = ctx.r(pbdma::intr(ctx.target_pbdma));
    let state = ctx.r(pb + pbdma::CHANNEL_STATE);
    let ctx_sig = ctx.r(pb + pbdma::CTX_SIGNATURE);
    let status = pccsr::status_name(post);
    eprintln!(
        "║   U2: PCCSR={post:#010x} STATUS={status} CTX_PUT={ctx_put} CTX_FETCH={ctx_fetch} DIR_PUT={dir_put} DIR_FETCH={dir_fetch}"
    );
    eprintln!(
        "║   U2: SIG={ctx_sig:#010x} STATE={state:#010x} INTR={intr:#010x} GP_BASE={ctx_get:#010x}"
    );
    eprintln!(
        "║   U2: faulted={} sched={}",
        post & (pccsr::PBDMA_FAULTED_RESET | pccsr::ENG_FAULTED_RESET) != 0,
        post & 2 != 0
    );
    Ok(())
}

/// V: Pure scheduler path — NO INST_BIND, NO direct PBDMA.
pub(super) fn scheduler_path_only(ctx: &mut ExperimentContext<'_>) -> DriverResult<()> {
    let pb = ctx.pb();
    let _ = ctx.w(pb + pbdma::CTX_USERD_LO, 0xDEAD_0008);
    let _ = ctx.w(pb + pbdma::CTX_USERD_HI, 0xDEAD_000C);
    let _ = ctx.w(pb + pbdma::CTX_SIGNATURE, 0xDEAD_0010);
    let _ = ctx.w(pb + pbdma::CTX_GP_BASE_LO, 0xDEAD_0048);
    let _ = ctx.w(pb + pbdma::CTX_GP_BASE_HI, 0xDEAD_004C);

    let sched_dis = ctx.r(pfifo::SCHED_DISABLE);
    eprintln!(
        "║   V: SCHED_DIS={sched_dis:#010x} PFIFO_INTR={:#010x}",
        ctx.r(pfifo::INTR)
    );

    let _ = ctx.w(pccsr::inst(ctx.channel_id), ctx.pccsr_inst_val);
    std::thread::sleep(std::time::Duration::from_millis(2));
    let _ = ctx.w(pccsr::channel(ctx.channel_id), pccsr::CHANNEL_ENABLE_SET);
    std::thread::sleep(std::time::Duration::from_millis(2));

    let gp_entry: u64 = (NOP_PB_IOVA & 0xFFFF_FFFC) | ((2_u64) << (32 + 10));
    ctx.gpfifo_ring[0..8].copy_from_slice(&gp_entry.to_le_bytes());
    write_u32_le(ctx.userd_page, ramuserd::GP_PUT, 1);
    write_u32_le(ctx.userd_page, ramuserd::GP_GET, 0);
    std::sync::atomic::fence(std::sync::atomic::Ordering::SeqCst);

    let _ = ctx.w(pfifo::RUNLIST_BASE, ctx.rl_base);
    let _ = ctx.w(pfifo::RUNLIST_SUBMIT, ctx.rl_submit);
    std::thread::sleep(std::time::Duration::from_millis(50));

    let post_rl = ctx.r(pccsr::channel(ctx.channel_id));
    let pfifo_intr = ctx.r(pfifo::INTR);
    let rl_ack = ctx.r(pfifo::RUNLIST_ACK);
    eprintln!(
        "║   V: post-rl PCCSR={post_rl:#010x} sched={} PFIFO_INTR={pfifo_intr:#010x} RL_ACK={rl_ack:#010x}",
        post_rl & 2 != 0
    );

    let ctx_userd = ctx.r(pb + pbdma::CTX_USERD_LO);
    let ctx_sig = ctx.r(pb + pbdma::CTX_SIGNATURE);
    let ctx_gpbase = ctx.r(pb + pbdma::CTX_GP_BASE_LO);
    let sentinel_changed = ctx_userd != 0xDEAD_0008 || ctx_sig != 0xDEAD_0010;
    eprintln!(
        "║   V: CTX: USERD={ctx_userd:#010x} SIG={ctx_sig:#010x} GP_BASE={ctx_gpbase:#010x} loaded={}",
        sentinel_changed
    );

    let _ = ctx.w(usermode::NOTIFY_CHANNEL_PENDING, ctx.channel_id);
    std::thread::sleep(std::time::Duration::from_millis(100));

    let post_db = ctx.r(pccsr::channel(ctx.channel_id));
    let ctx_userd_post = ctx.r(pb + pbdma::CTX_USERD_LO);
    let intr = ctx.r(pbdma::intr(ctx.target_pbdma));
    let pfifo_post = ctx.r(pfifo::INTR);
    eprintln!(
        "║   V: post-db PCCSR={post_db:#010x} CTX_USERD={ctx_userd_post:#010x} INTR={intr:#010x} PFIFO={pfifo_post:#010x}"
    );
    Ok(())
}
