// SPDX-License-Identifier: AGPL-3.0-or-later
//! GK110 PGRAPH MMIO initialization table — derived from nouveau's register lists.
//!
//! Nouveau applies these register writes during `gf100_gr_init()` BEFORE
//! uploading and booting FECS/GPCCS firmware. Without them, the GR engine
//! is in an unconfigured state and the falcons halt on boot.
//!
//! Source: `gk110_gr_pack_mmio[]` from `nouveau/nvkm/engine/gr/gk110.c`,
//! expanded from `{ base, count, stride, value }` format into flat pairs.

/// Full GK110 PGRAPH register init table.
///
/// Order follows `gk110_gr_pack_mmio[]` exactly:
/// 1. gk104_gr_init_main_0    — PGRAPH main control
/// 2. gk110_gr_init_fe_0      — Frontend engine
/// 3. gf100_gr_init_pri_0     — PRI interface
/// 4. gf100_gr_init_rstr2d_0  — Rasterizer 2D
/// 5. gf119_gr_init_pd_0      — Primitive distributor
/// 6. gk110_gr_init_ds_0      — Datastreamer
/// 7. gf100_gr_init_scc_0     — SCC
/// 8. gk110_gr_init_sked_0    — Scheduler
/// 9. gk110_gr_init_cwd_0     — CWD
/// 10. gf119_gr_init_prop_0   — PROP
/// 11. gf108_gr_init_gpc_unk_0
/// 12. gf100_gr_init_setup_0  — Setup
/// 13. gf100_gr_init_crstr_0  — CRSTR
/// 14. gf108_gr_init_setup_1  — Setup (additional)
/// 15. gf100_gr_init_zcull_0  — Z-cull
/// 16. gf119_gr_init_gpm_0    — GPM
/// 17. gk110_gr_init_gpc_unk_1
/// 18. gf100_gr_init_gcc_0    — GCC
/// 19. gk104_gr_init_gpc_unk_2
/// 20. gk104_gr_init_tpccs_0  — TPCCS
/// 21. gk110_gr_init_tex_0    — Texture
/// 22. gk104_gr_init_pe_0     — Pixel engine
/// 23. gk110_gr_init_l1c_0    — L1 cache
/// 24. gf100_gr_init_mpc_0    — MPC
/// 25. gk110_gr_init_sm_0     — Streaming multiprocessor
/// 26. gf117_gr_init_pes_0    — PES
/// 27. gf117_gr_init_wwdx_0   — WWDX
/// 28. gf117_gr_init_cbm_0    — CBM
/// 29. gk104_gr_init_be_0     — Backend
/// 30. gf100_gr_init_fe_1     — Frontend (final)
static GK110_GR_MMIO: &[(u32, u32)] = &[
    // ── gk104_gr_init_main_0 ──
    (0x400080, 0x003083c2),
    (0x400088, 0x0001ffe7),
    (0x40008c, 0x00000000),
    (0x400090, 0x00000030),
    (0x40013c, 0x003901f7),
    (0x400140, 0x00000100),
    (0x400144, 0x00000000),
    (0x400148, 0x00000110),
    (0x400138, 0x00000000),
    (0x400130, 0x00000000),
    (0x400134, 0x00000000),
    (0x400124, 0x00000002),
    // ── gk110_gr_init_fe_0 ──
    (0x40415c, 0x00000000),
    (0x404170, 0x00000000),
    (0x4041b4, 0x00000000),
    // ── gf100_gr_init_pri_0 ──
    (0x404488, 0x00000000),
    (0x40448c, 0x00000000),
    // ── gf100_gr_init_rstr2d_0 ──
    (0x407808, 0x00000000),
    // ── gf119_gr_init_pd_0 ──
    (0x406024, 0x00000000),
    (0x4064f0, 0x00000000),
    (0x4064f4, 0x00000000),
    (0x4064f8, 0x00000000),
    // ── gk110_gr_init_ds_0 ──
    (0x405844, 0x00ffffff),
    (0x405850, 0x00000000),
    (0x405900, 0x0000ff00),
    (0x405908, 0x00000000),
    (0x405928, 0x00000000),
    (0x40592c, 0x00000000),
    // ── gf100_gr_init_scc_0 ──
    (0x40803c, 0x00000000),
    // ── gk110_gr_init_sked_0 ──
    (0x407010, 0x00000000),
    (0x407040, 0x80440424),
    (0x407048, 0x0000000a),
    // ── gk110_gr_init_cwd_0 ──
    (0x405b44, 0x00000000),
    (0x405b50, 0x00000000),
    // ── gf119_gr_init_prop_0 ──
    (0x418408, 0x00000000),
    (0x4184a0, 0x00000000),
    (0x4184a4, 0x00000000),
    (0x4184a8, 0x00000000),
    // ── gf108_gr_init_gpc_unk_0 ──
    (0x418604, 0x00000000),
    (0x418680, 0x00000000),
    (0x418714, 0x00000000),
    (0x418384, 0x00000000),
    // ── gf100_gr_init_setup_0 ──
    (0x418814, 0x00000000),
    (0x418818, 0x00000000),
    (0x41881c, 0x00000000),
    // ── gf100_gr_init_crstr_0 ──
    (0x418b04, 0x00000000),
    // ── gf108_gr_init_setup_1 ──
    (0x4188c8, 0x00000000),
    (0x4188cc, 0x00000000),
    (0x4188d0, 0x00010000),
    (0x4188d4, 0x00000001),
    // ── gf100_gr_init_zcull_0 ──
    (0x418910, 0x00010001),
    (0x418914, 0x00000301),
    (0x418918, 0x00800000),
    (0x418980, 0x77777770),
    (0x418984, 0x77777777),
    (0x418988, 0x77777777),
    (0x41898c, 0x77777777),
    // ── gf119_gr_init_gpm_0 ──
    (0x418c04, 0x00000000),
    (0x418c64, 0x00000000),
    (0x418c68, 0x00000000),
    (0x418c88, 0x00000000),
    (0x418cb4, 0x00000000),
    (0x418cb8, 0x00000000),
    // ── gk110_gr_init_gpc_unk_1 ──
    (0x418d00, 0x00000000),
    (0x418d28, 0x00000000),
    (0x418d2c, 0x00000000),
    (0x418f00, 0x00000400),
    (0x418f08, 0x00000000),
    (0x418f20, 0x00000000),
    (0x418f24, 0x00000000),
    (0x418e00, 0x00000000),
    (0x418e08, 0x00000000),
    (0x418e1c, 0x00000000),
    (0x418e20, 0x00000000),
    // ── gf100_gr_init_gcc_0 ──
    (0x41900c, 0x00000000),
    (0x419018, 0x00000000),
    // ── gk104_gr_init_gpc_unk_2 ──
    (0x418884, 0x00000000),
    // ── gk104_gr_init_tpccs_0 ──
    (0x419d0c, 0x00000000),
    (0x419d10, 0x00000014),
    // ── gk110_gr_init_tex_0 ──
    (0x419ab0, 0x00000000),
    (0x419ac8, 0x00000000),
    (0x419ab8, 0x000000e7),
    (0x419aec, 0x00000000),
    (0x419abc, 0x00000000),
    (0x419ac0, 0x00000000),
    (0x419ab4, 0x00000000),
    (0x419aa8, 0x00000000),
    (0x419aac, 0x00000000),
    // ── gk104_gr_init_pe_0 ──
    (0x41980c, 0x00000010),
    (0x419844, 0x00000000),
    (0x419850, 0x00000004),
    (0x419854, 0x00000000),
    (0x419858, 0x00000000),
    // ── gk110_gr_init_l1c_0 ──
    (0x419c98, 0x00000000),
    (0x419ca8, 0x00000000),
    (0x419cb0, 0x01000000),
    (0x419cb4, 0x00000000),
    (0x419cb8, 0x00b08bea),
    (0x419c84, 0x00010384),
    (0x419cbc, 0x281b3646),
    (0x419cc0, 0x00000000),
    (0x419cc4, 0x00000000),
    (0x419c80, 0x00020230),
    (0x419ccc, 0x00000000),
    (0x419cd0, 0x00000000),
    // ── gf100_gr_init_mpc_0 ──
    (0x419c0c, 0x00000000),
    // ── gk110_gr_init_sm_0 ──
    (0x419e00, 0x00000080),
    (0x419ea0, 0x00000000),
    (0x419ee4, 0x00000000),
    (0x419ea4, 0x00000100),
    (0x419ea8, 0x00000000),
    (0x419eb4, 0x00000000),
    (0x419ebc, 0x00000000),
    (0x419ec0, 0x00000000),
    (0x419edc, 0x00000000),
    (0x419f00, 0x00000000),
    (0x419ed0, 0x00003234),
    (0x419f74, 0x00015555),
    (0x419f80, 0x00000000),
    (0x419f84, 0x00000000),
    (0x419f88, 0x00000000),
    (0x419f8c, 0x00000000),
    // ── gf117_gr_init_pes_0 ──
    (0x41be04, 0x00000000),
    (0x41be08, 0x00000004),
    (0x41be0c, 0x00000000),
    (0x41be10, 0x003b8bc7),
    (0x41be14, 0x00000000),
    (0x41be18, 0x00000000),
    // ── gf117_gr_init_wwdx_0 ──
    (0x41bfd4, 0x00800000),
    (0x41bfdc, 0x00000000),
    (0x41bff8, 0x00000000),
    (0x41bffc, 0x00000000),
    // ── gf117_gr_init_cbm_0 ──
    (0x41becc, 0x00000000),
    (0x41bee8, 0x00000000),
    (0x41beec, 0x00000000),
    // ── gk104_gr_init_be_0 ──
    (0x40880c, 0x00000000),
    (0x408850, 0x00000004),
    (0x408910, 0x00000000),
    (0x408914, 0x00000000),
    (0x408918, 0x00000000),
    (0x40891c, 0x00000000),
    (0x408920, 0x00000000),
    (0x408924, 0x00000000),
    (0x408928, 0x00000000),
    (0x40892c, 0x00000000),
    (0x408930, 0x00000000),
    (0x408950, 0x00000000),
    (0x408954, 0x0000ffff),
    (0x408958, 0x00000034),
    (0x408984, 0x00000000),
    (0x408988, 0x08040201),
    (0x40898c, 0x80402010),
    // ── gf100_gr_init_fe_1 ──
    (0x4040f0, 0x00000000),
];

