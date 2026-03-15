// SPDX-License-Identifier: AGPL-3.0-only

use super::super::super::page_tables::write_u32_le;
use super::super::super::registers::*;
use super::super::types::ExperimentOrdering;
use super::context::ExperimentContext;
use crate::error::DriverResult;

/// M: PFIFO engine reset + re-init via PMC toggle.
pub(super) fn pfifo_reset_init(ctx: &mut ExperimentContext<'_>) -> DriverResult<()> {
    let pb = ctx.pb();
    let pmc_cur = ctx.r(pmc::ENABLE);
    if !ctx.gpu_warm {
        eprintln!("║   M: GPU cold, performing PMC reset...");
        let _ = ctx.w(pmc::ENABLE, pmc_cur & !0x100);
        std::thread::sleep(std::time::Duration::from_millis(10));
        let _ = ctx.w(pmc::ENABLE, pmc_cur | 0x100);
        std::thread::sleep(std::time::Duration::from_millis(10));
    } else {
        eprintln!("║   M: GPU warm, skipping PMC toggle to preserve state");
    }

    let pmc_post = ctx.r(pmc::ENABLE);
    let pfifo_post = ctx.r(pfifo::ENABLE);
    let pbdma_post = ctx.r(pfifo::PBDMA_MAP);
    let sched_post = ctx.r(0x2630);
    eprintln!(
        "║   M state: PMC={pmc_post:#010x} PFIFO={pfifo_post:#010x} PBDMA_MAP={pbdma_post:#010x} SCHED_DIS={sched_post:#010x}"
    );

    let _ = ctx.w(pccsr::inst(ctx.channel_id), ctx.pccsr_inst_val);
    std::thread::sleep(std::time::Duration::from_millis(5));
    let _ = ctx.w(pccsr::channel(ctx.channel_id), pccsr::CHANNEL_ENABLE_SET);
    std::thread::sleep(std::time::Duration::from_millis(2));
    ctx.submit_runlist()?;
    std::thread::sleep(std::time::Duration::from_millis(50));

    let post = ctx.r(pccsr::channel(ctx.channel_id));
    let gp_fetch = ctx.r(pb + 0x48);
    let userd_rd = ctx.r(pb + 0xD0);
    eprintln!("║   M result: PCCSR={post:#010x} GP_FETCH={gp_fetch} USERD={userd_rd:#010x}");
    Ok(())
}

