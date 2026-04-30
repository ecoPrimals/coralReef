// SPDX-License-Identifier: AGPL-3.0-or-later
//! Kepler (GK110B) GR context-switch compressed register lists from Nouveau
//! (`ctxgf100.c`, `ctxgk104.c`, `ctxgk110.c`, `ctxgk110b.c`, `ctxgf117.c`).
//! `GF119_*` init slices match `ctxgf119.c` (referenced by GK110 packs, not duplicated in the other files).
//!
//! Each tuple is `(addr, count, pitch)` as in `struct gf100_gr_init`. Rows with `count == 0` are
//! omitted so iteration matches `pack_for_each_init` in Nouveau.

pub(crate) const GF100_GRCTX_INIT_MAIN_0: &[(u32, u32, u32)] = &[(0x00400204, 2, 0x00000004)];

pub(crate) const GF100_GRCTX_INIT_RSTR2D_0: &[(u32, u32, u32)] = &[
    (0x00407804, 1, 0x00000004),
    (0x0040780c, 1, 0x00000004),
    (0x00407810, 1, 0x00000004),
    (0x00407814, 1, 0x00000004),
    (0x00407818, 1, 0x00000004),
    (0x0040781c, 1, 0x00000004),
    (0x00407820, 1, 0x00000004),
    (0x004078bc, 1, 0x00000004),
];

pub(crate) const GF100_GRCTX_INIT_GPC_UNK_0: &[(u32, u32, u32)] = &[(0x00418380, 1, 0x00000004)];

pub(crate) const GF100_GRCTX_INIT_ZCULL_0: &[(u32, u32, u32)] = &[
    (0x0041891c, 1, 0x00000004),
    (0x00418924, 1, 0x00000004),
    (0x00418928, 1, 0x00000004),
    (0x0041892c, 1, 0x00000004),
];

pub(crate) const GF100_GRCTX_INIT_GCC_0: &[(u32, u32, u32)] = &[
    (0x00419000, 1, 0x00000004),
    (0x00419004, 2, 0x00000004),
    (0x00419014, 1, 0x00000004),
];

pub(crate) const GK104_GRCTX_INIT_MEMFMT_0: &[(u32, u32, u32)] = &[
    (0x00404604, 1, 0x00000004),
    (0x00404608, 1, 0x00000004),
    (0x0040460c, 1, 0x00000004),
    (0x00404610, 1, 0x00000004),
    (0x00404618, 4, 0x00000004),
    (0x0040462c, 2, 0x00000004),
    (0x00404640, 1, 0x00000004),
    (0x00404654, 1, 0x00000004),
    (0x00404660, 1, 0x00000004),
    (0x00404678, 1, 0x00000004),
    (0x0040467c, 1, 0x00000004),
    (0x00404680, 8, 0x00000004),
    (0x004046a0, 1, 0x00000004),
    (0x004046a4, 8, 0x00000004),
    (0x004046c8, 3, 0x00000004),
    (0x00404700, 3, 0x00000004),
    (0x00404718, 7, 0x00000004),
    (0x00404734, 1, 0x00000004),
    (0x00404738, 2, 0x00000004),
    (0x00404744, 2, 0x00000004),
    (0x00404754, 1, 0x00000004),
];

pub(crate) const GK104_GRCTX_INIT_DS_0: &[(u32, u32, u32)] = &[
    (0x00405800, 1, 0x00000004),
    (0x00405830, 1, 0x00000004),
    (0x00405834, 1, 0x00000004),
    (0x00405838, 1, 0x00000004),
    (0x00405854, 1, 0x00000004),
    (0x00405870, 4, 0x00000004),
    (0x00405a00, 2, 0x00000004),
    (0x00405a18, 1, 0x00000004),
];

pub(crate) const GK104_GRCTX_INIT_SCC_0: &[(u32, u32, u32)] = &[
    (0x00408000, 2, 0x00000004),
    (0x00408008, 1, 0x00000004),
    (0x0040800c, 2, 0x00000004),
    (0x00408014, 1, 0x00000004),
    (0x00408018, 1, 0x00000004),
    (0x00408064, 1, 0x00000004),
];

pub(crate) const GK104_GRCTX_INIT_GPM_0: &[(u32, u32, u32)] = &[
    (0x00418c08, 1, 0x00000004),
    (0x00418c10, 8, 0x00000004),
    (0x00418c40, 1, 0x00000004),
    (0x00418c6c, 1, 0x00000004),
    (0x00418c80, 1, 0x00000004),
    (0x00418c8c, 1, 0x00000004),
];

