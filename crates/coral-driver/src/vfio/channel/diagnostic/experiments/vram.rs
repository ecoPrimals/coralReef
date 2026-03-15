// SPDX-License-Identifier: AGPL-3.0-only

use super::super::super::page_tables::{populate_runlist_static, write_u32_le};
use super::super::super::registers::*;
use super::context::ExperimentContext;
use crate::error::DriverResult;

const PRAMIN_BASE: usize = 0x0070_0000;
const BAR0_WINDOW: usize = 0x0000_1700;
const VRAM_INST_OFF: usize = 0x3000;

/// J: Instance block written to VRAM via PRAMIN, normal runlist submit + doorbell.
pub(super) fn vram_instance_bind(ctx: &mut ExperimentContext<'_>) -> DriverResult<()> {
    let pb = ctx.pb();
    let _ = ctx.w(BAR0_WINDOW, 0);
    std::thread::sleep(std::time::Duration::from_millis(1));

    let inst_bytes = ctx.instance.as_slice();
    for off in (0..inst_bytes.len()).step_by(4) {
        let val = u32::from_le_bytes([
            inst_bytes[off],
            inst_bytes[off + 1],
            inst_bytes[off + 2],
            inst_bytes[off + 3],
        ]);
        let _ = ctx.w(PRAMIN_BASE + VRAM_INST_OFF + off, val);
    }

    let vram_sig = ctx.r(PRAMIN_BASE + VRAM_INST_OFF + ramfc::SIGNATURE);
    let vram_gpb = ctx.r(PRAMIN_BASE + VRAM_INST_OFF + ramfc::GP_BASE_LO);
    eprintln!("║   VRAM verify: SIG={vram_sig:#010x} GP_BASE={vram_gpb:#010x}");

    let _ = ctx.w(pb + 0x40, 0xBEEF_0040);
    let _ = ctx.w(pb + 0x44, 0xBEEF_0044);
    let _ = ctx.w(pb + 0x48, 0);
    let _ = ctx.w(pb + 0x4C, 0);
    let _ = ctx.w(pb + 0x54, 0);
    let _ = ctx.w(pb + 0xD0, 0xBEEF_00D0);
    let _ = ctx.w(pb + 0xD4, 0xBEEF_00D4);
    let _ = ctx.w(pb + 0xC0, 0xBEEF_00C0);
    let _ = ctx.w(pb + 0xAC, 0xBEEF_00AC);

    ctx.runlist.as_mut_slice().fill(0);
    populate_runlist_static(
        ctx.runlist.as_mut_slice(),
        ctx.userd_iova,
        ctx.channel_id,
        ctx.cfg.runlist_userd_target,
        0,
        0,
    );

    let _ = ctx.w(0x70010, 0x0000_0001);
    for _ in 0..2000_u32 {
        if ctx.r(0x70010) & 3 == 0 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_micros(100));
    }

    let vram_pccsr = VRAM_INST_OFF as u32 >> 12;
    let _ = ctx.w(pccsr::inst(ctx.channel_id), vram_pccsr);
    std::thread::sleep(std::time::Duration::from_millis(5));

    let post_inst = ctx.r(pccsr::channel(ctx.channel_id));
    eprintln!("║   post-INST(noBind): {post_inst:#010x}");

    let _ = ctx.w(pccsr::channel(ctx.channel_id), pccsr::CHANNEL_ENABLE_SET);
    std::thread::sleep(std::time::Duration::from_millis(5));

    ctx.submit_runlist()?;
    std::thread::sleep(std::time::Duration::from_millis(50));

    let sched_gpb = ctx.r(pb + 0x40);
    let sched_userd = ctx.r(pb + 0xD0);
    let sched_sig = ctx.r(pb + 0xC0);
    let sched_state = ctx.r(pb + 0xB0);
    eprintln!(
        "║   post-sched PBDMA: GP_BASE={sched_gpb:#010x} USERD={sched_userd:#010x} SIG={sched_sig:#010x} STATE={sched_state:#010x}"
    );

    let gp_entry: u64 = (NOP_PB_IOVA & 0xFFFF_FFFC) | ((2_u64) << (32 + 10));
    ctx.gpfifo_ring[0..8].copy_from_slice(&gp_entry.to_le_bytes());
    write_u32_le(ctx.userd_page, ramuserd::GP_PUT, 1);
    write_u32_le(ctx.userd_page, ramuserd::GP_GET, 0);
    std::sync::atomic::fence(std::sync::atomic::Ordering::SeqCst);

    let _ = ctx.w(usermode::NOTIFY_CHANNEL_PENDING, ctx.channel_id);
    Ok(())
}