/// Z3: No PMC reset — rely on pfifo_init state. Fast microsecond-level polling.
pub(super) fn no_pmc_reset_fast_poll(ctx: &mut ExperimentContext<'_>) -> DriverResult<()> {
    let pb = ctx.pb();
    let _ = ctx.w(pb + pbdma::CTX_USERD_LO, 0xDEAD_0008);
    let _ = ctx.w(pb + pbdma::CTX_SIGNATURE, 0xDEAD_0010);

    let _ = ctx.w(pfifo::INTR, 0xFFFF_FFFF);
    let _ = ctx.w(pfifo::INTR_EN, 0x7FFF_FFFF);
    let intr_en_rb = ctx.r(pfifo::INTR_EN);
    eprintln!("║   Z3: INTR_EN={intr_en_rb:#010x}");

    let _ = ctx.w(pccsr::inst(ctx.channel_id), ctx.pccsr_inst_val);
    std::thread::sleep(std::time::Duration::from_millis(2));
    let _ = ctx.w(pccsr::channel(ctx.channel_id), pccsr::CHANNEL_ENABLE_SET);
    std::thread::sleep(std::time::Duration::from_millis(2));

    let gp_entry: u64 = (NOP_PB_IOVA & 0xFFFF_FFFC) | ((2_u64) << (32 + 10));
    ctx.gpfifo_ring[0..8].copy_from_slice(&gp_entry.to_le_bytes());
    write_u32_le(ctx.userd_page, ramuserd::GP_PUT, 1);
    write_u32_le(ctx.userd_page, ramuserd::GP_GET, 0);
    std::sync::atomic::fence(std::sync::atomic::Ordering::SeqCst);

    let _ = ctx.w(pfifo::INTR, 0xFFFF_FFFF);

    ctx.submit_runlist()?;

    let imm_intr = ctx.r(pfifo::INTR);
    let imm_rb_lo = ctx.r(ctx.rl_base_reg);
    let imm_pccsr = ctx.r(pccsr::channel(ctx.channel_id));
    eprintln!(
        "║   Z3: IMMEDIATE after submit: INTR={imm_intr:#010x} RB_LO={imm_rb_lo:#010x} PCCSR={imm_pccsr:#010x}"
    );

    let mut any_intr = false;
    for i in 0..100 {
        let intr = ctx.r(pfifo::INTR);
        if intr != 0 && !any_intr {
            any_intr = true;
            let pccsr_now = ctx.r(pccsr::channel(ctx.channel_id));
            eprintln!("║   Z3: INTR={intr:#010x} at poll {i} PCCSR={pccsr_now:#010x}");
            if intr & 0x4000_0000 != 0 {
                let ack = ctx.r(pfifo::RUNLIST_ACK);
                eprintln!("║   Z3: BIT30! ACK={ack:#010x}");
                let _ = ctx.w(pfifo::RUNLIST_ACK, 1u32 << ctx.target_runlist);
                let _ = ctx.w(pfifo::INTR, 0x4000_0000);
                break;
            }
        }
        std::thread::sleep(std::time::Duration::from_micros(100));
    }

    let final_pccsr = ctx.r(pccsr::channel(ctx.channel_id));
    let final_ctx_userd = ctx.r(pb + pbdma::CTX_USERD_LO);
    eprintln!(
        "║   Z3: final PCCSR={final_pccsr:#010x} CTX_USERD={final_ctx_userd:#010x} any_intr={any_intr}"
    );

    let _ = ctx.w(usermode::NOTIFY_CHANNEL_PENDING, ctx.channel_id);
    std::thread::sleep(std::time::Duration::from_millis(100));
    Ok(())
}

