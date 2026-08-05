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
        name: "V_CMP_F_I32",
        opcode: 128,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMP_LT_I32",
        opcode: 129,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMP_EQ_I32",
        opcode: 130,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMP_LE_I32",
        opcode: 131,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMP_GT_I32",
        opcode: 132,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMP_NE_I32",
        opcode: 133,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMP_GE_I32",
        opcode: 134,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMP_T_I32",
        opcode: 135,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMP_LT_I16",
        opcode: 137,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMP_EQ_I16",
        opcode: 138,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMP_LE_I16",
        opcode: 139,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMP_GT_I16",
        opcode: 140,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMP_NE_I16",
        opcode: 141,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMP_GE_I16",
        opcode: 142,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMPX_F_I32",
        opcode: 144,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMPX_LT_I32",
        opcode: 145,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMPX_EQ_I32",
        opcode: 146,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMPX_LE_I32",
        opcode: 147,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMPX_GT_I32",
        opcode: 148,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMPX_NE_I32",
        opcode: 149,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMPX_GE_I32",
        opcode: 150,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMPX_T_I32",
        opcode: 151,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMPX_LT_I16",
        opcode: 153,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMPX_EQ_I16",
        opcode: 154,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMPX_LE_I16",
        opcode: 155,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMPX_GT_I16",
        opcode: 156,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMPX_NE_I16",
        opcode: 157,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMPX_GE_I16",
        opcode: 158,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMP_F_I64",
        opcode: 160,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMP_LT_I64",
        opcode: 161,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMP_EQ_I64",
        opcode: 162,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMP_LE_I64",
        opcode: 163,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMP_GT_I64",
        opcode: 164,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMP_NE_I64",
        opcode: 165,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMP_GE_I64",
        opcode: 166,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMP_T_I64",
        opcode: 167,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMP_LT_U16",
        opcode: 169,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMP_EQ_U16",
        opcode: 170,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMP_LE_U16",
        opcode: 171,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMP_GT_U16",
        opcode: 172,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMP_NE_U16",
        opcode: 173,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMP_GE_U16",
        opcode: 174,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMPX_F_I64",
        opcode: 176,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMPX_LT_I64",
        opcode: 177,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMPX_EQ_I64",
        opcode: 178,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMPX_LE_I64",
        opcode: 179,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMPX_GT_I64",
        opcode: 180,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMPX_NE_I64",
        opcode: 181,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMPX_GE_I64",
        opcode: 182,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMPX_T_I64",
        opcode: 183,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMPX_LT_U16",
        opcode: 185,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMPX_EQ_U16",
        opcode: 186,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMPX_LE_U16",
        opcode: 187,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMPX_GT_U16",
        opcode: 188,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMPX_NE_U16",
        opcode: 189,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMPX_GE_U16",
        opcode: 190,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMP_F_U32",
        opcode: 192,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMP_LT_U32",
        opcode: 193,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMP_EQ_U32",
        opcode: 194,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMP_LE_U32",
        opcode: 195,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMP_GT_U32",
        opcode: 196,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMP_NE_U32",
        opcode: 197,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMP_GE_U32",
        opcode: 198,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMP_T_U32",
        opcode: 199,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMPX_F_U32",
        opcode: 208,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMPX_LT_U32",
        opcode: 209,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMPX_EQ_U32",
        opcode: 210,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMPX_LE_U32",
        opcode: 211,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMPX_GT_U32",
        opcode: 212,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMPX_NE_U32",
        opcode: 213,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMPX_GE_U32",
        opcode: 214,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMPX_T_U32",
        opcode: 215,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMP_F_U64",
        opcode: 224,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMP_LT_U64",
        opcode: 225,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMP_EQ_U64",
        opcode: 226,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMP_LE_U64",
        opcode: 227,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMP_GT_U64",
        opcode: 228,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMP_NE_U64",
        opcode: 229,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMP_GE_U64",
        opcode: 230,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMP_T_U64",
        opcode: 231,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMPX_F_U64",
        opcode: 240,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMPX_LT_U64",
        opcode: 241,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMPX_EQ_U64",
        opcode: 242,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMPX_LE_U64",
        opcode: 243,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMPX_GT_U64",
        opcode: 244,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMPX_NE_U64",
        opcode: 245,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMPX_GE_U64",
        opcode: 246,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMPX_T_U64",
        opcode: 247,
        is_branch: false,
        is_terminator: false,
    },
];

