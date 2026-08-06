// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright © 2026 ecoPrimals
//! AUTO-GENERATED from AMD RDNA2 ISA XML specification.
//!
//! Source: specs/amd/amdgpu_isa_rdna2.xml (MIT license, AMD GPUOpen)
//! Generator: tools/amd-isa-gen (pure Rust, sovereign toolchain)
//!
//! DO NOT EDIT BY HAND. Regenerate with:
//!   cargo run -p amd-isa-gen

#![allow(unused_imports)]

use super::isa_types::{BitField, InstrEntry};
mod table_arith;
mod table_cmp_f32_f64;
mod table_cmp_int;
mod table_logic;
mod table_math;

use std::sync::OnceLock;

static TABLE_CACHE: OnceLock<Vec<InstrEntry>> = OnceLock::new();

/// All ENC_VOP3 instructions (combined from sub-tables).
#[must_use]
pub fn table() -> &'static [InstrEntry] {
    TABLE_CACHE
        .get_or_init(|| {
            [
                table_cmp_f32_f64::TABLE,
                table_cmp_int::TABLE,
                table_math::TABLE,
                table_arith::TABLE,
                table_logic::TABLE,
            ]
            .concat()
        })
        .as_slice()
}

/// Look up an instruction by opcode.
#[must_use]
pub fn lookup(opcode: u16) -> Option<&'static InstrEntry> {
    table_cmp_f32_f64::lookup(opcode)
        .or_else(|| table_cmp_int::lookup(opcode))
        .or_else(|| table_math::lookup(opcode))
        .or_else(|| table_arith::lookup(opcode))
        .or_else(|| table_logic::lookup(opcode))
}

/// ENC_VOP3 encoding fields (64 bits).
pub mod fields {
    use super::BitField;
    pub const VDST: BitField = BitField {
        offset: 0,
        width: 8,
    };
    pub const ABS: BitField = BitField {
        offset: 8,
        width: 3,
    };
    pub const OP_SEL: BitField = BitField {
        offset: 11,
        width: 4,
    };
    pub const CLAMP: BitField = BitField {
        offset: 15,
        width: 1,
    };
    pub const OP: BitField = BitField {
        offset: 16,
        width: 10,
    };
    pub const ENCODING: BitField = BitField {
        offset: 26,
        width: 6,
    };
    pub const SRC0: BitField = BitField {
        offset: 32,
        width: 9,
    };
    pub const SRC1: BitField = BitField {
        offset: 41,
        width: 9,
    };
    pub const SRC2: BitField = BitField {
        offset: 50,
        width: 9,
    };
    pub const OMOD: BitField = BitField {
        offset: 59,
        width: 2,
    };
    pub const NEG: BitField = BitField {
        offset: 61,
        width: 3,
    };
}