fn wv(ctx: &ExperimentContext<'_>, pm: usize, off: usize, val: u32) -> DriverResult<()> {
    ctx.w(pm + off, val)
}

/// K: ALL structures in VRAM via PRAMIN.
pub(super) fn all_vram(ctx: &mut ExperimentContext<'_>) -> DriverResult<()> {
    let pb = ctx.pb();
    const PM: usize = 0x0070_0000;
    const BW: usize = 0x0000_1700;
    let _ = ctx.w(BW, 0);
    std::thread::sleep(std::time::Duration::from_millis(1));

    for off in (0..0xD000_usize).step_by(4) {
        let _ = wv(ctx, PM, off, 0);
    }

    let vram_pde = |addr: u64| -> u64 { (addr >> 4) | 1 };
    let vram_pte = |addr: u64| -> u64 { (addr >> 4) | 1 };

    let e = vram_pde(0x1000);
    let _ = wv(ctx, PM, 0x0000, e as u32);
    let _ = wv(ctx, PM, 0x0004, (e >> 32) as u32);
    let e = vram_pde(0x2000);
    let _ = wv(ctx, PM, 0x1000, e as u32);
    let _ = wv(ctx, PM, 0x1004, (e >> 32) as u32);
    let e = vram_pde(0x3000);
    let _ = wv(ctx, PM, 0x2000, e as u32);
    let _ = wv(ctx, PM, 0x2004, (e >> 32) as u32);
    let e = vram_pde(0x4000);
    let _ = wv(ctx, PM, 0x3008, e as u32);
    let _ = wv(ctx, PM, 0x300C, (e >> 32) as u32);
    for page in 0..13_usize {
        let phys = (page as u64) * 4096;
        let e = vram_pte(phys);
        let off = 0x4000 + page * 8;
        let _ = wv(ctx, PM, off, e as u32);
        let _ = wv(ctx, PM, off + 4, (e >> 32) as u32);
    }

    let inst_base = 0x8000_usize;
    let pdb_lo: u32 = ((0_u64 >> 12) as u32) << 12 | (1 << 11) | (1 << 10) | (1 << 2) | 1;
    let _ = wv(ctx, PM, inst_base + ramin::PAGE_DIR_BASE_LO, pdb_lo);
    let _ = wv(ctx, PM, inst_base + ramin::PAGE_DIR_BASE_HI, 0);
    let _ = wv(ctx, PM, inst_base + ramin::ENGINE_WFI_VEID, 0);
    let _ = wv(ctx, PM, inst_base + ramin::SC_PDB_VALID, 1);
    let _ = wv(ctx, PM, inst_base + ramin::SC0_PAGE_DIR_BASE_LO, pdb_lo);
    let _ = wv(ctx, PM, inst_base + ramin::SC0_PAGE_DIR_BASE_HI, 0);

    let gpfifo_vram: u64 = 0x9000;
    let userd_vram: u64 = 0xA000;
    let _ = wv(ctx, PM, inst_base + ramfc::GP_BASE_LO, gpfifo_vram as u32);
    let _ = wv(
        ctx,
        PM,
        inst_base + ramfc::GP_BASE_HI,
        (gpfifo_vram >> 32) as u32 | (ctx.limit2 << 16),
    );
    let _ = wv(
        ctx,
        PM,
        inst_base + ramfc::USERD_LO,
        userd_vram as u32 & 0xFFFF_FE00,
    );
    let _ = wv(ctx, PM, inst_base + ramfc::USERD_HI, 0);
    let _ = wv(ctx, PM, inst_base + ramfc::SIGNATURE, 0x0000_FACE);
    let _ = wv(ctx, PM, inst_base + ramfc::ACQUIRE, 0x7FFF_F902);
    let _ = wv(ctx, PM, inst_base + ramfc::PB_HEADER, 0x2040_0000);
    let _ = wv(ctx, PM, inst_base + ramfc::SUBDEVICE, 0x3000_0000 | 0xFFF);
    let _ = wv(ctx, PM, inst_base + ramfc::HCE_CTRL, 0x0000_0020);
    let _ = wv(ctx, PM, inst_base + ramfc::CHID, ctx.channel_id);
    let _ = wv(ctx, PM, inst_base + ramfc::CONFIG, 0x0000_1100);
    let _ = wv(ctx, PM, inst_base + ramfc::CHANNEL_INFO, 0x1000_3080);

    let v_sig = ctx.r(PM + inst_base + ramfc::SIGNATURE);
    let v_gpb = ctx.r(PM + inst_base + ramfc::GP_BASE_LO);
    let v_pdb = ctx.r(PM + inst_base + ramin::PAGE_DIR_BASE_LO);
    let v_usr = ctx.r(PM + inst_base + ramfc::USERD_LO);
    eprintln!(
        "║   VRAM inst: SIG={v_sig:#010x} GP={v_gpb:#010x} PDB={v_pdb:#010x} USR={v_usr:#010x}"
    );

    let gp_entry: u64 = (0xB000_u64 & 0xFFFF_FFFC) | ((1_u64) << (32 + 10));
    let _ = wv(ctx, PM, 0x9000, gp_entry as u32);
    let _ = wv(ctx, PM, 0x9004, (gp_entry >> 32) as u32);

    let _ = wv(ctx, PM, 0xA000 + ramuserd::GP_PUT, 1);
    let _ = wv(ctx, PM, 0xA000 + ramuserd::GP_GET, 0);

    let _ = wv(ctx, PM, 0xC000, (128 << 24) | (3 << 16) | 1);
    let _ = wv(ctx, PM, 0xC004, 1);
    let _ = wv(ctx, PM, 0xC008, 0);
    let _ = wv(ctx, PM, 0xC00C, 0);
    let chan_dw0 = userd_vram as u32 & 0xFFFF_FF00;
    let _ = wv(ctx, PM, 0xC010, chan_dw0);
    let _ = wv(ctx, PM, 0xC014, 0);
    let chan_dw2 = (0x8000_u32 & 0xFFFF_F000) | ctx.channel_id;
    let _ = wv(ctx, PM, 0xC018, chan_dw2);
    let _ = wv(ctx, PM, 0xC01C, 0);

    let _ = ctx.w(pb + 0x40, 0xBEEF_0040);
    let _ = ctx.w(pb + 0x44, 0xBEEF_0044);
    let _ = ctx.w(pb + 0x48, 0);
    let _ = ctx.w(pb + 0x54, 0);
    let _ = ctx.w(pb + 0xD0, 0xBEEF_00D0);
    let _ = ctx.w(pb + 0xD4, 0xBEEF_00D4);
    let _ = ctx.w(pb + 0xC0, 0xBEEF_00C0);

    let _ = ctx.w(0x70010, 0x0000_0001);
    for _ in 0..2000_u32 {
        if ctx.r(0x70010) & 3 == 0 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_micros(100));
    }

    let vram_inst = (0x8000_u32 >> 12) | pccsr::INST_BIND_TRUE;
    let _ = ctx.w(pccsr::inst(ctx.channel_id), vram_inst);
    std::thread::sleep(std::time::Duration::from_millis(10));

    let post_bind = ctx.r(pccsr::channel(ctx.channel_id));
    eprintln!("║   post-BIND(allVram): {post_bind:#010x}");

    if post_bind & (pccsr::PBDMA_FAULTED_RESET | pccsr::ENG_FAULTED_RESET) != 0 {
        let _ = ctx.w(
            pccsr::channel(ctx.channel_id),
            pccsr::PBDMA_FAULTED_RESET | pccsr::ENG_FAULTED_RESET,
        );
        std::thread::sleep(std::time::Duration::from_millis(5));
    }

    let _ = ctx.w(pccsr::channel(ctx.channel_id), pccsr::CHANNEL_ENABLE_SET);
    std::thread::sleep(std::time::Duration::from_millis(5));

    let _ = ctx.w(
        pfifo::runlist_base(ctx.target_runlist),
        pfifo::gv100_runlist_base_value(0xC000),
    );
    let _ = ctx.w(
        pfifo::runlist_submit(ctx.target_runlist),
        pfifo::gv100_runlist_submit_value(0xC000, 2),
    );
    std::thread::sleep(std::time::Duration::from_millis(50));

    let post_rl = ctx.r(pccsr::channel(ctx.channel_id));
    let eng_rl_base = ctx.r(0x2288);
    let eng_rl = ctx.r(0x228C);
    let sched_gpb = ctx.r(pb + 0x40);
    let sched_usr = ctx.r(pb + 0xD0);
    let sched_sig = ctx.r(pb + 0xC0);
    let sched_state = ctx.r(pb + 0xB0);
    eprintln!(
        "║   post-submit: PCCSR={post_rl:#010x} ENG_RL_BASE={eng_rl_base:#010x} ENG_RL={eng_rl:#010x}"
    );
    eprintln!(
        "║   PBDMA: GP_BASE={sched_gpb:#010x} USERD={sched_usr:#010x} SIG={sched_sig:#010x} STATE={sched_state:#010x}"
    );

    let _ = ctx.w(usermode::NOTIFY_CHANNEL_PENDING, ctx.channel_id);
    std::thread::sleep(std::time::Duration::from_millis(50));

    let post_db = ctx.r(pccsr::channel(ctx.channel_id));
    let gpb_post = ctx.r(pb + 0x40);
    let usr_post = ctx.r(pb + 0xD0);
    let sig_post = ctx.r(pb + 0xC0);
    let gp_put_post = ctx.r(pb + 0x54);
    let gp_fetch_post = ctx.r(pb + 0x48);
    eprintln!(
        "║   post-doorbell: PCCSR={post_db:#010x} GP_BASE={gpb_post:#010x} USERD={usr_post:#010x} SIG={sig_post:#010x} GP_PUT={gp_put_post} GP_FETCH={gp_fetch_post}"
    );
    Ok(())
}