/// Z/Z2: Full PFIFO nuke-and-pave — complete reinit from scratch.
pub(super) fn full_pfifo_reinit(ctx: &mut ExperimentContext<'_>) -> DriverResult<()> {
    let pb = ctx.pb();
    let is_z2 = matches!(
        ctx.cfg.ordering,
        ExperimentOrdering::FullPfifoReinitDirectPbdma
    );

    let _ = ctx.w(pb + pbdma::CTX_USERD_LO, 0xDEAD_0008);
    let _ = ctx.w(pb + pbdma::CTX_SIGNATURE, 0xDEAD_0010);
    let _ = ctx.w(pb + pbdma::CTX_GP_BASE_LO, 0xDEAD_0048);

    let pmc_cur = ctx.r(pmc::ENABLE);
    let _ = ctx.w(pmc::ENABLE, pmc_cur & !0x100);
    std::thread::sleep(std::time::Duration::from_millis(10));
    let _ = ctx.w(pmc::ENABLE, pmc_cur | 0x100);
    std::thread::sleep(std::time::Duration::from_millis(10));
    let pmc_post = ctx.r(pmc::ENABLE);
    let pfifo_post = ctx.r(pfifo::ENABLE);
    eprintln!("║   Z: PMC={pmc_post:#010x} PFIFO_EN={pfifo_post:#010x}");

    let pbdma_map = ctx.r(pfifo::PBDMA_MAP);
    eprintln!("║   Z: PBDMA_MAP={pbdma_map:#010x}");

    for pid in 0..32_usize {
        if pbdma_map & (1 << pid) == 0 {
            continue;
        }
        let b = 0x0004_0000 + pid * 0x2000;
        let _ = ctx.w(b + 0x0108, 0xFFFF_FFFF);
        let _ = ctx.w(b + 0x010C, 0xFFFF_FEFF);
        let _ = ctx.w(b + 0x013C, 0);
        let _ = ctx.w(b + 0x0148, 0);
        let _ = ctx.w(b + 0x014C, 0);
        let _ = ctx.w(b + 0x0164, 0xFFFF_FFFF);
    }

    let _ = ctx.w(pfifo::INTR, 0xFFFF_FFFF);
    let _ = ctx.w(pfifo::INTR_EN, 0x7FFF_FFFF);
    std::thread::sleep(std::time::Duration::from_millis(2));

    let intr_en_rb = ctx.r(pfifo::INTR_EN);
    let intr_rb = ctx.r(pfifo::INTR);
    eprintln!("║   Z: INTR_EN={intr_en_rb:#010x} INTR={intr_rb:#010x}");

    let rl_id = ctx.target_runlist;
    let _ = ctx.w(
        pfifo::runlist_base(rl_id),
        pfifo::gv100_runlist_base_value(RUNLIST_IOVA),
    );
    let _ = ctx.w(
        pfifo::runlist_submit(rl_id),
        pfifo::gv100_runlist_submit_value(RUNLIST_IOVA, 0),
    );
    std::thread::sleep(std::time::Duration::from_millis(10));
    let flush_intr = ctx.r(pfifo::INTR);
    if flush_intr & 0x4000_0000 != 0 {
        let _ = ctx.r(pfifo::RUNLIST_ACK);
        let _ = ctx.w(pfifo::RUNLIST_ACK, 1u32 << rl_id);
        let _ = ctx.w(pfifo::INTR, 0x4000_0000);
        eprintln!("║   Z: flush rl{rl_id} ACK'd");
    }
    let flush_done_intr = ctx.r(pfifo::INTR);
    eprintln!("║   Z: post-flush INTR={flush_done_intr:#010x}");

    let _ = ctx.w(
        pccsr::inst(ctx.channel_id),
        ctx.pccsr_inst_val | pccsr::INST_BIND_TRUE,
    );
    std::thread::sleep(std::time::Duration::from_millis(10));
    let bind_pccsr = ctx.r(pccsr::channel(ctx.channel_id));
    eprintln!("║   Z: post-INST_BIND PCCSR={bind_pccsr:#010x}");

    let _ = ctx.w(
        pccsr::channel(ctx.channel_id),
        pccsr::PBDMA_FAULTED_RESET | pccsr::ENG_FAULTED_RESET,
    );
    std::thread::sleep(std::time::Duration::from_millis(5));
    let cleared_pccsr = ctx.r(pccsr::channel(ctx.channel_id));
    eprintln!("║   Z: post-clear PCCSR={cleared_pccsr:#010x}");

    let _ = ctx.w(pccsr::channel(ctx.channel_id), pccsr::CHANNEL_ENABLE_SET);
    std::thread::sleep(std::time::Duration::from_millis(5));
    let enabled_pccsr = ctx.r(pccsr::channel(ctx.channel_id));
    eprintln!("║   Z: post-enable PCCSR={enabled_pccsr:#010x}");

    let gp_entry: u64 = (NOP_PB_IOVA & 0xFFFF_FFFC) | ((2_u64) << (32 + 10));
    ctx.gpfifo_ring[0..8].copy_from_slice(&gp_entry.to_le_bytes());
    write_u32_le(ctx.userd_page, ramuserd::GP_PUT, 1);
    write_u32_le(ctx.userd_page, ramuserd::GP_GET, 0);
    std::sync::atomic::fence(std::sync::atomic::Ordering::SeqCst);

    if is_z2 {
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
        let _ = ctx.w(pb + 0x48, 0);
        let _ = ctx.w(pb + 0x54, 1);
        eprintln!("║   Z2: direct PBDMA written");
    }

    let _ = ctx.w(pfifo::INTR, 0xFFFF_FFFF);
    ctx.submit_runlist()?;

    let rb_lo = ctx.r(ctx.rl_base_reg);
    let rb_sub = ctx.r(ctx.rl_submit_reg);
    eprintln!(
        "║   Z: RL submit: wrote BASE={:#010x} SUB={:#010x} → rb BASE={rb_lo:#010x} SUB={rb_sub:#010x}",
        ctx.rl_base, ctx.rl_submit
    );

    let mut rl_completed = false;
    for poll in 0..40 {
        std::thread::sleep(std::time::Duration::from_millis(5));
        let intr = ctx.r(pfifo::INTR);
        // Clear bit 8 (GV100 post-submit interrupt) so scheduler doesn't stall.
        if intr & pfifo::INTR_BIT8 != 0 {
            let _ = ctx.w(pfifo::INTR, pfifo::INTR_BIT8);
            if poll < 5 {
                eprintln!("║   Z: cleared INTR bit8 at poll {poll}");
            }
        }
        if intr & pfifo::INTR_RL_COMPLETE != 0 {
            rl_completed = true;
            let ack_val = ctx.r(pfifo::RUNLIST_ACK);
            eprintln!("║   Z: BIT30 SET at poll {poll}: INTR={intr:#010x} ACK={ack_val:#010x}");
            let _ = ctx.w(pfifo::RUNLIST_ACK, 1u32 << ctx.target_runlist);
            let _ = ctx.w(pfifo::INTR, pfifo::INTR_RL_COMPLETE);
            std::thread::sleep(std::time::Duration::from_millis(5));
            break;
        }
        if intr != 0 && poll < 5 {
            eprintln!("║   Z: poll {poll}: INTR={intr:#010x} (not bit30)");
        }
        if poll == 39 {
            eprintln!("║   Z: BIT30 NEVER SET — final INTR={intr:#010x}");
            let sched_dis = ctx.r(pfifo::SCHED_DISABLE);
            let pfifo_en = ctx.r(pfifo::ENABLE);
            let intr_en = ctx.r(pfifo::INTR_EN);
            let engn0 = ctx.r(0x2640);
            let bind_err = ctx.r(0x252C);
            eprintln!(
                "║   Z: SCHED_DIS={sched_dis:#010x} PFIFO_EN={pfifo_en:#010x} INTR_EN={intr_en:#010x} ENGN0={engn0:#010x}"
            );
            eprintln!("║   Z: BIND_ERR={bind_err:#010x}");
            for pid in [1_usize, 2, 3] {
                if pbdma_map & (1 << pid) == 0 {
                    continue;
                }
                let b = 0x40000 + pid * 0x2000;
                let p_intr = ctx.r(b + 0x0108);
                let p_state = ctx.r(b + 0x00B0);
                let p_method = ctx.r(b + 0x01C0);
                let p_userd = ctx.r(b + 0x00D0);
                let p_gpb = ctx.r(b + 0x0040);
                eprintln!(
                    "║   Z: PBDMA{pid} INTR={p_intr:#010x} STATE={p_state:#010x} METHOD={p_method:#010x} USERD={p_userd:#010x} GPB={p_gpb:#010x}"
                );
            }
            let nrfb_get = ctx.r(0x100E4C);
            let nrfb_put = ctx.r(0x100E50);
            eprintln!("║   Z: FAULTBUF NR_GET={nrfb_get:#010x} NR_PUT={nrfb_put:#010x}");
        }
    }

    let pccsr_post = ctx.r(pccsr::channel(ctx.channel_id));
    let ctx_userd = ctx.r(pb + pbdma::CTX_USERD_LO);
    let ctx_sig = ctx.r(pb + pbdma::CTX_SIGNATURE);
    let sentinel_changed = ctx_userd != 0xDEAD_0008 || ctx_sig != 0xDEAD_0010;
    eprintln!(
        "║   Z: post-ack PCCSR={pccsr_post:#010x} sched={} loaded={sentinel_changed} rl_done={rl_completed}",
        pccsr_post & 2 != 0
    );

    let _ = ctx.w(usermode::NOTIFY_CHANNEL_PENDING, ctx.channel_id);
    std::thread::sleep(std::time::Duration::from_millis(100));

    let final_pccsr = ctx.r(pccsr::channel(ctx.channel_id));
    let final_gp_put = ctx.r(pb + pbdma::GP_PUT);
    let final_gp_fetch = ctx.r(pb + pbdma::GP_FETCH);
    let final_intr = ctx.r(pbdma::intr(ctx.target_pbdma));
    let final_ctx_userd = ctx.r(pb + pbdma::CTX_USERD_LO);
    let pfifo_final = ctx.r(pfifo::INTR);
    eprintln!(
        "║   Z: final PCCSR={final_pccsr:#010x} GP_PUT={final_gp_put} FETCH={final_gp_fetch} INTR={final_intr:#010x}"
    );
    eprintln!("║   Z: CTX_USERD={final_ctx_userd:#010x} PFIFO={pfifo_final:#010x}");
    Ok(())
}
