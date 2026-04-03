// SPDX-License-Identifier: AGPL-3.0-only

use super::super::super::page_tables::write_u32_le;
use super::super::super::registers::*;
use super::context::ExperimentContext;
use crate::error::DriverResult;

/// N: INST_BIND + scheduled + GPFIFO work + doorbell — the full dispatch path.
pub(super) fn full_dispatch_with_inst_bind(ctx: &mut ExperimentContext<'_>) -> DriverResult<()> {
    let pb = ctx.pb();
    let pbdma_map = ctx.pbdma_map;

    // Pre-dispatch: reset PBDMA + clear stale PFIFO interrupts (incl. bit 8).
    ctx.reset_pbdma();
    ctx.clear_pfifo_intr();

    let gp_entry: u64 = (NOP_PB_IOVA & 0xFFFF_FFFC) | ((2_u64) << (32 + 10));
    ctx.gpfifo_ring[0..8].copy_from_slice(&gp_entry.to_le_bytes());
    write_u32_le(ctx.userd_page, ramuserd::GP_PUT, 1);
    write_u32_le(ctx.userd_page, ramuserd::GP_GET, 0);
    std::sync::atomic::fence(std::sync::atomic::Ordering::SeqCst);

    let _ = ctx.w(pccsr::inst(ctx.channel_id), ctx.pccsr_inst_val);
    std::thread::sleep(std::time::Duration::from_millis(5));
    let post_bind = ctx.r(pccsr::channel(ctx.channel_id));
    tracing::info!(
        post_bind = format_args!("{:#010x}", post_bind),
        inst_val = format_args!("{:#010x}", ctx.pccsr_inst_val),
        "║   N post-INST_BIND"
    );

    if post_bind & (pccsr::PBDMA_FAULTED_RESET | pccsr::ENG_FAULTED_RESET) != 0 {
        let bind_err = ctx.r(0x252C);
        let pfifo_intr = ctx.r(pfifo::INTR);
        tracing::info!(
            bind_err = format_args!("{:#010x}", bind_err),
            pfifo_intr = format_args!("{:#010x}", pfifo_intr),
            "║   N FAULT DIAG"
        );
        let mmu_fault_status = ctx.r(0x100E34);
        let mmu_fault_addr_lo = ctx.r(0x100E38);
        let mmu_fault_addr_hi = ctx.r(0x100E3C);
        tracing::info!(
            mmu_fault_status = format_args!("{:#010x}", mmu_fault_status),
            mmu_fault_addr_hi = format_args!("{:#010x}", mmu_fault_addr_hi),
            mmu_fault_addr_lo = format_args!("{:#010x}", mmu_fault_addr_lo),
            "║   N FAULT DIAG MMU"
        );
        for pid in [1_usize, 2] {
            let intr = ctx.r(pbdma::intr(pid));
            let status = ctx.r(0x40000 + pid * 0x2000 + 0xB0);
            let method = ctx.r(0x40000 + pid * 0x2000 + 0x1C0);
            tracing::info!(
                pid,
                intr = format_args!("{:#010x}", intr),
                state = format_args!("{:#010x}", status),
                method = format_args!("{:#010x}", method),
                "║   N PBDMA"
            );
        }
    }

    let _ = ctx.w(pccsr::channel(ctx.channel_id), pccsr::CHANNEL_ENABLE_SET);
    std::thread::sleep(std::time::Duration::from_millis(2));

    ctx.submit_runlist()?;
    std::thread::sleep(std::time::Duration::from_millis(50));

    let post_rl = ctx.r(pccsr::channel(ctx.channel_id));
    let scheduled = (post_rl & 2) != 0;
    tracing::info!(
        post_rl = format_args!("{:#010x}", post_rl),
        scheduled,
        "║   N post-runlist"
    );

    let pbdma_userd = ctx.r(pb + 0xD0);
    let pbdma_gpbase = ctx.r(pb + 0x40);
    let pbdma_sig = ctx.r(pb + 0xC0);
    let pbdma_gp_put = ctx.r(pb + 0x54);
    let pbdma_gp_fetch = ctx.r(pb + 0x48);
    let pbdma_state = ctx.r(pb + 0xB0);
    tracing::info!(
        pbdma_userd = format_args!("{:#010x}", pbdma_userd),
        pbdma_gpbase = format_args!("{:#010x}", pbdma_gpbase),
        pbdma_sig = format_args!("{:#010x}", pbdma_sig),
        pbdma_gp_put,
        pbdma_gp_fetch,
        pbdma_state = format_args!("{:#010x}", pbdma_state),
        "║   N pre-doorbell PBDMA"
    );

    let _ = ctx.w(usermode::NOTIFY_CHANNEL_PENDING, ctx.channel_id);
    std::thread::sleep(std::time::Duration::from_millis(100));

    let post_db = ctx.r(pccsr::channel(ctx.channel_id));
    let db_userd = ctx.r(pb + 0xD0);
    let db_gpbase = ctx.r(pb + 0x40);
    let db_sig = ctx.r(pb + 0xC0);
    let db_gp_put = ctx.r(pb + 0x54);
    let db_gp_fetch = ctx.r(pb + 0x48);
    let db_state = ctx.r(pb + 0xB0);
    let db_gp_state = ctx.r(pb + 0x4C);
    tracing::info!(
        post_db = format_args!("{:#010x}", post_db),
        db_userd = format_args!("{:#010x}", db_userd),
        db_gpbase = format_args!("{:#010x}", db_gpbase),
        db_sig = format_args!("{:#010x}", db_sig),
        "║   N post-doorbell"
    );
    tracing::info!(
        db_gp_put,
        db_gp_fetch,
        db_state = format_args!("{:#010x}", db_state),
        db_gp_state = format_args!("{:#010x}", db_gp_state),
        "║   N post-doorbell"
    );

    let mut seq = 0_usize;
    for pid in 0..32_usize {
        if pbdma_map & (1 << pid) == 0 {
            continue;
        }
        let rl = ctx.r(0x2390 + seq * 4);
        seq += 1;
        if rl != ctx.target_runlist {
            continue;
        }
        let pbx = 0x40000 + pid * 0x2000;
        let userd = ctx.r(pbx + 0xD0);
        let gpbase = ctx.r(pbx + 0x40);
        let sig = ctx.r(pbx + 0xC0);
        let gp_put = ctx.r(pbx + 0x54);
        let gp_fetch = ctx.r(pbx + 0x48);
        let state = ctx.r(pbx + 0xB0);
        tracing::info!(
            pid,
            userd = format_args!("{:#010x}", userd),
            gpbase = format_args!("{:#010x}", gpbase),
            sig = format_args!("{:#010x}", sig),
            gp_put,
            gp_fetch,
            state = format_args!("{:#010x}", state),
            "║   N PBDMA"
        );
    }

    if db_gp_fetch == 0 {
        std::thread::sleep(std::time::Duration::from_millis(200));
        let _ = ctx.w(usermode::NOTIFY_CHANNEL_PENDING, ctx.channel_id);
        std::thread::sleep(std::time::Duration::from_millis(200));
        let final_pccsr = ctx.r(pccsr::channel(ctx.channel_id));
        let final_intr = ctx.r(pfifo::INTR);
        let mut any_fetch = false;
        let mut seqr = 0_usize;
        for pid in 0..32_usize {
            if pbdma_map & (1 << pid) == 0 {
                continue;
            }
            let rl = ctx.r(0x2390 + seqr * 4);
            seqr += 1;
            if rl != ctx.target_runlist {
                continue;
            }
            let pbx = 0x40000 + pid * 0x2000;
            let gp_fetch = ctx.r(pbx + 0x48);
            let userd = ctx.r(pbx + 0xD0);
            let state = ctx.r(pbx + 0xB0);
            if gp_fetch != 0 {
                any_fetch = true;
            }
            tracing::info!(
                pid,
                gp_fetch,
                userd = format_args!("{:#010x}", userd),
                state = format_args!("{:#010x}", state),
                "║   N retry PBDMA"
            );
        }
        tracing::info!(
            final_pccsr = format_args!("{:#010x}", final_pccsr),
            final_intr = format_args!("{:#010x}", final_intr),
            any_fetch,
            "║   N retry"
        );
    }
    Ok(())
}