/// Post-MMIO exception / interrupt setup — from `gf100_gr_init()`.
///
/// These writes configure the PGRAPH interrupt and exception mask
/// registers AFTER the MMIO init but BEFORE FECS/GPCCS boot.
static GK110_GR_EXCEPTIONS: &[(u32, u32)] = &[
    (0x400500, 0x00010001),
    (0x400100, 0xffffffff),
    (0x40013c, 0xffffffff),
    (0x400124, 0x00000002),
    // FECS exception config (firmware mode = 0x000e0001 for internal ucode)
    (0x409c24, 0x000e0001),
    // Trap/exception enables
    (0x404000, 0xc0000000),
    (0x404600, 0xc0000000),
    (0x408030, 0xc0000000),
    (0x40601c, 0xc0000000),
    (0x406018, 0xc0000000),
    (0x404490, 0xc0000000),
    (0x405840, 0xc0000000),
    (0x405844, 0x00ffffff),
    // Clear interrupt/status registers
    (0x400108, 0xffffffff),
    (0x400138, 0xffffffff),
    (0x400118, 0xffffffff),
    (0x400130, 0xffffffff),
    (0x40011c, 0xffffffff),
    (0x400134, 0xffffffff),
    // PGRAPH misc
    (0x400054, 0x34ce3464),
];