/// L: Hybrid — VRAM structures + direct PBDMA programming.
pub(super) fn all_vram_direct_pbdma(ctx: &mut ExperimentContext<'_>) -> DriverResult<()> {
    let pb = ctx.pb();
    const PM: usize = 0x0070_0000;
    const BW: usize = 0x0000_1700;
    let _ = ctx.w(BW, 0);
    std::thread::sleep(std::time::Duration::from_millis(1));

    for off in (0..0xD000_usize).step_by(4) {
        let _ = wv(ctx, PM, off, 0);
    }

    let vram_pde = |addr: u64| -> u64 { (addr >> 4) | 1 };
    let vram_pte = |addr: u64| -> u64 { (addr >> 4) | 1 };

    let e = vram_pde(0x1000);
    let _ = wv(ctx, PM, 0x0000, e as u32);
    let _ = wv(ctx, PM, 0x0004, (e >> 32) as u32);
    let e = vram_pde(0x2000);
    let _ = wv(ctx, PM, 0x1000, e as u32);
    let _ = wv(ctx, PM, 0x1004, (e >> 32) as u32);
    let e = vram_pde(0x3000);
    let _ = wv(ctx, PM, 0x2000, e as u32);
    let _ = wv(ctx, PM, 0x2004, (e >> 32) as u32);
    let e = vram_pde(0x4000);
    let _ = wv(ctx, PM, 0x3008, e as u32);
    let _ = wv(ctx, PM, 0x300C, (e >> 32) as u32);
    for page in 0..13_usize {
        let e = vram_pte((page as u64) * 4096);
        let off = 0x4000 + page * 8;
        let _ = wv(ctx, PM, off, e as u32);
        let _ = wv(ctx, PM, off + 4, (e >> 32) as u32);
    }

    let ib = 0x8000_usize;
    let pdb_lo: u32 = (1 << 11) | (1 << 10) | (1 << 2) | 1;
    let _ = wv(ctx, PM, ib + ramin::PAGE_DIR_BASE_LO, pdb_lo);
    let _ = wv(ctx, PM, ib + ramin::PAGE_DIR_BASE_HI, 0);
    let _ = wv(ctx, PM, ib + ramin::ENGINE_WFI_VEID, 0);
    let _ = wv(ctx, PM, ib + ramin::SC_PDB_VALID, 1);
    let _ = wv(ctx, PM, ib + ramin::SC0_PAGE_DIR_BASE_LO, pdb_lo);
    let _ = wv(ctx, PM, ib + ramin::SC0_PAGE_DIR_BASE_HI, 0);
    let gpfifo_vram: u64 = 0x9000;
    let userd_vram: u64 = 0xA000;
    let _ = wv(ctx, PM, ib + ramfc::GP_BASE_LO, gpfifo_vram as u32);
    let _ = wv(ctx, PM, ib + ramfc::GP_BASE_HI, ctx.limit2 << 16);
    let _ = wv(
        ctx,
        PM,
        ib + ramfc::USERD_LO,
        userd_vram as u32 & 0xFFFF_FE00,
    );
    let _ = wv(ctx, PM, ib + ramfc::USERD_HI, 0);
    let _ = wv(ctx, PM, ib + ramfc::SIGNATURE, 0x0000_FACE);
    let _ = wv(ctx, PM, ib + ramfc::ACQUIRE, 0x7FFF_F902);
    let _ = wv(ctx, PM, ib + ramfc::PB_HEADER, 0x2040_0000);
    let _ = wv(ctx, PM, ib + ramfc::SUBDEVICE, 0x3000_0FFF);
    let _ = wv(ctx, PM, ib + ramfc::HCE_CTRL, 0x0000_0020);
    let _ = wv(ctx, PM, ib + ramfc::CHID, ctx.channel_id);
    let _ = wv(ctx, PM, ib + ramfc::CONFIG, 0x0000_1100);
    let _ = wv(ctx, PM, ib + ramfc::CHANNEL_INFO, 0x1000_3080);

    let gp_entry: u64 = (0xB000_u64 & 0xFFFF_FFFC) | ((1_u64) << (32 + 10));
    let _ = wv(ctx, PM, 0x9000, gp_entry as u32);
    let _ = wv(ctx, PM, 0x9004, (gp_entry >> 32) as u32);

    let _ = wv(ctx, PM, 0xA000 + ramuserd::GP_PUT, 1);
    let _ = wv(ctx, PM, 0xA000 + ramuserd::GP_GET, 0);

    let _ = wv(ctx, PM, 0xC000, (128 << 24) | (3 << 16) | 1);
    let _ = wv(ctx, PM, 0xC004, 1);
    let _ = wv(ctx, PM, 0xC008, 0);
    let _ = wv(ctx, PM, 0xC00C, 0);
    let _ = wv(ctx, PM, 0xC010, (userd_vram as u32) & 0xFFFF_FF00);
    let _ = wv(ctx, PM, 0xC014, 0);
    let _ = wv(ctx, PM, 0xC018, (0x8000_u32 & 0xFFFF_F000) | ctx.channel_id);
    let _ = wv(ctx, PM, 0xC01C, 0);

    let _ = ctx.w(0x70010, 1);
    for _ in 0..2000_u32 {
        if ctx.r(0x70010) & 3 == 0 {
            break;
        }
        std::thread::sleep(std::time::Duration::from_micros(100));
    }

    let _ = ctx.w(pccsr::inst(ctx.channel_id), 0x8000_u32 >> 12);
    std::thread::sleep(std::time::Duration::from_millis(5));

    let _ = ctx.w(pccsr::channel(ctx.channel_id), pccsr::CHANNEL_ENABLE_SET);
    std::thread::sleep(std::time::Duration::from_millis(5));

    let _ = ctx.w(pb + 0x40, gpfifo_vram as u32);
    let _ = ctx.w(pb + 0x44, ctx.limit2 << 16);
    let _ = ctx.w(pb + 0x48, 0);
    let _ = ctx.w(pb + 0x4C, 0);
    let _ = ctx.w(pb + 0xD0, userd_vram as u32 & 0xFFFF_FE00);
    let _ = ctx.w(pb + 0xD4, 0);
    let _ = ctx.w(pb + 0xC0, 0x0000_FACE);
    let _ = ctx.w(pb + 0xAC, 0x1000_3080);
    let _ = ctx.w(pb + 0xA8, 0x0000_1100);

    let _ = ctx.w(
        pfifo::runlist_base(ctx.target_runlist),
        pfifo::gv100_runlist_base_value(0xC000),
    );
    let _ = ctx.w(
        pfifo::runlist_submit(ctx.target_runlist),
        pfifo::gv100_runlist_submit_value(0xC000, 2),
    );
    std::thread::sleep(std::time::Duration::from_millis(20));

    let _ = ctx.w(pb + 0x54, 1);
    std::thread::sleep(std::time::Duration::from_millis(5));
    let _ = ctx.w(usermode::NOTIFY_CHANNEL_PENDING, ctx.channel_id);
    std::thread::sleep(std::time::Duration::from_millis(50));

    let post = ctx.r(pccsr::channel(ctx.channel_id));
    let gp_put = ctx.r(pb + 0x54);
    let gp_fetch = ctx.r(pb + 0x48);
    let userd_lo = ctx.r(pb + 0xD0);
    let sig = ctx.r(pb + 0xC0);
    let state = ctx.r(pb + 0xB0);
    eprintln!(
        "║   L result: PCCSR={post:#010x} GP_PUT={gp_put} GP_FETCH={gp_fetch} USERD={userd_lo:#010x} SIG={sig:#010x} STATE={state:#010x}"
    );
    Ok(())
}