pub(crate) const GK104_GRCTX_INIT_PES_0: &[(u32, u32, u32)] = &[(0x0041be24, 1, 0x00000004)];

pub(crate) const GK110_GRCTX_INIT_FE_0: &[(u32, u32, u32)] = &[
    (0x00404004, 8, 0x00000004),
    (0x00404024, 1, 0x00000004),
    (0x00404028, 8, 0x00000004),
    (0x004040a8, 8, 0x00000004),
    (0x004040c8, 1, 0x00000004),
    (0x004040d0, 6, 0x00000004),
    (0x004040e8, 1, 0x00000004),
    (0x004040f8, 1, 0x00000004),
    (0x00404100, 10, 0x00000004),
    (0x00404130, 2, 0x00000004),
    (0x00404138, 1, 0x00000004),
    (0x00404150, 1, 0x00000004),
    (0x00404154, 1, 0x00000004),
    (0x00404158, 1, 0x00000004),
    (0x00404164, 1, 0x00000004),
    (0x0040417c, 2, 0x00000004),
    (0x004041a0, 4, 0x00000004),
    (0x00404200, 1, 0x00000004),
    (0x00404204, 1, 0x00000004),
    (0x00404208, 1, 0x00000004),
    (0x0040420c, 1, 0x00000004),
];

pub(crate) const GK110_GRCTX_INIT_PRI_0: &[(u32, u32, u32)] = &[
    (0x00404404, 12, 0x00000004),
    (0x00404438, 1, 0x00000004),
    (0x00404460, 2, 0x00000004),
    (0x00404468, 1, 0x00000004),
    (0x0040446c, 1, 0x00000004),
    (0x00404480, 1, 0x00000004),
    (0x00404498, 1, 0x00000004),
];

pub(crate) const GK110_GRCTX_INIT_CWD_0: &[(u32, u32, u32)] = &[
    (0x00405b00, 1, 0x00000004),
    (0x00405b10, 1, 0x00000004),
    (0x00405b20, 1, 0x00000004),
];

pub(crate) const GK110_GRCTX_INIT_PD_0: &[(u32, u32, u32)] = &[
    (0x00406020, 1, 0x00000004),
    (0x00406028, 4, 0x00000004),
    (0x004064a8, 1, 0x00000004),
    (0x004064ac, 1, 0x00000004),
    (0x004064b0, 3, 0x00000004),
    (0x004064c0, 1, 0x00000004),
    (0x004064c4, 1, 0x00000004),
    (0x004064c8, 1, 0x00000004),
    (0x004064cc, 9, 0x00000004),
    (0x004064fc, 1, 0x00000004),
];

pub(crate) const GK110_GRCTX_INIT_BE_0: &[(u32, u32, u32)] = &[
    (0x00408800, 1, 0x00000004),
    (0x00408804, 1, 0x00000004),
    (0x00408808, 1, 0x00000004),
    (0x00408840, 1, 0x00000004),
    (0x00408900, 1, 0x00000004),
    (0x00408904, 1, 0x00000004),
    (0x00408908, 1, 0x00000004),
    (0x00408980, 1, 0x00000004),
];

pub(crate) const GK110_GRCTX_INIT_SETUP_0: &[(u32, u32, u32)] = &[
    (0x00418800, 1, 0x00000004),
    (0x00418808, 1, 0x00000004),
    (0x0041880c, 1, 0x00000004),
    (0x00418810, 1, 0x00000004),
    (0x00418828, 1, 0x00000004),
    (0x00418830, 1, 0x00000004),
    (0x004188d8, 1, 0x00000004),
    (0x004188e0, 1, 0x00000004),
    (0x004188e8, 5, 0x00000004),
    (0x004188fc, 1, 0x00000004),
];

pub(crate) const GK110_GRCTX_INIT_GPC_UNK_2: &[(u32, u32, u32)] = &[(0x00418d24, 1, 0x00000004)];

pub(crate) const GK110_GRCTX_INIT_TEX_0: &[(u32, u32, u32)] = &[
    (0x00419a00, 1, 0x00000004),
    (0x00419a04, 1, 0x00000004),
    (0x00419a08, 1, 0x00000004),
    (0x00419a0c, 1, 0x00000004),
    (0x00419a10, 1, 0x00000004),
    (0x00419a14, 1, 0x00000004),
    (0x00419a1c, 1, 0x00000004),
    (0x00419a20, 1, 0x00000004),
    (0x00419a30, 1, 0x00000004),
    (0x00419ac4, 1, 0x00000004),
];

