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
    eprintln!(
        "║   N post-INST_BIND: {post_bind:#010x} (inst_val={:#010x})",
        ctx.pccsr_inst_val
    );

    if post_bind & (pccsr::PBDMA_FAULTED_RESET | pccsr::ENG_FAULTED_RESET) != 0 {
        let bind_err = ctx.r(0x252C);
        let pfifo_intr = ctx.r(pfifo::INTR);
        eprintln!("║   N FAULT DIAG: BIND_ERR={bind_err:#010x} PFIFO_INTR={pfifo_intr:#010x}");
        let mmu_fault_status = ctx.r(0x100E34);
        let mmu_fault_addr_lo = ctx.r(0x100E38);
        let mmu_fault_addr_hi = ctx.r(0x100E3C);
        eprintln!(
            "║   N FAULT DIAG: MMU_STATUS={mmu_fault_status:#010x} ADDR={mmu_fault_addr_hi:#010x}_{mmu_fault_addr_lo:#010x}"
        );
        for pid in [1_usize, 2] {
            let intr = ctx.r(pbdma::intr(pid));
            let status = ctx.r(0x40000 + pid * 0x2000 + 0xB0);
            let method = ctx.r(0x40000 + pid * 0x2000 + 0x1C0);
            eprintln!(
                "║   N PBDMA{pid} INTR={intr:#010x} STATE={status:#010x} METHOD={method:#010x}"
            );
        }
    }

    let _ = ctx.w(pccsr::channel(ctx.channel_id), pccsr::CHANNEL_ENABLE_SET);
    std::thread::sleep(std::time::Duration::from_millis(2));

    ctx.submit_runlist()?;
    std::thread::sleep(std::time::Duration::from_millis(50));

    let post_rl = ctx.r(pccsr::channel(ctx.channel_id));
    let scheduled = (post_rl & 2) != 0;
    eprintln!("║   N post-runlist: {post_rl:#010x} scheduled={scheduled}");

    let pbdma_userd = ctx.r(pb + 0xD0);
    let pbdma_gpbase = ctx.r(pb + 0x40);
    let pbdma_sig = ctx.r(pb + 0xC0);
    let pbdma_gp_put = ctx.r(pb + 0x54);
    let pbdma_gp_fetch = ctx.r(pb + 0x48);
    let pbdma_state = ctx.r(pb + 0xB0);
    eprintln!(
        "║   N pre-doorbell PBDMA: USERD={pbdma_userd:#010x} GP_BASE={pbdma_gpbase:#010x} SIG={pbdma_sig:#010x} GP_PUT={pbdma_gp_put} GP_FETCH={pbdma_gp_fetch} STATE={pbdma_state:#010x}"
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
    eprintln!(
        "║   N post-doorbell: PCCSR={post_db:#010x} USERD={db_userd:#010x} GP_BASE={db_gpbase:#010x} SIG={db_sig:#010x}"
    );
    eprintln!(
        "║   N post-doorbell: GP_PUT={db_gp_put} GP_FETCH={db_gp_fetch} STATE={db_state:#010x} GP_STATE={db_gp_state:#010x}"
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
        eprintln!(
            "║   N PBDMA{pid}: USERD={userd:#010x} GP_BASE={gpbase:#010x} SIG={sig:#010x} GP_PUT={gp_put} GP_FETCH={gp_fetch} STATE={state:#010x}"
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
            eprintln!(
                "║   N retry PBDMA{pid}: GP_FETCH={gp_fetch} USERD={userd:#010x} STATE={state:#010x}"
            );
        }
        eprintln!(
            "║   N retry: PCCSR={final_pccsr:#010x} PFIFO_INTR={final_intr:#010x} any_fetch={any_fetch}"
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
    eprintln!(
        "║   O post-runlist: {post_rl:#010x} sched={}",
        post_rl & 2 != 0
    );

    let preempt_ch = (1_u32 << 24) | ctx.channel_id;
    let _ = ctx.w(0x2634, preempt_ch);
    std::thread::sleep(std::time::Duration::from_millis(50));

    let post_preempt = ctx.r(pccsr::channel(ctx.channel_id));
    let preempt_rb = ctx.r(0x2634);
    eprintln!("║   O post-preempt(ch): PCCSR={post_preempt:#010x} PREEMPT={preempt_rb:#010x}");

    let preempt_rl = (1_u32 << 20) | (ctx.target_runlist << 16);
    let _ = ctx.w(0x2634, preempt_rl);
    std::thread::sleep(std::time::Duration::from_millis(50));

    let post_rl_preempt = ctx.r(pccsr::channel(ctx.channel_id));
    eprintln!(
        "║   O post-preempt(rl): PCCSR={post_rl_preempt:#010x} PREEMPT={:#010x}",
        ctx.r(0x2634)
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
    eprintln!(
        "║   O final: PCCSR={post_db:#010x} GP_PUT={gp_put} GP_FETCH={gp_fetch} USERD={userd_lo:#010x} GP_BASE={gpbase:#010x} SIG={sig:#010x} STATE={state:#010x}"
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
    eprintln!(
        "║   P phase2: post_bind={post_bind:#010x} post_rl={post_rl:#010x} sched={}",
        post_rl & 2 != 0
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
        eprintln!(
            "║   P pre-db PBDMA{pid}: USERD={userd:#010x} GP_BASE={gpbase:#010x} SIG={sig:#010x} GP_PUT={gp_put} GP_FETCH={gp_fetch} STATE={state:#010x} INTR={intr:#010x}"
        );
    }

    let _ = ctx.w(usermode::NOTIFY_CHANNEL_PENDING, ctx.channel_id);
    std::thread::sleep(std::time::Duration::from_millis(100));

    let post_db = ctx.r(pccsr::channel(ctx.channel_id));
    let pfifo_intr = ctx.r(pfifo::INTR);
    eprintln!("║   P post-doorbell: PCCSR={post_db:#010x} PFIFO_INTR={pfifo_intr:#010x}");

    let mmu_fault_status = ctx.r(0x100E34);
    let mmu_fault_lo = ctx.r(0x100E38);
    let mmu_fault_hi = ctx.r(0x100E3C);
    let mmu_fault_inst_lo = ctx.r(0x100E40);
    let bind_err = ctx.r(0x252C);
    eprintln!(
        "║   P FAULT: MMU_STATUS={mmu_fault_status:#010x} ADDR={mmu_fault_hi:#010x}_{mmu_fault_lo:#010x} INST={mmu_fault_inst_lo:#010x} BIND_ERR={bind_err:#010x}"
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
        eprintln!("║   P PBDMA{pid}: INTR={intr:#010x} HCE_INTR={hce_intr:#010x}");
        eprintln!(
            "║   P PBDMA{pid}: USERD={userd:#010x} GP_BASE={gpbase:#010x} GP_PUT={gp_put} GP_FETCH={gp_fetch} STATE={state:#010x} GP_STATE={gp_state:#010x}"
        );
    }

    let nrfb_get = ctx.r(0x100E4C);
    let nrfb_put = ctx.r(0x100E50);
    let rfb_get = ctx.r(0x100E30);
    let rfb_put = ctx.r(0x100E34);
    eprintln!(
        "║   P FAULTBUF: NR_GET={nrfb_get:#010x} NR_PUT={nrfb_put:#010x} R_GET={rfb_get:#010x} R_PUT={rfb_put:#010x}"
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
    eprintln!(
        "║   P retry: PCCSR={final_pccsr:#010x} PBDMA1_FETCH={final_fetch1} PBDMA2_FETCH={final_fetch2} PFIFO_INTR={final_pfifo_intr:#010x}"
    );
    Ok(())
}