/// Q: Instance block in VRAM (via PRAMIN) + full dispatch path.
pub(super) fn vram_full_dispatch(ctx: &mut ExperimentContext<'_>) -> DriverResult<()> {
    const PM: usize = 0x0070_0000;
    const BW: usize = 0x0000_1700;
    const VI: usize = 0x3000;
    const VRAM_USERD: usize = 0x0000;
    const PB2: usize = 0x44000;

    let _ = ctx.w(BW, 0);
    std::thread::sleep(std::time::Duration::from_millis(1));

    let inst_bytes = ctx.instance.as_slice();
    for off in (0..inst_bytes.len()).step_by(4) {
        let val = u32::from_le_bytes([
            inst_bytes[off],
            inst_bytes[off + 1],
            inst_bytes[off + 2],
            inst_bytes[off + 3],
        ]);
        if val != 0 {
            let _ = ctx.w(PM + VI + off, val);
        }
    }

    let _ = ctx.w(PM + VI + 0x0AC, 0x0001_0000);
    let _ = ctx.w(PM + VI + 0x210, (PT0_IOVA as u32) | 4);
    let _ = ctx.w(PM + VI + 0x214, (PT0_IOVA >> 32) as u32);

    let v_sig = ctx.r(PM + VI + ramfc::SIGNATURE);
    let v_gpb = ctx.r(PM + VI + ramfc::GP_BASE_LO);
    let v_usr = ctx.r(PM + VI + ramfc::USERD_LO);
    let v_pdb = ctx.r(PM + VI + ramin::PAGE_DIR_BASE_LO);
    eprintln!(
        "║   Q VRAM inst: SIG={v_sig:#010x} GP_BASE={v_gpb:#010x} USERD={v_usr:#010x} PDB={v_pdb:#010x}"
    );

    let _ = ctx.w(PM + VRAM_USERD + ramuserd::GP_PUT, 1);
    let _ = ctx.w(PM + VRAM_USERD + ramuserd::GP_GET, 0);
    let vram_gp_put = ctx.r(PM + VRAM_USERD + ramuserd::GP_PUT);
    eprintln!(
        "║   Q VRAM USERD: GP_PUT={vram_gp_put} (at 0x{:04x})",
        VRAM_USERD + ramuserd::GP_PUT
    );

    let _ = ctx.w(PM + VI + ramfc::USERD_LO, VRAM_USERD as u32 & 0xFFFF_FE00);
    let _ = ctx.w(PM + VI + ramfc::USERD_HI, 0);

    let gp_entry: u64 = (NOP_PB_IOVA & 0xFFFF_FFFC) | ((2_u64) << (32 + 10));
    ctx.gpfifo_ring[0..8].copy_from_slice(&gp_entry.to_le_bytes());
    write_u32_le(ctx.userd_page, ramuserd::GP_PUT, 1);
    write_u32_le(ctx.userd_page, ramuserd::GP_GET, 0);
    std::sync::atomic::fence(std::sync::atomic::Ordering::SeqCst);

    let _ = ctx.w(0x002638, 1 << ctx.target_runlist);
    std::thread::sleep(std::time::Duration::from_millis(50));
    let preempt_pending = ctx.r(0x002284 + (ctx.target_runlist as usize) * 8);
    eprintln!("║   Q pre-preempt: pending={preempt_pending:#010x}");

    eprintln!(
        "║   Q pccsr_inst_val={:#010x} rl_base={:#010x} rl_submit={:#010x}",
        ctx.pccsr_inst_val, ctx.rl_base, ctx.rl_submit
    );
    let _ = ctx.w(pccsr::inst(ctx.channel_id), ctx.pccsr_inst_val);
    std::thread::sleep(std::time::Duration::from_millis(5));
    let post_bind = ctx.r(pccsr::channel(ctx.channel_id));
    let _ = ctx.w(pccsr::channel(ctx.channel_id), pccsr::CHANNEL_ENABLE_SET);
    std::thread::sleep(std::time::Duration::from_millis(2));
    ctx.submit_runlist()?;
    std::thread::sleep(std::time::Duration::from_millis(50));

    let rl_pending = ctx.r(0x002284 + (ctx.target_runlist as usize) * 8);
    eprintln!(
        "║   Q runlist_pending={rl_pending:#010x} (bit20={})",
        rl_pending & 0x00100000 != 0
    );

    std::thread::sleep(std::time::Duration::from_millis(100));
    let post_rl = ctx.r(pccsr::channel(ctx.channel_id));
    eprintln!(
        "║   Q bind={post_bind:#010x} sched={post_rl:#010x} ok={}",
        post_rl & 2 != 0
    );

    let pfifo_intr = ctx.r(pfifo::INTR);
    let rl_mask = ctx.r(0x002A00);
    eprintln!("║   Q ack: PFIFO_INTR={pfifo_intr:#010x} RL_MASK={rl_mask:#010x}");
    if rl_mask != 0 {
        let _ = ctx.w(0x002A00, rl_mask);
    }
    let _ = ctx.w(pfifo::INTR, 0xFFFF_FFFF);
    let _ = ctx.w(pbdma::intr(1), 0xFFFF_FFFF);
    let _ = ctx.w(pbdma::intr(2), 0xFFFF_FFFF);
    let _ = ctx.w(
        pccsr::channel(ctx.channel_id),
        pccsr::PBDMA_FAULTED_RESET | pccsr::ENG_FAULTED_RESET,
    );
    std::thread::sleep(std::time::Duration::from_millis(20));

    let p2_sig = ctx.r(PB2 + 0x010);
    let p2_userd = ctx.r(PB2 + 0x008);
    let p2_gpbase = ctx.r(PB2 + 0x048);
    let p2_chid = ctx.r(PB2 + 0x0E8);
    eprintln!(
        "║   Q ctx: SIG={p2_sig:#010x} USERD={p2_userd:#010x} GP_BASE={p2_gpbase:#010x} CHID={p2_chid:#010x}"
    );

    let _ = ctx.w(usermode::NOTIFY_CHANNEL_PENDING, ctx.channel_id);
    std::thread::sleep(std::time::Duration::from_millis(100));

    eprint!("║   Q pb2-post-db:");
    for off in (0x000..=0x0FF_usize).step_by(4) {
        let val = ctx.r(PB2 + off);
        if val != 0 {
            eprint!(" [{off:#05x}]={val:#010x}");
        }
    }
    eprintln!();

    let pccsr_post = ctx.r(pccsr::channel(ctx.channel_id));
    let intr_post = ctx.r(pfifo::INTR);
    let p2_intr = ctx.r(pbdma::intr(2));
    let p2_hce = ctx.r(pbdma::hce_intr(2));
    let p2_method = ctx.r(PB2 + pbdma::METHOD0);
    let p2_data = ctx.r(PB2 + pbdma::DATA0);
    eprintln!(
        "║   Q post-db: PCCSR={pccsr_post:#010x} PFIFO_INTR={intr_post:#010x} PBDMA2_INTR={p2_intr:#010x} HCE={p2_hce:#010x}"
    );
    if p2_intr != 0 {
        eprintln!(
            "║   Q PBDMA2 fault: METHOD={p2_method:#010x} DATA={p2_data:#010x} (method_addr={:#06x})",
            (p2_method & 0xFFF) << 2
        );
    }

    if p2_intr != 0 || p2_hce != 0 {
        let _ = ctx.w(pbdma::intr(2), 0xFFFF_FFFF);
        let _ = ctx.w(pbdma::hce_intr(2), 0xFFFF_FFFF);
        let _ = ctx.w(pfifo::INTR, 0xFFFF_FFFF);
        let _ = ctx.w(
            pccsr::channel(ctx.channel_id),
            pccsr::PBDMA_FAULTED_RESET | pccsr::ENG_FAULTED_RESET,
        );
        std::thread::sleep(std::time::Duration::from_millis(20));

        let _ = ctx.w(usermode::NOTIFY_CHANNEL_PENDING, ctx.channel_id);
        std::thread::sleep(std::time::Duration::from_millis(200));

        eprint!("║   Q pb2-retry:");
        for off in (0x000..=0x0FF_usize).step_by(4) {
            let val = ctx.r(PB2 + off);
            if val != 0 {
                eprint!(" [{off:#05x}]={val:#010x}");
            }
        }
        eprintln!();

        let retry_intr = ctx.r(pbdma::intr(2));
        let retry_hce = ctx.r(pbdma::hce_intr(2));
        let retry_pccsr = ctx.r(pccsr::channel(ctx.channel_id));
        eprintln!(
            "║   Q retry: PCCSR={retry_pccsr:#010x} PBDMA2_INTR={retry_intr:#010x} HCE={retry_hce:#010x}"
        );
    }

    let final_pccsr = ctx.r(pccsr::channel(ctx.channel_id));
    let final_intr = ctx.r(pfifo::INTR);
    eprintln!("║   Q final: PCCSR={final_pccsr:#010x} PFIFO_INTR={final_intr:#010x}");
    Ok(())
}
