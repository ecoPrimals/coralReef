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
        name: "V_CMP_F_F32",
        opcode: 0,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMP_LT_F32",
        opcode: 1,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMP_EQ_F32",
        opcode: 2,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMP_LE_F32",
        opcode: 3,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMP_GT_F32",
        opcode: 4,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMP_LG_F32",
        opcode: 5,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMP_GE_F32",
        opcode: 6,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMP_O_F32",
        opcode: 7,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMP_U_F32",
        opcode: 8,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMP_NGE_F32",
        opcode: 9,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMP_NLG_F32",
        opcode: 10,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMP_NGT_F32",
        opcode: 11,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMP_NLE_F32",
        opcode: 12,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMP_NEQ_F32",
        opcode: 13,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMP_NLT_F32",
        opcode: 14,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMP_TRU_F32",
        opcode: 15,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMPX_F_F32",
        opcode: 16,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMPX_LT_F32",
        opcode: 17,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMPX_EQ_F32",
        opcode: 18,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMPX_LE_F32",
        opcode: 19,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMPX_GT_F32",
        opcode: 20,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMPX_LG_F32",
        opcode: 21,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMPX_GE_F32",
        opcode: 22,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMPX_O_F32",
        opcode: 23,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMPX_U_F32",
        opcode: 24,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMPX_NGE_F32",
        opcode: 25,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMPX_NLG_F32",
        opcode: 26,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMPX_NGT_F32",
        opcode: 27,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMPX_NLE_F32",
        opcode: 28,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMPX_NEQ_F32",
        opcode: 29,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMPX_NLT_F32",
        opcode: 30,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMPX_TRU_F32",
        opcode: 31,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMP_F_F64",
        opcode: 32,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMP_LT_F64",
        opcode: 33,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMP_EQ_F64",
        opcode: 34,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMP_LE_F64",
        opcode: 35,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMP_GT_F64",
        opcode: 36,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMP_LG_F64",
        opcode: 37,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMP_GE_F64",
        opcode: 38,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMP_O_F64",
        opcode: 39,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMP_U_F64",
        opcode: 40,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMP_NGE_F64",
        opcode: 41,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMP_NLG_F64",
        opcode: 42,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMP_NGT_F64",
        opcode: 43,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMP_NLE_F64",
        opcode: 44,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMP_NEQ_F64",
        opcode: 45,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMP_NLT_F64",
        opcode: 46,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMP_TRU_F64",
        opcode: 47,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMPX_F_F64",
        opcode: 48,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMPX_LT_F64",
        opcode: 49,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMPX_EQ_F64",
        opcode: 50,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMPX_LE_F64",
        opcode: 51,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMPX_GT_F64",
        opcode: 52,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMPX_LG_F64",
        opcode: 53,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMPX_GE_F64",
        opcode: 54,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMPX_O_F64",
        opcode: 55,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMPX_U_F64",
        opcode: 56,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMPX_NGE_F64",
        opcode: 57,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMPX_NLG_F64",
        opcode: 58,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMPX_NGT_F64",
        opcode: 59,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMPX_NLE_F64",
        opcode: 60,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMPX_NEQ_F64",
        opcode: 61,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMPX_NLT_F64",
        opcode: 62,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMPX_TRU_F64",
        opcode: 63,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMP_CLASS_F32",
        opcode: 136,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMP_CLASS_F16",
        opcode: 143,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMPX_CLASS_F32",
        opcode: 152,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMPX_CLASS_F16",
        opcode: 159,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMP_CLASS_F64",
        opcode: 168,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMPX_CLASS_F64",
        opcode: 184,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMP_F_F16",
        opcode: 200,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMP_LT_F16",
        opcode: 201,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMP_EQ_F16",
        opcode: 202,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMP_LE_F16",
        opcode: 203,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMP_GT_F16",
        opcode: 204,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMP_LG_F16",
        opcode: 205,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMP_GE_F16",
        opcode: 206,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMP_O_F16",
        opcode: 207,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMPX_F_F16",
        opcode: 216,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMPX_LT_F16",
        opcode: 217,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMPX_EQ_F16",
        opcode: 218,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMPX_LE_F16",
        opcode: 219,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMPX_GT_F16",
        opcode: 220,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMPX_LG_F16",
        opcode: 221,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMPX_GE_F16",
        opcode: 222,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMPX_O_F16",
        opcode: 223,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMP_U_F16",
        opcode: 232,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMP_NGE_F16",
        opcode: 233,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMP_NLG_F16",
        opcode: 234,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMP_NGT_F16",
        opcode: 235,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMP_NLE_F16",
        opcode: 236,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMP_NEQ_F16",
        opcode: 237,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMP_NLT_F16",
        opcode: 238,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMP_TRU_F16",
        opcode: 239,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMPX_U_F16",
        opcode: 248,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMPX_NGE_F16",
        opcode: 249,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMPX_NLG_F16",
        opcode: 250,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMPX_NGT_F16",
        opcode: 251,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMPX_NLE_F16",
        opcode: 252,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMPX_NEQ_F16",
        opcode: 253,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMPX_NLT_F16",
        opcode: 254,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CMPX_TRU_F16",
        opcode: 255,
        is_branch: false,
        is_terminator: false,
    },
];

