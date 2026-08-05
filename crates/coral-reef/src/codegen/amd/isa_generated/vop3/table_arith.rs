// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright © 2026 ecoPrimals
//! AUTO-GENERATED from AMD RDNA2 ISA XML specification.
//!
//! Source: specs/amd/amdgpu_isa_rdna2.xml (MIT license, AMD GPUOpen)
//! Generator: tools/amd-isa-gen (pure Rust, sovereign toolchain)
//!
//! DO NOT EDIT BY HAND. Regenerate with:
//!   cargo run -p amd-isa-gen

use super::super::isa_types::InstrEntry;

pub const TABLE: &[InstrEntry] = &[
    InstrEntry {
        name: "V_CNDMASK_B32",
        opcode: 257,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_ADD_F32",
        opcode: 259,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_SUB_F32",
        opcode: 260,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_SUBREV_F32",
        opcode: 261,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_FMAC_LEGACY_F32",
        opcode: 262,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_MUL_LEGACY_F32",
        opcode: 263,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_MUL_F32",
        opcode: 264,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_MUL_I32_I24",
        opcode: 265,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_MUL_HI_I32_I24",
        opcode: 266,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_MUL_U32_U24",
        opcode: 267,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_MUL_HI_U32_U24",
        opcode: 268,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_MIN_F32",
        opcode: 271,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_MAX_F32",
        opcode: 272,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_MIN_I32",
        opcode: 273,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_MAX_I32",
        opcode: 274,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_MIN_U32",
        opcode: 275,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_MAX_U32",
        opcode: 276,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_ADD_NC_U32",
        opcode: 293,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_SUB_NC_U32",
        opcode: 294,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_SUBREV_NC_U32",
        opcode: 295,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_FMAC_F32",
        opcode: 299,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_ADD_F16",
        opcode: 306,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_SUB_F16",
        opcode: 307,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_SUBREV_F16",
        opcode: 308,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_MUL_F16",
        opcode: 309,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_FMAC_F16",
        opcode: 310,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_MAX_F16",
        opcode: 313,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_MIN_F16",
        opcode: 314,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_FMA_LEGACY_F32",
        opcode: 320,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_MAD_I32_I24",
        opcode: 322,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_MAD_U32_U24",
        opcode: 323,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CUBEID_F32",
        opcode: 324,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CUBESC_F32",
        opcode: 325,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CUBETC_F32",
        opcode: 326,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CUBEMA_F32",
        opcode: 327,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_FMA_F32",
        opcode: 331,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_FMA_F64",
        opcode: 332,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_LERP_U8",
        opcode: 333,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_MULLIT_F32",
        opcode: 336,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_MIN3_F32",
        opcode: 337,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_MIN3_I32",
        opcode: 338,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_MIN3_U32",
        opcode: 339,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_MAX3_F32",
        opcode: 340,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_MAX3_I32",
        opcode: 341,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_MAX3_U32",
        opcode: 342,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_MED3_F32",
        opcode: 343,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_MED3_I32",
        opcode: 344,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_MED3_U32",
        opcode: 345,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_SAD_U8",
        opcode: 346,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_SAD_HI_U8",
        opcode: 347,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_SAD_U16",
        opcode: 348,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_SAD_U32",
        opcode: 349,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_DIV_FIXUP_F32",
        opcode: 351,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_DIV_FIXUP_F64",
        opcode: 352,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_ADD_F64",
        opcode: 356,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_MUL_F64",
        opcode: 357,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_MIN_F64",
        opcode: 358,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_MAX_F64",
        opcode: 359,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_MUL_LO_U32",
        opcode: 361,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_MUL_HI_U32",
        opcode: 362,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_MUL_HI_I32",
        opcode: 364,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_DIV_FMAS_F32",
        opcode: 367,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_DIV_FMAS_F64",
        opcode: 368,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_MSAD_U8",
        opcode: 369,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_QSAD_PK_U16_U8",
        opcode: 370,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_MQSAD_PK_U16_U8",
        opcode: 371,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_MQSAD_U32_U8",
        opcode: 373,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_NOP",
        opcode: 384,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_MOV_B32",
        opcode: 385,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_READFIRSTLANE_B32",
        opcode: 386,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_PIPEFLUSH",
        opcode: 411,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CLREXCP",
        opcode: 449,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_MOVRELD_B32",
        opcode: 450,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_MOVRELS_B32",
        opcode: 451,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_MOVRELSD_B32",
        opcode: 452,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_MOVRELSD_2_B32",
        opcode: 456,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_SAT_PK_U8_I16",
        opcode: 482,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_INTERP_P1_F32",
        opcode: 512,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_INTERP_P2_F32",
        opcode: 513,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_INTERP_MOV_F32",
        opcode: 514,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_ADD_NC_U16",
        opcode: 771,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_SUB_NC_U16",
        opcode: 772,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_MUL_LO_U16",
        opcode: 773,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_MAX_U16",
        opcode: 777,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_MAX_I16",
        opcode: 778,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_MIN_U16",
        opcode: 779,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_MIN_I16",
        opcode: 780,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_ADD_NC_I16",
        opcode: 781,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_SUB_NC_I16",
        opcode: 782,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_PACK_B32_F16",
        opcode: 785,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_MAD_U16",
        opcode: 832,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_INTERP_P1LL_F16",
        opcode: 834,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_INTERP_P1LV_F16",
        opcode: 835,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_XAD_U32",
        opcode: 837,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_LSHL_ADD_U32",
        opcode: 838,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_ADD_LSHL_U32",
        opcode: 839,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_FMA_F16",
        opcode: 843,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_MIN3_F16",
        opcode: 849,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_MIN3_I16",
        opcode: 850,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_MIN3_U16",
        opcode: 851,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_MAX3_F16",
        opcode: 852,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_MAX3_I16",
        opcode: 853,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_MAX3_U16",
        opcode: 854,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_MED3_F16",
        opcode: 855,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_MED3_I16",
        opcode: 856,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_MED3_U16",
        opcode: 857,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_INTERP_P2_F16",
        opcode: 858,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_MAD_I16",
        opcode: 862,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_DIV_FIXUP_F16",
        opcode: 863,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_READLANE_B32",
        opcode: 864,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_WRITELANE_B32",
        opcode: 865,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_ADD3_U32",
        opcode: 877,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_MAD_U32_U16",
        opcode: 883,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_MAD_I32_I16",
        opcode: 885,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_SUB_NC_I32",
        opcode: 886,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_ADD_NC_I32",
        opcode: 895,
        is_branch: false,
        is_terminator: false,
    },
];