/// O: N + PREEMPT to force context switch into PBDMA.
pub(super) fn full_dispatch_with_preempt(ctx: &mut ExperimentContext<'_>) -> DriverResult<()> {
    let pb = ctx.pb();

    let gp_entry: u64 = (NOP_PB_IOVA & 0xFFFF_FFFC) | ((2_u64) << (32 + 10));
    ctx.gpfifo_ring[0..8].copy_from_slice(&gp_entry.to_le_bytes());
    write_u32_le(ctx.userd_page, ramuserd::GP_PUT, 1);
    write_u32_le(ctx.userd_page, ramuserd::GP_GET, 0);
    std::sync::atomic::fence(std::sync::atomic::Ordering::SeqCst);

    let _ = ctx.w(pccsr::inst(ctx.channel_id), ctx.pccsr_inst_val);
    std::thread::sleep(std::time::Duration::from_millis(5));
    let _ = ctx.w(pccsr::channel(ctx.channel_id), pccsr::CHANNEL_ENABLE_SET);
    std::thread::sleep(std::time::Duration::from_millis(2));
    ctx.submit_runlist()?;
    std::thread::sleep(std::time::Duration::from_millis(50));

    let post_rl = ctx.r(pccsr::channel(ctx.channel_id));
    let sched = post_rl & 2 != 0;
    tracing::info!(
        post_rl = format_args!("{:#010x}", post_rl),
        sched,
        "║   O post-runlist"
    );

    let preempt_ch = (1_u32 << 24) | ctx.channel_id;
    let _ = ctx.w(0x2634, preempt_ch);
    std::thread::sleep(std::time::Duration::from_millis(50));

    let post_preempt = ctx.r(pccsr::channel(ctx.channel_id));
    let preempt_rb = ctx.r(0x2634);
    tracing::info!(
        post_preempt = format_args!("{:#010x}", post_preempt),
        preempt_rb = format_args!("{:#010x}", preempt_rb),
        "║   O post-preempt(ch)"
    );

    let preempt_rl = (1_u32 << 20) | (ctx.target_runlist << 16);
    let _ = ctx.w(0x2634, preempt_rl);
    std::thread::sleep(std::time::Duration::from_millis(50));

    let post_rl_preempt = ctx.r(pccsr::channel(ctx.channel_id));
    let preempt_reg = ctx.r(0x2634);
    tracing::info!(
        post_rl_preempt = format_args!("{:#010x}", post_rl_preempt),
        preempt = format_args!("{:#010x}", preempt_reg),
        "║   O post-preempt(rl)"
    );

    let _ = ctx.w(usermode::NOTIFY_CHANNEL_PENDING, ctx.channel_id);
    std::thread::sleep(std::time::Duration::from_millis(100));

    let post_db = ctx.r(pccsr::channel(ctx.channel_id));
    let gp_fetch = ctx.r(pb + 0x48);
    let gp_put = ctx.r(pb + 0x54);
    let userd_lo = ctx.r(pb + 0xD0);
    let sig = ctx.r(pb + 0xC0);
    let state = ctx.r(pb + 0xB0);
    let gpbase = ctx.r(pb + 0x40);
    tracing::info!(
        post_db = format_args!("{:#010x}", post_db),
        gp_put,
        gp_fetch,
        userd_lo = format_args!("{:#010x}", userd_lo),
        gpbase = format_args!("{:#010x}", gpbase),
        sig = format_args!("{:#010x}", sig),
        state = format_args!("{:#010x}", state),
        "║   O final"
    );
    Ok(())
}