#[must_use]
pub fn lookup(opcode: u16) -> Option<&'static InstrEntry> {
    TABLE.iter().find(|e| e.opcode == opcode)
}

/// Set the vector condition code to 0. Store the result into VCC or a scalar register.
pub const V_CMP_F_F32: u16 = 0;
/// Set the vector condition code to 1 iff the first input is less than the second input. Store the result into VCC or a ...
pub const V_CMP_LT_F32: u16 = 1;
/// Set the vector condition code to 1 iff the first input is equal to the second input. Store the result into VCC or a s...
pub const V_CMP_EQ_F32: u16 = 2;
/// Set the vector condition code to 1 iff the first input is less than or equal to the second input. Store the result in...
pub const V_CMP_LE_F32: u16 = 3;
/// Set the vector condition code to 1 iff the first input is greater than the second input. Store the result into VCC or...
pub const V_CMP_GT_F32: u16 = 4;
/// Set the vector condition code to 1 iff the first input is less than or greater than the second input. Store the resul...
pub const V_CMP_LG_F32: u16 = 5;
/// Set the vector condition code to 1 iff the first input is greater than or equal to the second input. Store the result...
pub const V_CMP_GE_F32: u16 = 6;
/// Set the vector condition code to 1 iff the first input is orderable to the second input. Store the result into VCC or...
pub const V_CMP_O_F32: u16 = 7;
/// Set the vector condition code to 1 iff the first input is not orderable to the second input. Store the result into VC...
pub const V_CMP_U_F32: u16 = 8;
/// Set the vector condition code to 1 iff the first input is not greater than or equal to the second input. Store the re...
pub const V_CMP_NGE_F32: u16 = 9;
/// Set the vector condition code to 1 iff the first input is not less than or greater than the second input. Store the r...
pub const V_CMP_NLG_F32: u16 = 10;
/// Set the vector condition code to 1 iff the first input is not greater than the second input. Store the result into VC...
pub const V_CMP_NGT_F32: u16 = 11;
/// Set the vector condition code to 1 iff the first input is not less than or equal to the second input. Store the resul...
pub const V_CMP_NLE_F32: u16 = 12;
/// Set the vector condition code to 1 iff the first input is not equal to the second input. Store the result into VCC or...
pub const V_CMP_NEQ_F32: u16 = 13;
/// Set the vector condition code to 1 iff the first input is not less than the second input. Store the result into VCC o...
pub const V_CMP_NLT_F32: u16 = 14;
/// Set the vector condition code to 1. Store the result into VCC or a scalar register.
pub const V_CMP_TRU_F32: u16 = 15;
/// Set the vector condition code to 0. Store the result into the EXEC mask.
pub const V_CMPX_F_F32: u16 = 16;
/// Set the vector condition code to 1 iff the first input is less than the second input. Store the result into the EXEC ...
pub const V_CMPX_LT_F32: u16 = 17;
/// Set the vector condition code to 1 iff the first input is equal to the second input. Store the result into the EXEC m...
pub const V_CMPX_EQ_F32: u16 = 18;
/// Set the vector condition code to 1 iff the first input is less than or equal to the second input. Store the result in...
pub const V_CMPX_LE_F32: u16 = 19;
/// Set the vector condition code to 1 iff the first input is greater than the second input. Store the result into the EX...
pub const V_CMPX_GT_F32: u16 = 20;
/// Set the vector condition code to 1 iff the first input is less than or greater than the second input. Store the resul...
pub const V_CMPX_LG_F32: u16 = 21;
/// Set the vector condition code to 1 iff the first input is greater than or equal to the second input. Store the result...
pub const V_CMPX_GE_F32: u16 = 22;
/// Set the vector condition code to 1 iff the first input is orderable to the second input. Store the result into the EX...
pub const V_CMPX_O_F32: u16 = 23;
/// Set the vector condition code to 1 iff the first input is not orderable to the second input. Store the result into th...
pub const V_CMPX_U_F32: u16 = 24;
/// Set the vector condition code to 1 iff the first input is not greater than or equal to the second input. Store the re...
pub const V_CMPX_NGE_F32: u16 = 25;
/// Set the vector condition code to 1 iff the first input is not less than or greater than the second input. Store the r...
pub const V_CMPX_NLG_F32: u16 = 26;
/// Set the vector condition code to 1 iff the first input is not greater than the second input. Store the result into th...
pub const V_CMPX_NGT_F32: u16 = 27;
/// Set the vector condition code to 1 iff the first input is not less than or equal to the second input. Store the resul...
pub const V_CMPX_NLE_F32: u16 = 28;
/// Set the vector condition code to 1 iff the first input is not equal to the second input. Store the result into the EX...
pub const V_CMPX_NEQ_F32: u16 = 29;
/// Set the vector condition code to 1 iff the first input is not less than the second input. Store the result into the E...
pub const V_CMPX_NLT_F32: u16 = 30;
/// Set the vector condition code to 1. Store the result into the EXEC mask.
pub const V_CMPX_TRU_F32: u16 = 31;
/// Set the vector condition code to 0. Store the result into VCC or a scalar register.
pub const V_CMP_F_F64: u16 = 32;
/// Set the vector condition code to 1 iff the first input is less than the second input. Store the result into VCC or a ...
pub const V_CMP_LT_F64: u16 = 33;
/// Set the vector condition code to 1 iff the first input is equal to the second input. Store the result into VCC or a s...
pub const V_CMP_EQ_F64: u16 = 34;
/// Set the vector condition code to 1 iff the first input is less than or equal to the second input. Store the result in...
pub const V_CMP_LE_F64: u16 = 35;
/// Set the vector condition code to 1 iff the first input is greater than the second input. Store the result into VCC or...
pub const V_CMP_GT_F64: u16 = 36;
/// Set the vector condition code to 1 iff the first input is less than or greater than the second input. Store the resul...
pub const V_CMP_LG_F64: u16 = 37;
/// Set the vector condition code to 1 iff the first input is greater than or equal to the second input. Store the result...
pub const V_CMP_GE_F64: u16 = 38;
/// Set the vector condition code to 1 iff the first input is orderable to the second input. Store the result into VCC or...
pub const V_CMP_O_F64: u16 = 39;
/// Set the vector condition code to 1 iff the first input is not orderable to the second input. Store the result into VC...
pub const V_CMP_U_F64: u16 = 40;
/// Set the vector condition code to 1 iff the first input is not greater than or equal to the second input. Store the re...
pub const V_CMP_NGE_F64: u16 = 41;
/// Set the vector condition code to 1 iff the first input is not less than or greater than the second input. Store the r...
pub const V_CMP_NLG_F64: u16 = 42;
/// Set the vector condition code to 1 iff the first input is not greater than the second input. Store the result into VC...
pub const V_CMP_NGT_F64: u16 = 43;
/// Set the vector condition code to 1 iff the first input is not less than or equal to the second input. Store the resul...
pub const V_CMP_NLE_F64: u16 = 44;
/// Set the vector condition code to 1 iff the first input is not equal to the second input. Store the result into VCC or...
pub const V_CMP_NEQ_F64: u16 = 45;
/// Set the vector condition code to 1 iff the first input is not less than the second input. Store the result into VCC o...
pub const V_CMP_NLT_F64: u16 = 46;
/// Set the vector condition code to 1. Store the result into VCC or a scalar register.
pub const V_CMP_TRU_F64: u16 = 47;
/// Set the vector condition code to 0. Store the result into the EXEC mask.
pub const V_CMPX_F_F64: u16 = 48;
/// Set the vector condition code to 1 iff the first input is less than the second input. Store the result into the EXEC ...
pub const V_CMPX_LT_F64: u16 = 49;
/// Set the vector condition code to 1 iff the first input is equal to the second input. Store the result into the EXEC m...
pub const V_CMPX_EQ_F64: u16 = 50;
/// Set the vector condition code to 1 iff the first input is less than or equal to the second input. Store the result in...
pub const V_CMPX_LE_F64: u16 = 51;
/// Set the vector condition code to 1 iff the first input is greater than the second input. Store the result into the EX...
pub const V_CMPX_GT_F64: u16 = 52;
/// Set the vector condition code to 1 iff the first input is less than or greater than the second input. Store the resul...
pub const V_CMPX_LG_F64: u16 = 53;
/// Set the vector condition code to 1 iff the first input is greater than or equal to the second input. Store the result...
pub const V_CMPX_GE_F64: u16 = 54;
/// Set the vector condition code to 1 iff the first input is orderable to the second input. Store the result into the EX...
pub const V_CMPX_O_F64: u16 = 55;
/// Set the vector condition code to 1 iff the first input is not orderable to the second input. Store the result into th...
pub const V_CMPX_U_F64: u16 = 56;
/// Set the vector condition code to 1 iff the first input is not greater than or equal to the second input. Store the re...
pub const V_CMPX_NGE_F64: u16 = 57;
/// Set the vector condition code to 1 iff the first input is not less than or greater than the second input. Store the r...
pub const V_CMPX_NLG_F64: u16 = 58;
/// Set the vector condition code to 1 iff the first input is not greater than the second input. Store the result into th...
pub const V_CMPX_NGT_F64: u16 = 59;
/// Set the vector condition code to 1 iff the first input is not less than or equal to the second input. Store the resul...
pub const V_CMPX_NLE_F64: u16 = 60;
/// Set the vector condition code to 1 iff the first input is not equal to the second input. Store the result into the EX...
pub const V_CMPX_NEQ_F64: u16 = 61;
/// Set the vector condition code to 1 iff the first input is not less than the second input. Store the result into the E...
pub const V_CMPX_NLT_F64: u16 = 62;
/// Set the vector condition code to 1. Store the result into the EXEC mask.
pub const V_CMPX_TRU_F64: u16 = 63;
/// Evaluate the IEEE numeric class function specified as a 10 bit mask in the second input on the first input, a single-...
pub const V_CMP_CLASS_F32: u16 = 136;
/// Evaluate the IEEE numeric class function specified as a 10 bit mask in the second input on the first input, a half-pr...
pub const V_CMP_CLASS_F16: u16 = 143;
/// Evaluate the IEEE numeric class function specified as a 10 bit mask in the second input on the first input, a single-...
pub const V_CMPX_CLASS_F32: u16 = 152;
/// Evaluate the IEEE numeric class function specified as a 10 bit mask in the second input on the first input, a half-pr...
pub const V_CMPX_CLASS_F16: u16 = 159;
/// Evaluate the IEEE numeric class function specified as a 10 bit mask in the second input on the first input, a double-...
pub const V_CMP_CLASS_F64: u16 = 168;
/// Evaluate the IEEE numeric class function specified as a 10 bit mask in the second input on the first input, a double-...
pub const V_CMPX_CLASS_F64: u16 = 184;
/// Set the vector condition code to 0. Store the result into VCC or a scalar register.
pub const V_CMP_F_F16: u16 = 200;
/// Set the vector condition code to 1 iff the first input is less than the second input. Store the result into VCC or a ...
pub const V_CMP_LT_F16: u16 = 201;
/// Set the vector condition code to 1 iff the first input is equal to the second input. Store the result into VCC or a s...
pub const V_CMP_EQ_F16: u16 = 202;
/// Set the vector condition code to 1 iff the first input is less than or equal to the second input. Store the result in...
pub const V_CMP_LE_F16: u16 = 203;
/// Set the vector condition code to 1 iff the first input is greater than the second input. Store the result into VCC or...
pub const V_CMP_GT_F16: u16 = 204;
/// Set the vector condition code to 1 iff the first input is less than or greater than the second input. Store the resul...
pub const V_CMP_LG_F16: u16 = 205;
/// Set the vector condition code to 1 iff the first input is greater than or equal to the second input. Store the result...
pub const V_CMP_GE_F16: u16 = 206;
/// Set the vector condition code to 1 iff the first input is orderable to the second input. Store the result into VCC or...
pub const V_CMP_O_F16: u16 = 207;
/// Set the vector condition code to 0. Store the result into the EXEC mask.
pub const V_CMPX_F_F16: u16 = 216;
/// Set the vector condition code to 1 iff the first input is less than the second input. Store the result into the EXEC ...
pub const V_CMPX_LT_F16: u16 = 217;
/// Set the vector condition code to 1 iff the first input is equal to the second input. Store the result into the EXEC m...
pub const V_CMPX_EQ_F16: u16 = 218;
/// Set the vector condition code to 1 iff the first input is less than or equal to the second input. Store the result in...
pub const V_CMPX_LE_F16: u16 = 219;
/// Set the vector condition code to 1 iff the first input is greater than the second input. Store the result into the EX...
pub const V_CMPX_GT_F16: u16 = 220;
/// Set the vector condition code to 1 iff the first input is less than or greater than the second input. Store the resul...
pub const V_CMPX_LG_F16: u16 = 221;
/// Set the vector condition code to 1 iff the first input is greater than or equal to the second input. Store the result...
pub const V_CMPX_GE_F16: u16 = 222;
/// Set the vector condition code to 1 iff the first input is orderable to the second input. Store the result into the EX...
pub const V_CMPX_O_F16: u16 = 223;
/// Set the vector condition code to 1 iff the first input is not orderable to the second input. Store the result into VC...
pub const V_CMP_U_F16: u16 = 232;
/// Set the vector condition code to 1 iff the first input is not greater than or equal to the second input. Store the re...
pub const V_CMP_NGE_F16: u16 = 233;
/// Set the vector condition code to 1 iff the first input is not less than or greater than the second input. Store the r...
pub const V_CMP_NLG_F16: u16 = 234;
/// Set the vector condition code to 1 iff the first input is not greater than the second input. Store the result into VC...
pub const V_CMP_NGT_F16: u16 = 235;
/// Set the vector condition code to 1 iff the first input is not less than or equal to the second input. Store the resul...
pub const V_CMP_NLE_F16: u16 = 236;
/// Set the vector condition code to 1 iff the first input is not equal to the second input. Store the result into VCC or...
pub const V_CMP_NEQ_F16: u16 = 237;
/// Set the vector condition code to 1 iff the first input is not less than the second input. Store the result into VCC o...
pub const V_CMP_NLT_F16: u16 = 238;
/// Set the vector condition code to 1. Store the result into VCC or a scalar register.
pub const V_CMP_TRU_F16: u16 = 239;
/// Set the vector condition code to 1 iff the first input is not orderable to the second input. Store the result into th...
pub const V_CMPX_U_F16: u16 = 248;
/// Set the vector condition code to 1 iff the first input is not greater than or equal to the second input. Store the re...
pub const V_CMPX_NGE_F16: u16 = 249;
/// Set the vector condition code to 1 iff the first input is not less than or greater than the second input. Store the r...
pub const V_CMPX_NLG_F16: u16 = 250;
/// Set the vector condition code to 1 iff the first input is not greater than the second input. Store the result into th...
pub const V_CMPX_NGT_F16: u16 = 251;
/// Set the vector condition code to 1 iff the first input is not less than or equal to the second input. Store the resul...
pub const V_CMPX_NLE_F16: u16 = 252;
/// Set the vector condition code to 1 iff the first input is not equal to the second input. Store the result into the EX...
pub const V_CMPX_NEQ_F16: u16 = 253;
/// Set the vector condition code to 1 iff the first input is not less than the second input. Store the result into the E...
pub const V_CMPX_NLT_F16: u16 = 254;
/// Set the vector condition code to 1. Store the result into the EXEC mask.
pub const V_CMPX_TRU_F16: u16 = 255;