pub(crate) const GK110_GRCTX_INIT_MPC_0: &[(u32, u32, u32)] = &[
    (0x00419c00, 1, 0x00000004),
    (0x00419c04, 1, 0x00000004),
    (0x00419c08, 1, 0x00000004),
    (0x00419c20, 1, 0x00000004),
    (0x00419c24, 1, 0x00000004),
    (0x00419c28, 1, 0x00000004),
];

pub(crate) const GK110_GRCTX_INIT_L1C_0: &[(u32, u32, u32)] =
    &[(0x00419ce8, 1, 0x00000004), (0x00419cf4, 1, 0x00000004)];

pub(crate) const GK110_GRCTX_INIT_CBM_0: &[(u32, u32, u32)] = &[
    (0x0041bec0, 1, 0x00000004),
    (0x0041bec4, 1, 0x00000004),
    (0x0041bee4, 1, 0x00000004),
];

pub(crate) const GK110B_GRCTX_INIT_SM_0: &[(u32, u32, u32)] = &[
    (0x00419e04, 1, 0x00000004),
    (0x00419e08, 1, 0x00000004),
    (0x00419e0c, 1, 0x00000004),
    (0x00419e10, 1, 0x00000004),
    (0x00419e44, 1, 0x00000004),
    (0x00419e48, 1, 0x00000004),
    (0x00419e4c, 1, 0x00000004),
    (0x00419e50, 2, 0x00000004),
    (0x00419e58, 1, 0x00000004),
    (0x00419e5c, 3, 0x00000004),
    (0x00419e68, 1, 0x00000004),
    (0x00419e6c, 12, 0x00000004),
    (0x00419eac, 1, 0x00000004),
    (0x00419eb0, 1, 0x00000004),
    (0x00419eb8, 1, 0x00000004),
    (0x00419ec8, 1, 0x00000004),
    (0x00419f30, 4, 0x00000004),
    (0x00419f40, 1, 0x00000004),
    (0x00419f44, 3, 0x00000004),
    (0x00419f58, 1, 0x00000004),
    (0x00419f70, 1, 0x00000004),
    (0x00419f78, 1, 0x00000004),
    (0x00419f7c, 1, 0x00000004),
];

pub(crate) const GF117_GRCTX_INIT_PE_0: &[(u32, u32, u32)] = &[
    (0x00419848, 1, 0x00000004),
    (0x00419864, 1, 0x00000004),
    (0x00419888, 1, 0x00000004),
];

pub(crate) const GF117_GRCTX_INIT_WWDX_0: &[(u32, u32, u32)] = &[
    (0x0041bf00, 1, 0x00000004),
    (0x0041bf04, 1, 0x00000004),
    (0x0041bf08, 1, 0x00000004),
    (0x0041bf0c, 1, 0x00000004),
    (0x0041bf10, 1, 0x00000004),
    (0x0041bf14, 1, 0x00000004),
    (0x0041bfd0, 1, 0x00000004),
    (0x0041bfe0, 1, 0x00000004),
    (0x0041bfe4, 1, 0x00000004),
];

pub(crate) const GF119_GRCTX_INIT_PROP_0: &[(u32, u32, u32)] = &[
    (0x00418400, 1, 0x00000004),
    (0x00418404, 1, 0x00000004),
    (0x0041840c, 1, 0x00000004),
    (0x00418410, 1, 0x00000004),
    (0x00418414, 1, 0x00000004),
    (0x00418450, 6, 0x00000004),
    (0x00418468, 1, 0x00000004),
    (0x0041846c, 2, 0x00000004),
];

pub(crate) const GF119_GRCTX_INIT_GPC_UNK_1: &[(u32, u32, u32)] = &[
    (0x00418600, 1, 0x00000004),
    (0x00418684, 1, 0x00000004),
    (0x00418700, 1, 0x00000004),
    (0x00418704, 1, 0x00000004),
    (0x00418708, 3, 0x00000004),
];

pub(crate) const GF119_GRCTX_INIT_CRSTR_0: &[(u32, u32, u32)] = &[
    (0x00418b00, 1, 0x00000004),
    (0x00418b08, 1, 0x00000004),
    (0x00418b0c, 1, 0x00000004),
    (0x00418b10, 1, 0x00000004),
    (0x00418b14, 1, 0x00000004),
    (0x00418b18, 1, 0x00000004),
    (0x00418b1c, 1, 0x00000004),
    (0x00418bb8, 1, 0x00000004),
];