/// P: INST_BIND(SCHEDULED) + direct PBDMA writes + doorbell.
pub(super) fn scheduled_plus_direct_pbdma(ctx: &mut ExperimentContext<'_>) -> DriverResult<()> {
    let pb = ctx.pb();

    ctx.reset_pbdma();
    ctx.clear_pfifo_intr();
    let _ = ctx.w(pfifo::INTR_EN, 0x6181_0101);
    let _ = ctx.w(pbdma::intr_en(1), 0xFFFF_FFFF);
    let _ = ctx.w(pbdma::intr_en(2), 0xFFFF_FFFF);

    let gp_entry: u64 = (NOP_PB_IOVA & 0xFFFF_FFFC) | ((2_u64) << (32 + 10));
    ctx.gpfifo_ring[0..8].copy_from_slice(&gp_entry.to_le_bytes());
    write_u32_le(ctx.userd_page, ramuserd::GP_PUT, 1);
    write_u32_le(ctx.userd_page, ramuserd::GP_GET, 0);
    std::sync::atomic::fence(std::sync::atomic::Ordering::SeqCst);

    let _ = ctx.w(pccsr::inst(ctx.channel_id), ctx.pccsr_inst_val);
    std::thread::sleep(std::time::Duration::from_millis(5));
    let post_bind = ctx.r(pccsr::channel(ctx.channel_id));
    let _ = ctx.w(pccsr::channel(ctx.channel_id), pccsr::CHANNEL_ENABLE_SET);
    std::thread::sleep(std::time::Duration::from_millis(2));
    ctx.submit_runlist()?;
    std::thread::sleep(std::time::Duration::from_millis(100));

    let post_rl = ctx.r(pccsr::channel(ctx.channel_id));
    let sched = post_rl & 2 != 0;
    tracing::info!(
        post_bind = format_args!("{:#010x}", post_bind),
        post_rl = format_args!("{:#010x}", post_rl),
        sched,
        "║   P phase2"
    );

    for pid in [1_usize, 2] {
        let pbx = 0x40000 + pid * 0x2000;
        let userd = ctx.r(pbx + 0xD0);
        let gpbase = ctx.r(pbx + 0x40);
        let sig = ctx.r(pbx + 0xC0);
        let gp_put = ctx.r(pbx + 0x50);
        let gp_fetch = ctx.r(pbx + 0x48);
        let state = ctx.r(pbx + 0xB0);
        let intr = ctx.r(pbdma::intr(pid));
        tracing::info!(
            pid,
            userd = format_args!("{:#010x}", userd),
            gpbase = format_args!("{:#010x}", gpbase),
            sig = format_args!("{:#010x}", sig),
            gp_put,
            gp_fetch,
            state = format_args!("{:#010x}", state),
            intr = format_args!("{:#010x}", intr),
            "║   P pre-db PBDMA"
        );
    }

    let _ = ctx.w(usermode::NOTIFY_CHANNEL_PENDING, ctx.channel_id);
    std::thread::sleep(std::time::Duration::from_millis(100));

    let post_db = ctx.r(pccsr::channel(ctx.channel_id));
    let pfifo_intr = ctx.r(pfifo::INTR);
    tracing::info!(
        post_db = format_args!("{:#010x}", post_db),
        pfifo_intr = format_args!("{:#010x}", pfifo_intr),
        "║   P post-doorbell"
    );

    let mmu_fault_status = ctx.r(0x100E34);
    let mmu_fault_lo = ctx.r(0x100E38);
    let mmu_fault_hi = ctx.r(0x100E3C);
    let mmu_fault_inst_lo = ctx.r(0x100E40);
    let bind_err = ctx.r(0x252C);
    tracing::info!(
        mmu_fault_status = format_args!("{:#010x}", mmu_fault_status),
        mmu_fault_hi = format_args!("{:#010x}", mmu_fault_hi),
        mmu_fault_lo = format_args!("{:#010x}", mmu_fault_lo),
        mmu_fault_inst_lo = format_args!("{:#010x}", mmu_fault_inst_lo),
        bind_err = format_args!("{:#010x}", bind_err),
        "║   P FAULT"
    );

    for pid in [1_usize, 2] {
        let pbx = 0x40000 + pid * 0x2000;
        let intr = ctx.r(pbdma::intr(pid));
        let hce_intr = ctx.r(pbdma::hce_intr(pid));
        let userd = ctx.r(pbx + 0xD0);
        let gpbase = ctx.r(pbx + 0x40);
        let gp_put = ctx.r(pbx + 0x50);
        let gp_fetch = ctx.r(pbx + 0x48);
        let state = ctx.r(pbx + 0xB0);
        let gp_state = ctx.r(pbx + 0x4C);
        tracing::info!(
            pid,
            intr = format_args!("{:#010x}", intr),
            hce_intr = format_args!("{:#010x}", hce_intr),
            "║   P PBDMA intr"
        );
        tracing::info!(
            pid,
            userd = format_args!("{:#010x}", userd),
            gpbase = format_args!("{:#010x}", gpbase),
            gp_put,
            gp_fetch,
            state = format_args!("{:#010x}", state),
            gp_state = format_args!("{:#010x}", gp_state),
            "║   P PBDMA regs"
        );
    }

    let nrfb_get = ctx.r(0x100E4C);
    let nrfb_put = ctx.r(0x100E50);
    let rfb_get = ctx.r(0x100E30);
    let rfb_put = ctx.r(0x100E34);
    tracing::info!(
        nrfb_get = format_args!("{:#010x}", nrfb_get),
        nrfb_put = format_args!("{:#010x}", nrfb_put),
        rfb_get = format_args!("{:#010x}", rfb_get),
        rfb_put = format_args!("{:#010x}", rfb_put),
        "║   P FAULTBUF"
    );

    let _ = ctx.w(
        pccsr::channel(ctx.channel_id),
        pccsr::PBDMA_FAULTED_RESET | pccsr::ENG_FAULTED_RESET,
    );
    let _ = ctx.w(pfifo::INTR, 0xFFFF_FFFF);
    let _ = ctx.w(pbdma::intr(1), 0xFFFF_FFFF);
    let _ = ctx.w(pbdma::intr(2), 0xFFFF_FFFF);
    std::thread::sleep(std::time::Duration::from_millis(50));
    let _ = ctx.w(usermode::NOTIFY_CHANNEL_PENDING, ctx.channel_id);
    std::thread::sleep(std::time::Duration::from_millis(300));
    let final_pccsr = ctx.r(pccsr::channel(ctx.channel_id));
    let final_fetch1 = ctx.r(pb + 0x48);
    let final_fetch2 = ctx.r(0x44000 + 0x48);
    let final_pfifo_intr = ctx.r(pfifo::INTR);
    tracing::info!(
        final_pccsr = format_args!("{:#010x}", final_pccsr),
        final_fetch1,
        final_fetch2,
        final_pfifo_intr = format_args!("{:#010x}", final_pfifo_intr),
        "║   P retry"
    );
    Ok(())
}