/// Apply the GK110 PGRAPH MMIO init table + exception config.
///
/// Returns `(applied, faulted)` counts.
pub(crate) fn apply_gk110_gr_init(guard: &super::hardware_guard::GuardedBar<'_>) -> (u32, u32) {
    let mut applied = 0u32;
    let mut faulted = 0u32;

    for &(reg, val) in GK110_GR_MMIO.iter().chain(GK110_GR_EXCEPTIONS.iter()) {
        match guard.write_u32(reg, val) {
            Ok(()) => applied += 1,
            Err(_) => faulted += 1,
        }
    }

    tracing::info!(applied, faulted, "GK110 PGRAPH MMIO init applied");
    (applied, faulted)
}

/// GK110 clock gating initialization table (BLCG + SLCG).
///
/// Derived from nouveau's `gk110_clkgate_pack[]` which references both
/// `gk104_clkgate_blcg_*` base tables and `gk110_clkgate_*` overrides.
/// Applied by nouveau via `nvkm_therm_clkgate_init()` after MMIO init
/// and before firmware upload. Without these, GPC Falcon CPU clocks may
/// remain gated after PMC GR reset, causing STARTCPU to be silently ignored.
///
/// Multi-count entries (count > 1) are expanded to flat (addr, value) pairs
/// with stride 4.
#[rustfmt::skip]
const GK110_CLKGATE_INIT: &[(u32, u32)] = &[
    // --- BLCG tables ---
    // gk104_clkgate_blcg_init_main_0
    (0x40_41f0, 0x0000_4046),
    (0x40_9890, 0x0000_0045),
    (0x40_98b0, 0x0000_007f),
    // gk104_clkgate_blcg_init_rstr2d_0
    (0x40_78c0, 0x0000_0042),
    // gk104_clkgate_blcg_init_unk_0
    (0x40_6000, 0x0000_4044),
    (0x40_5860, 0x0000_4042),
    (0x40_590c, 0x0000_4042),
    // gk104_clkgate_blcg_init_gcc_0
    (0x40_8040, 0x0000_4044),
    // gk110_clkgate_blcg_init_sked_0 (GK110 override)
    (0x40_7000, 0x0000_4041),
    // gk104_clkgate_blcg_init_unk_1
    (0x40_5bf0, 0x0000_4044),
    // gk104_clkgate_blcg_init_gpc_ctxctl_0 — GPCCS Falcon clock gating
    (0x41_a890, 0x0000_0042),
    (0x41_a8b0, 0x0000_007f),
    // gk104_clkgate_blcg_init_gpc_unk_0
    (0x41_8500, 0x0000_4042),
    (0x41_8608, 0x0000_4042),
    (0x41_8688, 0x0000_4042),
    (0x41_8718, 0x0000_0042),
    // gk104_clkgate_blcg_init_gpc_esetup_0
    (0x41_8828, 0x0000_0044),
    // gk104_clkgate_blcg_init_gpc_tpbus_0
    (0x41_8bbc, 0x0000_4042),
    // gk104_clkgate_blcg_init_gpc_zcull_0
    (0x41_8970, 0x0000_4042),
    // gk104_clkgate_blcg_init_gpc_tpconf_0
    (0x41_8c70, 0x0000_4042),
    // gk104_clkgate_blcg_init_gpc_unk_1
    (0x41_8cf0, 0x0000_4042),
    (0x41_8d70, 0x0000_4042),
    (0x41_8f0c, 0x0000_4042),
    (0x41_8e0c, 0x0000_4042),
    // gk110_clkgate_blcg_init_gpc_gcc_0 (GK110 override)
    (0x41_9020, 0x0000_0042),
    (0x41_9038, 0x0000_0042),
    // gk104_clkgate_blcg_init_gpc_ffb_0
    (0x41_8898, 0x0000_0042),
    // gk104_clkgate_blcg_init_gpc_tex_0 (9 regs @ stride 4)
    (0x41_9a40, 0x0000_4042),
    (0x41_9a44, 0x0000_4042),
    (0x41_9a48, 0x0000_4042),
    (0x41_9a4c, 0x0000_4042),
    (0x41_9a50, 0x0000_4042),
    (0x41_9a54, 0x0000_4042),
    (0x41_9a58, 0x0000_4042),
    (0x41_9a5c, 0x0000_4042),
    (0x41_9a60, 0x0000_4042),
    (0x41_9acc, 0x0000_4047),
    // gk104_clkgate_blcg_init_gpc_poly_0
    (0x41_9868, 0x0000_0042),
    // gk110_clkgate_blcg_init_gpc_l1c_0 (GK110 override, 2 regs)
    (0x41_9cd4, 0x0000_4042),
    (0x41_9cd8, 0x0000_4042),
    // gk104_clkgate_blcg_init_gpc_unk_2
    (0x41_9c70, 0x0000_4045),
    // gk110_clkgate_blcg_init_gpc_mp_0 (GK110 override)
    (0x41_9fd0, 0x0000_4043),
    (0x41_9fd8, 0x0000_4049),
    (0x41_9fe0, 0x0000_4042),
    (0x41_9fe4, 0x0000_4042),
    (0x41_9ff0, 0x0000_0046),
    (0x41_9ff8, 0x0000_4042),
    (0x41_9f90, 0x0000_4042),
    // gk104_clkgate_blcg_init_gpc_ppc_0
    (0x41_be28, 0x0000_0042),
    (0x41_bfe8, 0x0000_4042),
    (0x41_bed0, 0x0000_4042),
    // gk104_clkgate_blcg_init_rop_zrop_0 (2 regs)
    (0x40_8810, 0x0000_4042),
    (0x40_8814, 0x0000_4042),
    // gk104_clkgate_blcg_init_rop_0 (6 regs)
    (0x40_8a80, 0x0000_4042),
    (0x40_8a84, 0x0000_4042),
    (0x40_8a88, 0x0000_4042),
    (0x40_8a8c, 0x0000_4042),
    (0x40_8a90, 0x0000_4042),
    (0x40_8a94, 0x0000_4042),
    // gk104_clkgate_blcg_init_rop_crop_0
    (0x40_89a8, 0x0000_4042),
    (0x40_89b0, 0x0000_0042),
    (0x40_89b8, 0x0000_4042),
    // gk104_clkgate_blcg_init_pxbar_0
    (0x13_c820, 0x0001_007f),
    (0x13_cbe0, 0x0000_0042),

    // --- SLCG tables (from gk110.c) ---
    // gk110_clkgate_slcg_init_main_0
    (0x40_41f4, 0x0000_0000),
    (0x40_9894, 0x0000_0000),
    // gk110_clkgate_slcg_init_unk_0
    (0x40_6004, 0x0000_0000),
    // gk110_clkgate_slcg_init_sked_0
    (0x40_7004, 0x0000_0000),
    // gk110_clkgate_slcg_init_gpc_ctxctl_0 — GPCCS SLCG
    (0x41_a894, 0x0000_0000),
    // gk110_clkgate_slcg_init_gpc_unk_0
    (0x41_8504, 0x0000_0000),
    (0x41_860c, 0x0000_0000),
    (0x41_868c, 0x0000_0000),
    // gk110_clkgate_slcg_init_gpc_esetup_0
    (0x41_882c, 0x0000_0000),
    // gk110_clkgate_slcg_init_gpc_zcull_0
    (0x41_8974, 0x0000_0000),
    // gk110_clkgate_slcg_init_gpc_l1c_0 (2 regs)
    (0x41_9cd8, 0x0000_0000),
    (0x41_9cdc, 0x0000_0000),
    // gk110_clkgate_slcg_init_gpc_unk_1
    (0x41_9c74, 0x0000_0000),
    // gk110_clkgate_slcg_init_gpc_mp_0
    (0x41_9fd4, 0x0000_4a4a),
    (0x41_9fdc, 0x0000_0014),
    (0x41_9fe4, 0x0000_0000),
    (0x41_9ff4, 0x0000_1724),
    // gk110_clkgate_slcg_init_gpc_ppc_0
    (0x41_be2c, 0x0000_0000),
    // gk110_clkgate_slcg_init_pcounter_0
    (0x1b_e018, 0x0000_01ff),
    (0x1b_c018, 0x0000_01ff),
    (0x1b_8018, 0x0000_01ff),
    (0x1b_4124, 0x0000_0000),
];

/// Apply the GK110 clock gating initialization (BLCG + SLCG).
///
/// Returns `(applied, faulted)` counts.
pub(crate) fn apply_gk110_clkgate(guard: &super::hardware_guard::GuardedBar<'_>) -> (u32, u32) {
    let mut applied = 0u32;
    let mut faulted = 0u32;

    for &(reg, val) in GK110_CLKGATE_INIT {
        match guard.write_u32(reg, val) {
            Ok(()) => applied += 1,
            Err(_) => faulted += 1,
        }
    }

    tracing::info!(applied, faulted, "GK110 clock gating init applied");
    (applied, faulted)
}