#[must_use]
pub fn lookup(opcode: u16) -> Option<&'static InstrEntry> {
    TABLE.iter().find(|e| e.opcode == opcode)
}

/// Set the vector condition code to 0. Store the result into VCC or a scalar register.
pub const V_CMP_F_I32: u16 = 128;
/// Set the vector condition code to 1 iff the first input is less than the second input. Store the result into VCC or a ...
pub const V_CMP_LT_I32: u16 = 129;
/// Set the vector condition code to 1 iff the first input is equal to the second input. Store the result into VCC or a s...
pub const V_CMP_EQ_I32: u16 = 130;
/// Set the vector condition code to 1 iff the first input is less than or equal to the second input. Store the result in...
pub const V_CMP_LE_I32: u16 = 131;
/// Set the vector condition code to 1 iff the first input is greater than the second input. Store the result into VCC or...
pub const V_CMP_GT_I32: u16 = 132;
/// Set the vector condition code to 1 iff the first input is not equal to the second input. Store the result into VCC or...
pub const V_CMP_NE_I32: u16 = 133;
/// Set the vector condition code to 1 iff the first input is greater than or equal to the second input. Store the result...
pub const V_CMP_GE_I32: u16 = 134;
/// Set the vector condition code to 1. Store the result into VCC or a scalar register.
pub const V_CMP_T_I32: u16 = 135;
/// Set the vector condition code to 1 iff the first input is less than the second input. Store the result into VCC or a ...
pub const V_CMP_LT_I16: u16 = 137;
/// Set the vector condition code to 1 iff the first input is equal to the second input. Store the result into VCC or a s...
pub const V_CMP_EQ_I16: u16 = 138;
/// Set the vector condition code to 1 iff the first input is less than or equal to the second input. Store the result in...
pub const V_CMP_LE_I16: u16 = 139;
/// Set the vector condition code to 1 iff the first input is greater than the second input. Store the result into VCC or...
pub const V_CMP_GT_I16: u16 = 140;
/// Set the vector condition code to 1 iff the first input is not equal to the second input. Store the result into VCC or...
pub const V_CMP_NE_I16: u16 = 141;
/// Set the vector condition code to 1 iff the first input is greater than or equal to the second input. Store the result...
pub const V_CMP_GE_I16: u16 = 142;
/// Set the vector condition code to 0. Store the result into the EXEC mask.
pub const V_CMPX_F_I32: u16 = 144;
/// Set the vector condition code to 1 iff the first input is less than the second input. Store the result into the EXEC ...
pub const V_CMPX_LT_I32: u16 = 145;
/// Set the vector condition code to 1 iff the first input is equal to the second input. Store the result into the EXEC m...
pub const V_CMPX_EQ_I32: u16 = 146;
/// Set the vector condition code to 1 iff the first input is less than or equal to the second input. Store the result in...
pub const V_CMPX_LE_I32: u16 = 147;
/// Set the vector condition code to 1 iff the first input is greater than the second input. Store the result into the EX...
pub const V_CMPX_GT_I32: u16 = 148;
/// Set the vector condition code to 1 iff the first input is not equal to the second input. Store the result into the EX...
pub const V_CMPX_NE_I32: u16 = 149;
/// Set the vector condition code to 1 iff the first input is greater than or equal to the second input. Store the result...
pub const V_CMPX_GE_I32: u16 = 150;
/// Set the vector condition code to 1. Store the result into the EXEC mask.
pub const V_CMPX_T_I32: u16 = 151;
/// Set the vector condition code to 1 iff the first input is less than the second input. Store the result into the EXEC ...
pub const V_CMPX_LT_I16: u16 = 153;
/// Set the vector condition code to 1 iff the first input is equal to the second input. Store the result into the EXEC m...
pub const V_CMPX_EQ_I16: u16 = 154;
/// Set the vector condition code to 1 iff the first input is less than or equal to the second input. Store the result in...
pub const V_CMPX_LE_I16: u16 = 155;
/// Set the vector condition code to 1 iff the first input is greater than the second input. Store the result into the EX...
pub const V_CMPX_GT_I16: u16 = 156;
/// Set the vector condition code to 1 iff the first input is not equal to the second input. Store the result into the EX...
pub const V_CMPX_NE_I16: u16 = 157;
/// Set the vector condition code to 1 iff the first input is greater than or equal to the second input. Store the result...
pub const V_CMPX_GE_I16: u16 = 158;
/// Set the vector condition code to 0. Store the result into VCC or a scalar register.
pub const V_CMP_F_I64: u16 = 160;
/// Set the vector condition code to 1 iff the first input is less than the second input. Store the result into VCC or a ...
pub const V_CMP_LT_I64: u16 = 161;
/// Set the vector condition code to 1 iff the first input is equal to the second input. Store the result into VCC or a s...
pub const V_CMP_EQ_I64: u16 = 162;
/// Set the vector condition code to 1 iff the first input is less than or equal to the second input. Store the result in...
pub const V_CMP_LE_I64: u16 = 163;
/// Set the vector condition code to 1 iff the first input is greater than the second input. Store the result into VCC or...
pub const V_CMP_GT_I64: u16 = 164;
/// Set the vector condition code to 1 iff the first input is not equal to the second input. Store the result into VCC or...
pub const V_CMP_NE_I64: u16 = 165;
/// Set the vector condition code to 1 iff the first input is greater than or equal to the second input. Store the result...
pub const V_CMP_GE_I64: u16 = 166;
/// Set the vector condition code to 1. Store the result into VCC or a scalar register.
pub const V_CMP_T_I64: u16 = 167;
/// Set the vector condition code to 1 iff the first input is less than the second input. Store the result into VCC or a ...
pub const V_CMP_LT_U16: u16 = 169;
/// Set the vector condition code to 1 iff the first input is equal to the second input. Store the result into VCC or a s...
pub const V_CMP_EQ_U16: u16 = 170;
/// Set the vector condition code to 1 iff the first input is less than or equal to the second input. Store the result in...
pub const V_CMP_LE_U16: u16 = 171;
/// Set the vector condition code to 1 iff the first input is greater than the second input. Store the result into VCC or...
pub const V_CMP_GT_U16: u16 = 172;
/// Set the vector condition code to 1 iff the first input is not equal to the second input. Store the result into VCC or...
pub const V_CMP_NE_U16: u16 = 173;
/// Set the vector condition code to 1 iff the first input is greater than or equal to the second input. Store the result...
pub const V_CMP_GE_U16: u16 = 174;
/// Set the vector condition code to 0. Store the result into the EXEC mask.
pub const V_CMPX_F_I64: u16 = 176;
/// Set the vector condition code to 1 iff the first input is less than the second input. Store the result into the EXEC ...
pub const V_CMPX_LT_I64: u16 = 177;
/// Set the vector condition code to 1 iff the first input is equal to the second input. Store the result into the EXEC m...
pub const V_CMPX_EQ_I64: u16 = 178;
/// Set the vector condition code to 1 iff the first input is less than or equal to the second input. Store the result in...
pub const V_CMPX_LE_I64: u16 = 179;
/// Set the vector condition code to 1 iff the first input is greater than the second input. Store the result into the EX...
pub const V_CMPX_GT_I64: u16 = 180;
/// Set the vector condition code to 1 iff the first input is not equal to the second input. Store the result into the EX...
pub const V_CMPX_NE_I64: u16 = 181;
/// Set the vector condition code to 1 iff the first input is greater than or equal to the second input. Store the result...
pub const V_CMPX_GE_I64: u16 = 182;
/// Set the vector condition code to 1. Store the result into the EXEC mask.
pub const V_CMPX_T_I64: u16 = 183;
/// Set the vector condition code to 1 iff the first input is less than the second input. Store the result into the EXEC ...
pub const V_CMPX_LT_U16: u16 = 185;
/// Set the vector condition code to 1 iff the first input is equal to the second input. Store the result into the EXEC m...
pub const V_CMPX_EQ_U16: u16 = 186;
/// Set the vector condition code to 1 iff the first input is less than or equal to the second input. Store the result in...
pub const V_CMPX_LE_U16: u16 = 187;
/// Set the vector condition code to 1 iff the first input is greater than the second input. Store the result into the EX...
pub const V_CMPX_GT_U16: u16 = 188;
/// Set the vector condition code to 1 iff the first input is not equal to the second input. Store the result into the EX...
pub const V_CMPX_NE_U16: u16 = 189;
/// Set the vector condition code to 1 iff the first input is greater than or equal to the second input. Store the result...
pub const V_CMPX_GE_U16: u16 = 190;
/// Set the vector condition code to 0. Store the result into VCC or a scalar register.
pub const V_CMP_F_U32: u16 = 192;
/// Set the vector condition code to 1 iff the first input is less than the second input. Store the result into VCC or a ...
pub const V_CMP_LT_U32: u16 = 193;
/// Set the vector condition code to 1 iff the first input is equal to the second input. Store the result into VCC or a s...
pub const V_CMP_EQ_U32: u16 = 194;
/// Set the vector condition code to 1 iff the first input is less than or equal to the second input. Store the result in...
pub const V_CMP_LE_U32: u16 = 195;
/// Set the vector condition code to 1 iff the first input is greater than the second input. Store the result into VCC or...
pub const V_CMP_GT_U32: u16 = 196;
/// Set the vector condition code to 1 iff the first input is not equal to the second input. Store the result into VCC or...
pub const V_CMP_NE_U32: u16 = 197;
/// Set the vector condition code to 1 iff the first input is greater than or equal to the second input. Store the result...
pub const V_CMP_GE_U32: u16 = 198;
/// Set the vector condition code to 1. Store the result into VCC or a scalar register.
pub const V_CMP_T_U32: u16 = 199;
/// Set the vector condition code to 0. Store the result into the EXEC mask.
pub const V_CMPX_F_U32: u16 = 208;
/// Set the vector condition code to 1 iff the first input is less than the second input. Store the result into the EXEC ...
pub const V_CMPX_LT_U32: u16 = 209;
/// Set the vector condition code to 1 iff the first input is equal to the second input. Store the result into the EXEC m...
pub const V_CMPX_EQ_U32: u16 = 210;
/// Set the vector condition code to 1 iff the first input is less than or equal to the second input. Store the result in...
pub const V_CMPX_LE_U32: u16 = 211;
/// Set the vector condition code to 1 iff the first input is greater than the second input. Store the result into the EX...
pub const V_CMPX_GT_U32: u16 = 212;
/// Set the vector condition code to 1 iff the first input is not equal to the second input. Store the result into the EX...
pub const V_CMPX_NE_U32: u16 = 213;
/// Set the vector condition code to 1 iff the first input is greater than or equal to the second input. Store the result...
pub const V_CMPX_GE_U32: u16 = 214;
/// Set the vector condition code to 1. Store the result into the EXEC mask.
pub const V_CMPX_T_U32: u16 = 215;
/// Set the vector condition code to 0. Store the result into VCC or a scalar register.
pub const V_CMP_F_U64: u16 = 224;
/// Set the vector condition code to 1 iff the first input is less than the second input. Store the result into VCC or a ...
pub const V_CMP_LT_U64: u16 = 225;
/// Set the vector condition code to 1 iff the first input is equal to the second input. Store the result into VCC or a s...
pub const V_CMP_EQ_U64: u16 = 226;
/// Set the vector condition code to 1 iff the first input is less than or equal to the second input. Store the result in...
pub const V_CMP_LE_U64: u16 = 227;
/// Set the vector condition code to 1 iff the first input is greater than the second input. Store the result into VCC or...
pub const V_CMP_GT_U64: u16 = 228;
/// Set the vector condition code to 1 iff the first input is not equal to the second input. Store the result into VCC or...
pub const V_CMP_NE_U64: u16 = 229;
/// Set the vector condition code to 1 iff the first input is greater than or equal to the second input. Store the result...
pub const V_CMP_GE_U64: u16 = 230;
/// Set the vector condition code to 1. Store the result into VCC or a scalar register.
pub const V_CMP_T_U64: u16 = 231;
/// Set the vector condition code to 0. Store the result into the EXEC mask.
pub const V_CMPX_F_U64: u16 = 240;
/// Set the vector condition code to 1 iff the first input is less than the second input. Store the result into the EXEC ...
pub const V_CMPX_LT_U64: u16 = 241;
/// Set the vector condition code to 1 iff the first input is equal to the second input. Store the result into the EXEC m...
pub const V_CMPX_EQ_U64: u16 = 242;
/// Set the vector condition code to 1 iff the first input is less than or equal to the second input. Store the result in...
pub const V_CMPX_LE_U64: u16 = 243;
/// Set the vector condition code to 1 iff the first input is greater than the second input. Store the result into the EX...
pub const V_CMPX_GT_U64: u16 = 244;
/// Set the vector condition code to 1 iff the first input is not equal to the second input. Store the result into the EX...
pub const V_CMPX_NE_U64: u16 = 245;
/// Set the vector condition code to 1 iff the first input is greater than or equal to the second input. Store the result...
pub const V_CMPX_GE_U64: u16 = 246;
/// Set the vector condition code to 1. Store the result into the EXEC mask.
pub const V_CMPX_T_U64: u16 = 247;