#[must_use]
pub fn lookup(opcode: u16) -> Option<&'static InstrEntry> {
    TABLE.iter().find(|e| e.opcode == opcode)
}

/// Copy data from one of two inputs based on the vector condition code and store the result into a vector register.
pub const V_CNDMASK_B32: u16 = 257;
/// Add two floating point inputs and store the result into a vector register.
pub const V_ADD_F32: u16 = 259;
/// Subtract the second floating point input from the first input and store the result into a vector register.
pub const V_SUB_F32: u16 = 260;
/// Subtract the first floating point input from the second input and store the result into a vector register.
pub const V_SUBREV_F32: u16 = 261;
/// Multiply two single-precision values and accumulate the result with the destination. Follows DX9 rules where 0.0 time...
pub const V_FMAC_LEGACY_F32: u16 = 262;
/// Multiply two floating point inputs and store the result in a vector register. Follows DX9 rules where 0.0 times anyth...
pub const V_MUL_LEGACY_F32: u16 = 263;
/// Multiply two floating point inputs and store the result into a vector register.
pub const V_MUL_F32: u16 = 264;
/// Multiply two signed 24-bit integer inputs and store the result as a signed 32-bit integer into a vector register.
pub const V_MUL_I32_I24: u16 = 265;
/// Multiply two signed 24-bit integer inputs and store the high 32 bits of the result as a signed 32-bit integer into a ...
pub const V_MUL_HI_I32_I24: u16 = 266;
/// Multiply two unsigned 24-bit integer inputs and store the result as an unsigned 32-bit integer into a vector register.
pub const V_MUL_U32_U24: u16 = 267;
/// Multiply two unsigned 24-bit integer inputs and store the high 32 bits of the result as an unsigned 32-bit integer in...
pub const V_MUL_HI_U32_U24: u16 = 268;
/// Select the minimum of two single-precision float inputs and store the result into a vector register.
pub const V_MIN_F32: u16 = 271;
/// Select the maximum of two single-precision float inputs and store the result into a vector register.
pub const V_MAX_F32: u16 = 272;
/// Select the minimum of two signed 32-bit integer inputs and store the selected value into a vector register.
pub const V_MIN_I32: u16 = 273;
/// Select the maximum of two signed 32-bit integer inputs and store the selected value into a vector register.
pub const V_MAX_I32: u16 = 274;
/// Select the minimum of two unsigned 32-bit integer inputs and store the selected value into a vector register.
pub const V_MIN_U32: u16 = 275;
/// Select the maximum of two unsigned 32-bit integer inputs and store the selected value into a vector register.
pub const V_MAX_U32: u16 = 276;
/// Add two unsigned 32-bit integer inputs and store the result into a vector register. No carry-in or carry-out support.
pub const V_ADD_NC_U32: u16 = 293;
/// Subtract the second unsigned input from the first input and store the result into a vector register. No carry-in or c...
pub const V_SUB_NC_U32: u16 = 294;
/// Subtract the first unsigned input from the second input and store the result into a vector register. No carry-in or c...
pub const V_SUBREV_NC_U32: u16 = 295;
/// Multiply two floating point inputs and accumulate the result into the destination register using fused multiply add.
pub const V_FMAC_F32: u16 = 299;
/// Add two floating point inputs and store the result into a vector register.
pub const V_ADD_F16: u16 = 306;
/// Subtract the second floating point input from the first input and store the result into a vector register.
pub const V_SUB_F16: u16 = 307;
/// Subtract the first floating point input from the second input and store the result into a vector register.
pub const V_SUBREV_F16: u16 = 308;
/// Multiply two floating point inputs and store the result into a vector register.
pub const V_MUL_F16: u16 = 309;
/// Multiply two floating point inputs and accumulate the result into the destination register using fused multiply add.
pub const V_FMAC_F16: u16 = 310;
/// Select the maximum of two half-precision float inputs and store the result into a vector register.
pub const V_MAX_F16: u16 = 313;
/// Select the minimum of two half-precision float inputs and store the result into a vector register.
pub const V_MIN_F16: u16 = 314;
/// Multiply and add single-precision values. Follows DX9 rules where 0.0 times anything produces 0.0.
pub const V_FMA_LEGACY_F32: u16 = 320;
/// Multiply two signed 24-bit integer inputs in the signed 32-bit integer domain, add a signed 32-bit integer value from...
pub const V_MAD_I32_I24: u16 = 322;
/// Multiply two unsigned 24-bit integer inputs in the unsigned 32-bit integer domain, add a unsigned 32-bit integer valu...
pub const V_MAD_U32_U24: u16 = 323;
/// Compute the cubemap face ID of a 3D coordinate specified as three single-precision float inputs. Store the result in ...
pub const V_CUBEID_F32: u16 = 324;
/// Compute the cubemap S coordinate of a 3D coordinate specified as three single-precision float inputs. Store the resul...
pub const V_CUBESC_F32: u16 = 325;
/// Compute the cubemap T coordinate of a 3D coordinate specified as three single-precision float inputs. Store the resul...
pub const V_CUBETC_F32: u16 = 326;
/// Compute the cubemap major axis coordinate of a 3D coordinate specified as three single-precision float inputs. Store ...
pub const V_CUBEMA_F32: u16 = 327;
/// Multiply two single-precision float inputs and add a third input using fused multiply add, and store the result into ...
pub const V_FMA_F32: u16 = 331;
/// Multiply two double-precision float inputs and add a third input using fused multiply add, and store the result into ...
pub const V_FMA_F64: u16 = 332;
/// Average two 4-D vectors stored as packed bytes in the first two inputs with rounding control provided by the third in...
pub const V_LERP_U8: u16 = 333;
/// Multiply two floating point inputs and store the result into a vector register. Specific rules apply to accommodate l...
pub const V_MULLIT_F32: u16 = 336;
/// Select the minimum of three single-precision float inputs and store the selected value into a vector register.
pub const V_MIN3_F32: u16 = 337;
/// Select the minimum of three signed 32-bit integer inputs and store the selected value into a vector register.
pub const V_MIN3_I32: u16 = 338;
/// Select the minimum of three unsigned 32-bit integer inputs and store the selected value into a vector register.
pub const V_MIN3_U32: u16 = 339;
/// Select the maximum of three single-precision float inputs and store the selected value into a vector register.
pub const V_MAX3_F32: u16 = 340;
/// Select the maximum of three signed 32-bit integer inputs and store the selected value into a vector register.
pub const V_MAX3_I32: u16 = 341;
/// Select the maximum of three unsigned 32-bit integer inputs and store the selected value into a vector register.
pub const V_MAX3_U32: u16 = 342;
/// Select the median of three single-precision float values and store the selected value into a vector register.
pub const V_MED3_F32: u16 = 343;
/// Select the median of three signed 32-bit integer values and store the selected value into a vector register.
pub const V_MED3_I32: u16 = 344;
/// Select the median of three unsigned 32-bit integer values and store the selected value into a vector register.
pub const V_MED3_U32: u16 = 345;
/// Calculate the sum of absolute differences of elements in two packed 4-component unsigned 8-bit integer inputs, add an...
pub const V_SAD_U8: u16 = 346;
/// Calculate the sum of absolute differences of elements in two packed 4-component unsigned 8-bit integer inputs, shift ...
pub const V_SAD_HI_U8: u16 = 347;
/// Calculate the sum of absolute differences of elements in two packed 2-component unsigned 16-bit integer inputs, add a...
pub const V_SAD_U16: u16 = 348;
/// Calculate the absolute difference of two unsigned 32-bit integer inputs, add an unsigned 32-bit integer value from th...
pub const V_SAD_U32: u16 = 349;
/// Given a single-precision float quotient in the first input, a denominator in the second input and a numerator in the ...
pub const V_DIV_FIXUP_F32: u16 = 351;
/// Given a double-precision float quotient in the first input, a denominator in the second input and a numerator in the ...
pub const V_DIV_FIXUP_F64: u16 = 352;
/// Add two floating point inputs and store the result into a vector register.
pub const V_ADD_F64: u16 = 356;
/// Multiply two floating point inputs and store the result into a vector register.
pub const V_MUL_F64: u16 = 357;
/// Select the minimum of two double-precision float inputs and store the result into a vector register.
pub const V_MIN_F64: u16 = 358;
/// Select the maximum of two double-precision float inputs and store the result into a vector register.
pub const V_MAX_F64: u16 = 359;
/// Multiply two unsigned 32-bit integer inputs and store the result into a vector register.
pub const V_MUL_LO_U32: u16 = 361;
/// Multiply two unsigned 32-bit integer inputs and store the high 32 bits of the result into a vector register.
pub const V_MUL_HI_U32: u16 = 362;
/// Multiply two signed 32-bit integer inputs and store the high 32 bits of the result into a vector register.
pub const V_MUL_HI_I32: u16 = 364;
/// Multiply two single-precision float inputs and add a third input using fused multiply add, then scale the exponent of...
pub const V_DIV_FMAS_F32: u16 = 367;
/// Multiply two double-precision float inputs and add a third input using fused multiply add, then scale the exponent of...
pub const V_DIV_FMAS_F64: u16 = 368;
/// Calculate the sum of absolute differences of elements in two packed 4-component unsigned 8-bit integer inputs, except...
pub const V_MSAD_U8: u16 = 369;
/// Perform the V_SAD_U8 operation four times using different slices of the first array, all entries of the second array ...
pub const V_QSAD_PK_U16_U8: u16 = 370;
/// Perform the V_MSAD_U8 operation four times using different slices of the first array, all entries of the second array...
pub const V_MQSAD_PK_U16_U8: u16 = 371;
/// Perform the V_MSAD_U8 operation four times using different slices of the first array, all entries of the second array...
pub const V_MQSAD_U32_U8: u16 = 373;
/// Do nothing.
pub const V_NOP: u16 = 384;
/// Move data from a vector input into a vector register.
pub const V_MOV_B32: u16 = 385;
/// Read the scalar value in the lowest active lane of the input vector register and store it into a scalar register.
pub const V_READFIRSTLANE_B32: u16 = 386;
/// Flush the vector ALU pipeline through the destination cache.
pub const V_PIPEFLUSH: u16 = 411;
/// Clear this wave's exception state in the vector ALU.
pub const V_CLREXCP: u16 = 449;
/// Move data from a vector input into a relatively-indexed vector register.
pub const V_MOVRELD_B32: u16 = 450;
/// Move data from a relatively-indexed vector register into another vector register.
pub const V_MOVRELS_B32: u16 = 451;
/// Move data from a relatively-indexed vector register into another relatively-indexed vector register.
pub const V_MOVRELSD_B32: u16 = 452;
/// Move data from a relatively-indexed vector register into another relatively-indexed vector register, using different ...
pub const V_MOVRELSD_2_B32: u16 = 456;
/// Given two 16-bit unsigned integer inputs, saturate each input over an 8-bit unsigned range, pack the resulting values...
pub const V_SAT_PK_U8_I16: u16 = 482;
/// Given the I coordinate in a vector register and an attribute specifier, load parameter data from the local data share...
pub const V_INTERP_P1_F32: u16 = 512;
/// Given the J coordinate in a vector register, an attribute specifier and the result of a prior V_INTERP_P1_F32 in the ...
pub const V_INTERP_P2_F32: u16 = 513;
/// Given an attribute specifier and a parameter ID (P0, P10 or P20), load one of the parameter values from the local dat...
pub const V_INTERP_MOV_F32: u16 = 514;
/// Add two unsigned 16-bit integer inputs and store the result into a vector register. No carry-in or carry-out support.
pub const V_ADD_NC_U16: u16 = 771;
/// Subtract the second unsigned input from the first input and store the result into a vector register. No carry-in or c...
pub const V_SUB_NC_U16: u16 = 772;
/// Multiply two unsigned 16-bit integer inputs and store the low bits of the result into a vector register.
pub const V_MUL_LO_U16: u16 = 773;
/// Select the maximum of two unsigned 16-bit integer inputs and store the selected value into a vector register.
pub const V_MAX_U16: u16 = 777;
/// Select the maximum of two signed 16-bit integer inputs and store the selected value into a vector register.
pub const V_MAX_I16: u16 = 778;
/// Select the minimum of two unsigned 16-bit integer inputs and store the selected value into a vector register.
pub const V_MIN_U16: u16 = 779;
/// Select the minimum of two signed 16-bit integer inputs and store the selected value into a vector register.
pub const V_MIN_I16: u16 = 780;
/// Add two signed 16-bit integer inputs and store the result into a vector register. No carry-in or carry-out support.
pub const V_ADD_NC_I16: u16 = 781;
/// Subtract the second signed input from the first input and store the result into a vector register. No carry-in or car...
pub const V_SUB_NC_I16: u16 = 782;
/// Pack two half-precision float values into a single 32-bit value and store the result into a vector register.
pub const V_PACK_B32_F16: u16 = 785;
/// Multiply two unsigned 16-bit integer inputs, add an unsigned 16-bit integer value from a third input, and store the r...
pub const V_MAD_U16: u16 = 832;
/// Given a single-precision float I coordinate in a vector register and an attribute specifier, load two half-precision ...
pub const V_INTERP_P1LL_F16: u16 = 834;
/// Given a single-precision float I coordinate in a vector register, a half-precision float P0 value in another vector r...
pub const V_INTERP_P1LV_F16: u16 = 835;
/// Calculate bitwise XOR of the first two vector inputs, then add the third vector input to the intermediate result, the...
pub const V_XAD_U32: u16 = 837;
/// Given a shift count in the second input, calculate the logical shift left of the first input, then add the third inpu...
pub const V_LSHL_ADD_U32: u16 = 838;
/// Add the first two integer inputs, then given a shift count in the third input, calculate the logical shift left of th...
pub const V_ADD_LSHL_U32: u16 = 839;
/// Multiply two half-precision float inputs and add a third input using fused multiply add, and store the result into a ...
pub const V_FMA_F16: u16 = 843;
/// Select the minimum of three half-precision float inputs and store the selected value into a vector register.
pub const V_MIN3_F16: u16 = 849;
/// Select the minimum of three signed 16-bit integer inputs and store the selected value into a vector register.
pub const V_MIN3_I16: u16 = 850;
/// Select the minimum of three unsigned 16-bit integer inputs and store the selected value into a vector register.
pub const V_MIN3_U16: u16 = 851;
/// Select the maximum of three half-precision float inputs and store the selected value into a vector register.
pub const V_MAX3_F16: u16 = 852;
/// Select the maximum of three signed 16-bit integer inputs and store the selected value into a vector register.
pub const V_MAX3_I16: u16 = 853;
/// Select the maximum of three unsigned 16-bit integer inputs and store the selected value into a vector register.
pub const V_MAX3_U16: u16 = 854;
/// Select the median of three half-precision float values and store the selected value into a vector register.
pub const V_MED3_F16: u16 = 855;
/// Select the median of three signed 16-bit integer values and store the selected value into a vector register.
pub const V_MED3_I16: u16 = 856;
/// Select the median of three unsigned 16-bit integer values and store the selected value into a vector register.
pub const V_MED3_U16: u16 = 857;
/// Given a single-precision float J coordinate in a vector register, an attribute specifier and the result of a prior V_...
pub const V_INTERP_P2_F16: u16 = 858;
/// Multiply two signed 16-bit integer inputs, add a signed 16-bit integer value from a third input, and store the result...
pub const V_MAD_I16: u16 = 862;
/// Given a half-precision float quotient in the first input, a denominator in the second input and a numerator in the th...
pub const V_DIV_FIXUP_F16: u16 = 863;
/// Read the scalar value in the specified lane of the first input where the lane select is in the second input. Store th...
pub const V_READLANE_B32: u16 = 864;
/// Write the scalar value in the first input into the specified lane of a vector register where the lane select is in th...
pub const V_WRITELANE_B32: u16 = 865;
/// Add three unsigned inputs and store the result into a vector register. No carry-in or carry-out support.
pub const V_ADD3_U32: u16 = 877;
/// Multiply two unsigned 16-bit integer inputs in the unsigned 32-bit integer domain, add an unsigned 32-bit integer val...
pub const V_MAD_U32_U16: u16 = 883;
/// Multiply two signed 16-bit integer inputs in the signed 32-bit integer domain, add a signed 32-bit integer value from...
pub const V_MAD_I32_I16: u16 = 885;
/// Subtract the second signed input from the first input and store the result into a vector register. No carry-in or car...
pub const V_SUB_NC_I32: u16 = 886;
/// Add two signed 32-bit integer inputs and store the result into a vector register. No carry-in or carry-out support.
pub const V_ADD_NC_I32: u16 = 895;