/// Hub pack: falcon 0x409000, starstar 0, base 0.
pub(crate) const GK110B_GRCTX_PACK_HUB: &[&[(u32, u32, u32)]] = &[
    GF100_GRCTX_INIT_MAIN_0,
    GK110_GRCTX_INIT_FE_0,
    GK110_GRCTX_INIT_PRI_0,
    GK104_GRCTX_INIT_MEMFMT_0,
    GK104_GRCTX_INIT_DS_0,
    GK110_GRCTX_INIT_CWD_0,
    GK110_GRCTX_INIT_PD_0,
    GF100_GRCTX_INIT_RSTR2D_0,
    GK104_GRCTX_INIT_SCC_0,
    GK110_GRCTX_INIT_BE_0,
];

/// GPC_0 pack: falcon 0x41a000, starstar 0, base 0x418000.
pub(crate) const GK110B_GRCTX_PACK_GPC_0: &[&[(u32, u32, u32)]] = &[
    GF100_GRCTX_INIT_GPC_UNK_0,
    GF119_GRCTX_INIT_PROP_0,
    GF119_GRCTX_INIT_GPC_UNK_1,
    GK110_GRCTX_INIT_SETUP_0,
    GF100_GRCTX_INIT_ZCULL_0,
];

/// GPC_1 pack.
pub(crate) const GK110B_GRCTX_PACK_GPC_1: &[&[(u32, u32, u32)]] = &[
    GF119_GRCTX_INIT_CRSTR_0,
    GK104_GRCTX_INIT_GPM_0,
    GK110_GRCTX_INIT_GPC_UNK_2,
    GF100_GRCTX_INIT_GCC_0,
];

/// TPC pack (GK110B uses `gk110b_grctx_pack_tpc` — SM list differs from GK110).
pub(crate) const GK110B_GRCTX_PACK_TPC: &[&[(u32, u32, u32)]] = &[
    GF117_GRCTX_INIT_PE_0,
    GK110_GRCTX_INIT_TEX_0,
    GK110_GRCTX_INIT_MPC_0,
    GK110_GRCTX_INIT_L1C_0,
    GK110B_GRCTX_INIT_SM_0,
];

/// PPC pack: falcon 0x41a000, starstar 8, base 0x41be00.
pub(crate) const GK110B_GRCTX_PACK_PPC: &[&[(u32, u32, u32)]] = &[
    GK104_GRCTX_INIT_PES_0,
    GK110_GRCTX_INIT_CBM_0,
    GF117_GRCTX_INIT_WWDX_0,
];

const AINCR: u32 = 0x0200_0000;
const AINCW: u32 = 0x0100_0000;

/// Falcon DMEM control (`falcon + 0x1c0`) / data (`falcon + 0x1c4`) programming for
/// compressed context-switch register lists (`gf100_gr_init_csdata` in Nouveau).
///
/// # Preconditions
/// - Falcon must be idle (not running)
/// - PGRAPH must be enabled in PMC
pub(crate) fn load_csdata(
    rd: &dyn Fn(u32) -> u32,
    wr: &dyn Fn(u32, u32),
    pack: &[&[(u32, u32, u32)]],
    falcon: u32,
    starstar: u32,
    base: u32,
) {
    wr(falcon + 0x01c0, AINCR + starstar);
    let mut star = rd(falcon + 0x01c4);
    let temp = rd(falcon + 0x01c4);
    if temp > star {
        star = temp;
    }
    wr(falcon + 0x01c0, AINCW + star);

    let mut addr = !0u32;
    let mut prev = !0u32;
    let mut xfer = 0u32;

    for init_list in pack {
        for &(init_addr, count, pitch) in *init_list {
            if count == 0 {
                break;
            }
            let mut head = init_addr.wrapping_sub(base);
            let tail = head.wrapping_add(count.wrapping_mul(pitch));
            while head < tail {
                if head != prev.wrapping_add(4) || xfer >= 32 {
                    if xfer != 0 {
                        let data = ((xfer - 1) << 26) | addr;
                        wr(falcon + 0x01c4, data);
                        star = star.wrapping_add(4);
                    }
                    addr = head;
                    xfer = 0;
                }
                prev = head;
                xfer = xfer.wrapping_add(1);
                head = head.wrapping_add(pitch);
            }
        }
    }

    debug_assert!(xfer > 0, "empty csdata init entry");
    let data = ((xfer - 1) << 26) | addr;
    wr(falcon + 0x01c4, data);
    wr(falcon + 0x01c0, 0x0100_0004 + starstar);
    wr(falcon + 0x01c4, star.wrapping_add(4));
}
