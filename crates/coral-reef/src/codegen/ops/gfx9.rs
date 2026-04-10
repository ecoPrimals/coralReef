// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright © 2026 ecoPrimals
//! RDNA2 → GFX9 instruction word remapping (VOP2/VOP3/VOPC opcode translation).

/// Patch VOP3 words from RDNA2 to GFX9.
///
/// Both architectures share the same VOP3a word-0 layout:
///   [31:26]=prefix  [25:16]=OP(10)  [15]=CLAMP  [10:8]=ABS  [7:0]=VDST
///
/// Three things differ:
///   1. Prefix: 110101 (RDNA2) → 110100 (GFX9)
///   2. VOP3-only opcode values (≥320) are remapped between architectures
///   3. VOPC opcodes encoded as VOP3 (0-255) are also reshuffled on GFX9
///
/// VOP2-promoted VOP3 opcodes (256-319) are already translated before encoding
/// via `vop3_promoted_opcode_for_gfx`, so we leave those unchanged.
pub(super) fn patch_vop3_prefix_for_gfx9(words: &mut [u32]) {
    for word in words.iter_mut() {
        if (*word >> 26) & 0x3F == 0b11_0101 {
            let op_rdna2 = ((*word >> 16) & 0x3FF) as u16;
            let rest = *word & 0xFFFF;
            let op_gfx9 = if op_rdna2 >= 320 {
                vop3_only_opcode_for_gfx9(op_rdna2)
            } else if op_rdna2 < 256 {
                vopc_opcode_for_gfx9(op_rdna2)
            } else {
                op_rdna2
            };
            *word = (0b11_0100u32 << 26) | ((op_gfx9 as u32 & 0x3FF) << 16) | rest;
        }
    }
}

/// Translate VOP3-only opcodes from RDNA2 to GFX9.
///
/// Group A (MAD/BFE/BFI/FMA, RDNA2 320-351) shifts uniformly by +128.
/// Group B (F64 arith / MUL_HI, RDNA2 352+) requires per-instruction mapping
/// because the instruction ordering changed between architectures.
/// Group C (VOP1-promoted VOP3, RDNA2 384+): on RDNA2, VOP3 = VOP1 + 384;
/// on GFX9, VOP3 = VOP1_gfx9 + 320. RDNA2 inserted new VOP1 opcodes
/// (V_PIPEFLUSH, etc.) that shifted everything after opcode 26.
/// LLVM `llvm-mc --mcpu=gfx906` was used to derive every entry.
fn vop3_only_opcode_for_gfx9(rdna2_op: u16) -> u16 {
    match rdna2_op {
        // Group A: uniform +128 offset (verified via LLVM for MAD/FMA/BFE/BFI)
        320..=351 => rdna2_op + 128,
        // Group B: per-instruction (LLVM-validated)
        356 => 640, // V_ADD_F64
        357 => 641, // V_MUL_F64
        358 => 642, // V_MIN_F64
        359 => 643, // V_MAX_F64
        362 => 646, // V_MUL_HI_U32
        364 => 647, // V_MUL_HI_I32
        // Group C: VOP1-promoted VOP3 (RDNA2 384+, VOP1 opcodes 0-26 unchanged)
        384..=410 => rdna2_op - 64,
        // Group C cont: VOP1 opcodes 27+ shifted on GFX9 (per-instruction)
        416 => 347, // V_FRACT_F32
        417 => 348, // V_TRUNC_F32
        418 => 349, // V_CEIL_F32
        419 => 350, // V_RNDNE_F32
        420 => 351, // V_FLOOR_F32
        421 => 352, // V_EXP_F32
        423 => 353, // V_LOG_F32
        426 => 354, // V_RCP_F32
        427 => 355, // V_RCP_IFLAG_F32
        430 => 356, // V_RSQ_F32
        431 => 357, // V_RCP_F64
        433 => 358, // V_RSQ_F64
        435 => 359, // V_SQRT_F32
        436 => 360, // V_SQRT_F64
        437 => 361, // V_SIN_F32
        438 => 362, // V_COS_F32
        439 => 363, // V_NOT_B32
        440 => 364, // V_BFREV_B32
        441 => 365, // V_FFBH_U32
        442 => 366, // V_FFBL_B32
        443 => 367, // V_FFBH_I32
        444 => 368, // V_FREXP_EXP_I32_F64
        445 => 369, // V_FREXP_MANT_F64
        446 => 370, // V_FRACT_F64
        447 => 371, // V_FREXP_EXP_I32_F32
        448 => 372, // V_FREXP_MANT_F32
        449 => 373, // V_CLREXCP
        _ => rdna2_op,
    }
}

/// Remap VOPC opcodes from RDNA2 to GFX9.
///
/// RDNA2 reorganised the VOPC opcode space; GFX9 uses a different layout.
/// Applied to VOPC opcodes that appear inside VOP3 encoding (f64 compares)
/// and to VOPC e32 encoding (f32/i32/u32 compares).
/// Verified against `llvm-mc -mcpu=gfx906 -show-encoding`.
pub(super) fn vopc_opcode_for_gfx9(rdna2_op: u16) -> u16 {
    match rdna2_op {
        // F32 compares: RDNA2 0-15 → GFX9 64-79
        0..=15 => rdna2_op + 64,
        // CMPX F32: RDNA2 16-31 → GFX9 80-95
        16..=31 => rdna2_op + 64,
        // F64 compares: RDNA2 32-47 → GFX9 96-111
        32..=47 => rdna2_op + 64,
        // CMPX F64: RDNA2 48-63 → GFX9 112-127
        48..=63 => rdna2_op + 64,
        // I32 compares: RDNA2 128-143 → GFX9 192-207
        128..=143 => rdna2_op + 64,
        // CMPX I32: RDNA2 144-159 → GFX9 208-223
        144..=159 => rdna2_op + 64,
        // U32 compares: RDNA2 192-207 → GFX9 200-215
        192..=207 => rdna2_op + 8,
        // CMPX U32: RDNA2 208-223 → GFX9 216-231
        208..=223 => rdna2_op + 8,
        _ => rdna2_op,
    }
}