pub use table_arith::{
    V_ADD_F16, V_ADD_F32, V_ADD_F64, V_ADD_LSHL_U32, V_ADD_NC_I16, V_ADD_NC_I32, V_ADD_NC_U16,
    V_ADD_NC_U32, V_ADD3_U32, V_CLREXCP, V_CNDMASK_B32, V_CUBEID_F32, V_CUBEMA_F32, V_CUBESC_F32,
    V_CUBETC_F32, V_DIV_FIXUP_F16, V_DIV_FIXUP_F32, V_DIV_FIXUP_F64, V_DIV_FMAS_F32,
    V_DIV_FMAS_F64, V_FMA_F16, V_FMA_F32, V_FMA_F64, V_FMA_LEGACY_F32, V_FMAC_F16, V_FMAC_F32,
    V_FMAC_LEGACY_F32, V_INTERP_MOV_F32, V_INTERP_P1_F32, V_INTERP_P1LL_F16, V_INTERP_P1LV_F16,
    V_INTERP_P2_F16, V_INTERP_P2_F32, V_LERP_U8, V_LSHL_ADD_U32, V_MAD_I16, V_MAD_I32_I16,
    V_MAD_I32_I24, V_MAD_U16, V_MAD_U32_U16, V_MAD_U32_U24, V_MAX_F16, V_MAX_F32, V_MAX_F64,
    V_MAX_I16, V_MAX_I32, V_MAX_U16, V_MAX_U32, V_MAX3_F16, V_MAX3_F32, V_MAX3_I16, V_MAX3_I32,
    V_MAX3_U16, V_MAX3_U32, V_MED3_F16, V_MED3_F32, V_MED3_I16, V_MED3_I32, V_MED3_U16, V_MED3_U32,
    V_MIN_F16, V_MIN_F32, V_MIN_F64, V_MIN_I16, V_MIN_I32, V_MIN_U16, V_MIN_U32, V_MIN3_F16,
    V_MIN3_F32, V_MIN3_I16, V_MIN3_I32, V_MIN3_U16, V_MIN3_U32, V_MOV_B32, V_MOVRELD_B32,
    V_MOVRELS_B32, V_MOVRELSD_2_B32, V_MOVRELSD_B32, V_MQSAD_PK_U16_U8, V_MQSAD_U32_U8, V_MSAD_U8,
    V_MUL_F16, V_MUL_F32, V_MUL_F64, V_MUL_HI_I32, V_MUL_HI_I32_I24, V_MUL_HI_U32,
    V_MUL_HI_U32_U24, V_MUL_I32_I24, V_MUL_LEGACY_F32, V_MUL_LO_U16, V_MUL_LO_U32, V_MUL_U32_U24,
    V_MULLIT_F32, V_NOP, V_PACK_B32_F16, V_PIPEFLUSH, V_QSAD_PK_U16_U8, V_READFIRSTLANE_B32,
    V_READLANE_B32, V_SAD_HI_U8, V_SAD_U8, V_SAD_U16, V_SAD_U32, V_SAT_PK_U8_I16, V_SUB_F16,
    V_SUB_F32, V_SUB_NC_I16, V_SUB_NC_I32, V_SUB_NC_U16, V_SUB_NC_U32, V_SUBREV_F16, V_SUBREV_F32,
    V_SUBREV_NC_U32, V_WRITELANE_B32, V_XAD_U32,
};
pub use table_cmp_f32_f64::{
    V_CMP_CLASS_F16, V_CMP_CLASS_F32, V_CMP_CLASS_F64, V_CMP_EQ_F16, V_CMP_EQ_F32, V_CMP_EQ_F64,
    V_CMP_F_F16, V_CMP_F_F32, V_CMP_F_F64, V_CMP_GE_F16, V_CMP_GE_F32, V_CMP_GE_F64, V_CMP_GT_F16,
    V_CMP_GT_F32, V_CMP_GT_F64, V_CMP_LE_F16, V_CMP_LE_F32, V_CMP_LE_F64, V_CMP_LG_F16,
    V_CMP_LG_F32, V_CMP_LG_F64, V_CMP_LT_F16, V_CMP_LT_F32, V_CMP_LT_F64, V_CMP_NEQ_F16,
    V_CMP_NEQ_F32, V_CMP_NEQ_F64, V_CMP_NGE_F16, V_CMP_NGE_F32, V_CMP_NGE_F64, V_CMP_NGT_F16,
    V_CMP_NGT_F32, V_CMP_NGT_F64, V_CMP_NLE_F16, V_CMP_NLE_F32, V_CMP_NLE_F64, V_CMP_NLG_F16,
    V_CMP_NLG_F32, V_CMP_NLG_F64, V_CMP_NLT_F16, V_CMP_NLT_F32, V_CMP_NLT_F64, V_CMP_O_F16,
    V_CMP_O_F32, V_CMP_O_F64, V_CMP_TRU_F16, V_CMP_TRU_F32, V_CMP_TRU_F64, V_CMP_U_F16,
    V_CMP_U_F32, V_CMP_U_F64, V_CMPX_CLASS_F16, V_CMPX_CLASS_F32, V_CMPX_CLASS_F64, V_CMPX_EQ_F16,
    V_CMPX_EQ_F32, V_CMPX_EQ_F64, V_CMPX_F_F16, V_CMPX_F_F32, V_CMPX_F_F64, V_CMPX_GE_F16,
    V_CMPX_GE_F32, V_CMPX_GE_F64, V_CMPX_GT_F16, V_CMPX_GT_F32, V_CMPX_GT_F64, V_CMPX_LE_F16,
    V_CMPX_LE_F32, V_CMPX_LE_F64, V_CMPX_LG_F16, V_CMPX_LG_F32, V_CMPX_LG_F64, V_CMPX_LT_F16,
    V_CMPX_LT_F32, V_CMPX_LT_F64, V_CMPX_NEQ_F16, V_CMPX_NEQ_F32, V_CMPX_NEQ_F64, V_CMPX_NGE_F16,
    V_CMPX_NGE_F32, V_CMPX_NGE_F64, V_CMPX_NGT_F16, V_CMPX_NGT_F32, V_CMPX_NGT_F64, V_CMPX_NLE_F16,
    V_CMPX_NLE_F32, V_CMPX_NLE_F64, V_CMPX_NLG_F16, V_CMPX_NLG_F32, V_CMPX_NLG_F64, V_CMPX_NLT_F16,
    V_CMPX_NLT_F32, V_CMPX_NLT_F64, V_CMPX_O_F16, V_CMPX_O_F32, V_CMPX_O_F64, V_CMPX_TRU_F16,
    V_CMPX_TRU_F32, V_CMPX_TRU_F64, V_CMPX_U_F16, V_CMPX_U_F32, V_CMPX_U_F64,
};
pub use table_cmp_int::{
    V_CMP_EQ_I16, V_CMP_EQ_I32, V_CMP_EQ_I64, V_CMP_EQ_U16, V_CMP_EQ_U32, V_CMP_EQ_U64,
    V_CMP_F_I32, V_CMP_F_I64, V_CMP_F_U32, V_CMP_F_U64, V_CMP_GE_I16, V_CMP_GE_I32, V_CMP_GE_I64,
    V_CMP_GE_U16, V_CMP_GE_U32, V_CMP_GE_U64, V_CMP_GT_I16, V_CMP_GT_I32, V_CMP_GT_I64,
    V_CMP_GT_U16, V_CMP_GT_U32, V_CMP_GT_U64, V_CMP_LE_I16, V_CMP_LE_I32, V_CMP_LE_I64,
    V_CMP_LE_U16, V_CMP_LE_U32, V_CMP_LE_U64, V_CMP_LT_I16, V_CMP_LT_I32, V_CMP_LT_I64,
    V_CMP_LT_U16, V_CMP_LT_U32, V_CMP_LT_U64, V_CMP_NE_I16, V_CMP_NE_I32, V_CMP_NE_I64,
    V_CMP_NE_U16, V_CMP_NE_U32, V_CMP_NE_U64, V_CMP_T_I32, V_CMP_T_I64, V_CMP_T_U32, V_CMP_T_U64,
    V_CMPX_EQ_I16, V_CMPX_EQ_I32, V_CMPX_EQ_I64, V_CMPX_EQ_U16, V_CMPX_EQ_U32, V_CMPX_EQ_U64,
    V_CMPX_F_I32, V_CMPX_F_I64, V_CMPX_F_U32, V_CMPX_F_U64, V_CMPX_GE_I16, V_CMPX_GE_I32,
    V_CMPX_GE_I64, V_CMPX_GE_U16, V_CMPX_GE_U32, V_CMPX_GE_U64, V_CMPX_GT_I16, V_CMPX_GT_I32,
    V_CMPX_GT_I64, V_CMPX_GT_U16, V_CMPX_GT_U32, V_CMPX_GT_U64, V_CMPX_LE_I16, V_CMPX_LE_I32,
    V_CMPX_LE_I64, V_CMPX_LE_U16, V_CMPX_LE_U32, V_CMPX_LE_U64, V_CMPX_LT_I16, V_CMPX_LT_I32,
    V_CMPX_LT_I64, V_CMPX_LT_U16, V_CMPX_LT_U32, V_CMPX_LT_U64, V_CMPX_NE_I16, V_CMPX_NE_I32,
    V_CMPX_NE_I64, V_CMPX_NE_U16, V_CMPX_NE_U32, V_CMPX_NE_U64, V_CMPX_T_I32, V_CMPX_T_I64,
    V_CMPX_T_U32, V_CMPX_T_U64,
};
pub use table_logic::{
    V_ALIGNBIT_B32, V_ALIGNBYTE_B32, V_AND_B32, V_AND_OR_B32, V_ASHRREV_I16, V_ASHRREV_I32,
    V_ASHRREV_I64, V_BCNT_U32_B32, V_BFE_I32, V_BFE_U32, V_BFI_B32, V_BFM_B32, V_BFREV_B32,
    V_FFBH_I32, V_FFBH_U32, V_FFBL_B32, V_LSHL_OR_B32, V_LSHLREV_B16, V_LSHLREV_B32, V_LSHLREV_B64,
    V_LSHRREV_B16, V_LSHRREV_B32, V_LSHRREV_B64, V_MBCNT_HI_U32_B32, V_MBCNT_LO_U32_B32, V_NOT_B32,
    V_OR_B32, V_OR3_B32, V_PERM_B32, V_PERMLANE16_B32, V_PERMLANEX16_B32, V_XNOR_B32, V_XOR_B32,
    V_XOR3_B32,
};
pub use table_math::{
    V_CEIL_F16, V_CEIL_F32, V_CEIL_F64, V_COS_F16, V_COS_F32, V_CVT_F16_F32, V_CVT_F16_I16,
    V_CVT_F16_U16, V_CVT_F32_F16, V_CVT_F32_F64, V_CVT_F32_I32, V_CVT_F32_U32, V_CVT_F32_UBYTE0,
    V_CVT_F32_UBYTE1, V_CVT_F32_UBYTE2, V_CVT_F32_UBYTE3, V_CVT_F64_F32, V_CVT_F64_I32,
    V_CVT_F64_U32, V_CVT_FLR_I32_F32, V_CVT_I16_F16, V_CVT_I32_F32, V_CVT_I32_F64,
    V_CVT_NORM_I16_F16, V_CVT_NORM_U16_F16, V_CVT_OFF_F32_I4, V_CVT_PK_I16_I32, V_CVT_PK_U8_F32,
    V_CVT_PK_U16_U32, V_CVT_PKNORM_I16_F16, V_CVT_PKNORM_I16_F32, V_CVT_PKNORM_U16_F16,
    V_CVT_PKNORM_U16_F32, V_CVT_PKRTZ_F16_F32, V_CVT_RPI_I32_F32, V_CVT_U16_F16, V_CVT_U32_F32,
    V_CVT_U32_F64, V_EXP_F16, V_EXP_F32, V_FLOOR_F16, V_FLOOR_F32, V_FLOOR_F64, V_FRACT_F16,
    V_FRACT_F32, V_FRACT_F64, V_FREXP_EXP_I16_F16, V_FREXP_EXP_I32_F32, V_FREXP_EXP_I32_F64,
    V_FREXP_MANT_F16, V_FREXP_MANT_F32, V_FREXP_MANT_F64, V_LDEXP_F16, V_LDEXP_F32, V_LDEXP_F64,
    V_LOG_F16, V_LOG_F32, V_RCP_F16, V_RCP_F32, V_RCP_F64, V_RCP_IFLAG_F32, V_RNDNE_F16,
    V_RNDNE_F32, V_RNDNE_F64, V_RSQ_F16, V_RSQ_F32, V_RSQ_F64, V_SIN_F16, V_SIN_F32, V_SQRT_F16,
    V_SQRT_F32, V_SQRT_F64, V_TRIG_PREOP_F64, V_TRUNC_F16, V_TRUNC_F32, V_TRUNC_F64,
};