/// Patch VOPC e32 (32-bit) words from RDNA2 to GFX9.
///
/// VOPC e32 format: [31:25]=0111110, [24:17]=OP(8), [16:9]=VSRC1, [8:0]=SRC0.
/// The prefix is the same on both architectures, but opcodes differ.
pub(super) fn patch_vopc_for_gfx9(words: &mut [u32]) {
    for word in words.iter_mut() {
        if (*word >> 25) & 0x7F == 0b011_1110 {
            let op_rdna2 = ((*word >> 17) & 0xFF) as u16;
            let op_gfx9 = vopc_opcode_for_gfx9(op_rdna2);
            let rest = *word & 0x0001_FFFF;
            let prefix = *word & 0xFE00_0000;
            *word = prefix | ((op_gfx9 as u32 & 0xFF) << 17) | rest;
        }
    }
}

// ---- GFX9 VOP2 opcode remap ----
//
// RDNA (GFX10+) reshuffled VOP2 opcodes relative to GCN5 (GFX9).
// V_ADD_NC_U32 (no carry) doesn't exist on GFX9; we substitute
// V_ADD_CO_U32 which generates carry to VCC (harmless when unread).

pub(super) fn vop2_opcode_for_gfx(rdna2_op: u16, gfx_major: u8) -> u16 {
    if gfx_major >= 10 {
        return rdna2_op;
    }
    match rdna2_op {
        1 => 0,   // V_CNDMASK_B32
        3 => 1,   // V_ADD_F32
        4 => 2,   // V_SUB_F32
        5 => 3,   // V_SUBREV_F32
        8 => 5,   // V_MUL_F32
        9 => 6,   // V_MUL_I32_I24
        11 => 8,  // V_MUL_U32_U24
        15 => 10, // V_MIN_F32
        16 => 11, // V_MAX_F32
        17 => 12, // V_MIN_I32
        18 => 13, // V_MAX_I32
        19 => 14, // V_MIN_U32
        20 => 15, // V_MAX_U32
        22 => 16, // V_LSHRREV_B32
        24 => 17, // V_ASHRREV_I32
        26 => 18, // V_LSHLREV_B32
        27 => 19, // V_AND_B32
        28 => 20, // V_OR_B32
        29 => 21, // V_XOR_B32
        37 => 25, // V_ADD_NC_U32 → V_ADD_CO_U32
        38 => 26, // V_SUB_NC_U32 → V_SUB_CO_U32
        39 => 27, // V_SUBREV_NC_U32 → V_SUBREV_CO_U32
        40 => 28, // V_ADD_CO_CI_U32 → V_ADDC_CO_U32
        _ => rdna2_op,
    }
}

/// Remap a VOP3 opcode that was promoted from VOP2.
/// On RDNA2: VOP3_opcode = VOP2_opcode + 256.
/// On GFX9:  VOP3_opcode = GFX9_VOP2_opcode + 256.
pub(super) fn vop3_promoted_opcode_for_gfx(rdna2_vop3: u16, gfx_major: u8) -> u16 {
    if gfx_major >= 10 {
        return rdna2_vop3;
    }
    if (256..512).contains(&rdna2_vop3) {
        let rdna2_vop2 = rdna2_vop3 - 256;
        vop2_opcode_for_gfx(rdna2_vop2, gfx_major) + 256
    } else {
        rdna2_vop3
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patch_vop3_prefix_for_gfx9_rewrites_prefix_and_opcode() {
        let mut w = (0b11_0101u32 << 26) | ((0x0140_u32 & 0x3FF) << 16) | 0x00AB_u32;
        patch_vop3_prefix_for_gfx9(std::slice::from_mut(&mut w));
        assert_eq!((w >> 26) & 0x3F, 0b11_0100, "GFX9 VOP3 prefix");
        assert_eq!((w >> 16) & 0x3FF, 448, "320 + 128 remap");
    }

    #[test]
    fn patch_vopc_for_gfx9_remaps_compare_opcode() {
        let mut w: u32 = (0b011_1110u32 << 25) | ((5_u32 & 0xFF) << 17) | 0x0001_FEDC;
        patch_vopc_for_gfx9(std::slice::from_mut(&mut w));
        assert_eq!((w >> 25) & 0x7F, 0b011_1110);
        assert_eq!((w >> 17) & 0xFF, 69, "RDNA2 VOPC 5 → GFX9 69");
    }

    #[test]
    fn patch_vopc_for_gfx9_leaves_non_vopc_prefix() {
        let mut w: u32 = 0xFFFF_FFFF;
        patch_vopc_for_gfx9(std::slice::from_mut(&mut w));
        assert_eq!(w, 0xFFFF_FFFF);
    }

    #[test]
    fn vop3_promoted_opcode_for_gfx9_remaps_vop2_base() {
        assert_eq!(
            vop3_promoted_opcode_for_gfx(256 + 3, 9),
            256 + 1,
            "V_ADD_F32 RDNA2 op 3 → GFX9 op 1 inside VOP3+256"
        );
    }
}
