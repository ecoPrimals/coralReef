// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright © 2026 ecoPrimals
//! AUTO-GENERATED from AMD RDNA2 ISA XML specification.
//!
//! Source: specs/amd/amdgpu_isa_rdna2.xml (MIT license, AMD GPUOpen)
//! Generator: tools/amd-isa-gen (pure Rust, sovereign toolchain)
//!
//! DO NOT EDIT BY HAND. Regenerate with:
//!   cargo run -p amd-isa-gen

use super::isa_types::{BitField, InstrEntry};

/// ENC_VOP2 encoding fields (32 bits).
pub mod fields {
    use super::BitField;
    pub const SRC0: BitField = BitField {
        offset: 0,
        width: 9,
    };
    pub const VSRC1: BitField = BitField {
        offset: 9,
        width: 8,
    };
    pub const VDST: BitField = BitField {
        offset: 17,
        width: 8,
    };
    pub const OP: BitField = BitField {
        offset: 25,
        width: 6,
    };
    pub const ENCODING: BitField = BitField {
        offset: 31,
        width: 1,
    };
}

/// Copy data from one of two inputs based on the vector condition code and store the result into a vector register.
pub const V_CNDMASK_B32: u16 = 1;
/// Compute the dot product of two packed 2-D half-precision float inputs in the single-precision float domain and accumu...
pub const V_DOT2C_F32_F16: u16 = 2;
/// Add two floating point inputs and store the result into a vector register.
pub const V_ADD_F32: u16 = 3;
/// Subtract the second floating point input from the first input and store the result into a vector register.
pub const V_SUB_F32: u16 = 4;
/// Subtract the first floating point input from the second input and store the result into a vector register.
pub const V_SUBREV_F32: u16 = 5;
/// Multiply two single-precision values and accumulate the result with the destination. Follows DX9 rules where 0.0 time...
pub const V_FMAC_LEGACY_F32: u16 = 6;
/// Multiply two floating point inputs and store the result in a vector register. Follows DX9 rules where 0.0 times anyth...
pub const V_MUL_LEGACY_F32: u16 = 7;
/// Multiply two floating point inputs and store the result into a vector register.
pub const V_MUL_F32: u16 = 8;
/// Multiply two signed 24-bit integer inputs and store the result as a signed 32-bit integer into a vector register.
pub const V_MUL_I32_I24: u16 = 9;
/// Multiply two signed 24-bit integer inputs and store the high 32 bits of the result as a signed 32-bit integer into a ...
pub const V_MUL_HI_I32_I24: u16 = 10;
/// Multiply two unsigned 24-bit integer inputs and store the result as an unsigned 32-bit integer into a vector register.
pub const V_MUL_U32_U24: u16 = 11;
/// Multiply two unsigned 24-bit integer inputs and store the high 32 bits of the result as an unsigned 32-bit integer in...
pub const V_MUL_HI_U32_U24: u16 = 12;
/// Compute the dot product of two packed 4-D signed 8-bit integer inputs in the signed 32-bit integer domain and accumul...
pub const V_DOT4C_I32_I8: u16 = 13;
/// Select the minimum of two single-precision float inputs and store the result into a vector register.
pub const V_MIN_F32: u16 = 15;
/// Select the maximum of two single-precision float inputs and store the result into a vector register.
pub const V_MAX_F32: u16 = 16;
/// Select the minimum of two signed 32-bit integer inputs and store the selected value into a vector register.
pub const V_MIN_I32: u16 = 17;
/// Select the maximum of two signed 32-bit integer inputs and store the selected value into a vector register.
pub const V_MAX_I32: u16 = 18;
/// Select the minimum of two unsigned 32-bit integer inputs and store the selected value into a vector register.
pub const V_MIN_U32: u16 = 19;
/// Select the maximum of two unsigned 32-bit integer inputs and store the selected value into a vector register.
pub const V_MAX_U32: u16 = 20;
/// Given a shift count in the first vector input, calculate the logical shift right of the second vector input and store...
pub const V_LSHRREV_B32: u16 = 22;
/// Given a shift count in the first vector input, calculate the arithmetic shift right (preserving sign bit) of the seco...
pub const V_ASHRREV_I32: u16 = 24;
/// Given a shift count in the first vector input, calculate the logical shift left of the second vector input and store ...
pub const V_LSHLREV_B32: u16 = 26;
/// Calculate bitwise AND on two vector inputs and store the result into a vector register.
pub const V_AND_B32: u16 = 27;
/// Calculate bitwise OR on two vector inputs and store the result into a vector register.
pub const V_OR_B32: u16 = 28;
/// Calculate bitwise XOR on two vector inputs and store the result into a vector register.
pub const V_XOR_B32: u16 = 29;
/// Calculate bitwise XNOR on two vector inputs and store the result into a vector register.
pub const V_XNOR_B32: u16 = 30;
/// Add two unsigned 32-bit integer inputs and store the result into a vector register. No carry-in or carry-out support.
pub const V_ADD_NC_U32: u16 = 37;
/// Subtract the second unsigned input from the first input and store the result into a vector register. No carry-in or c...
pub const V_SUB_NC_U32: u16 = 38;
/// Subtract the first unsigned input from the second input and store the result into a vector register. No carry-in or c...
pub const V_SUBREV_NC_U32: u16 = 39;
/// Add two unsigned inputs and a bit from a carry-in mask, store the result into a vector register and store the carry-o...
pub const V_ADD_CO_CI_U32: u16 = 40;
/// Subtract the second unsigned input from the first input, subtract a bit from the carry-in mask, store the result into...
pub const V_SUB_CO_CI_U32: u16 = 41;
/// Subtract the first unsigned input from the second input, subtract a bit from the carry-in mask, store the result into...
pub const V_SUBREV_CO_CI_U32: u16 = 42;
/// Multiply two floating point inputs and accumulate the result into the destination register using fused multiply add.
pub const V_FMAC_F32: u16 = 43;
/// Convert two single-precision float inputs to a packed half-precision float value using round toward zero semantics (i...
pub const V_CVT_PKRTZ_F16_F32: u16 = 47;
/// Add two floating point inputs and store the result into a vector register.
pub const V_ADD_F16: u16 = 50;
/// Subtract the second floating point input from the first input and store the result into a vector register.
pub const V_SUB_F16: u16 = 51;
/// Subtract the first floating point input from the second input and store the result into a vector register.
pub const V_SUBREV_F16: u16 = 52;
/// Multiply two floating point inputs and store the result into a vector register.
pub const V_MUL_F16: u16 = 53;
/// Multiply two floating point inputs and accumulate the result into the destination register using fused multiply add.
pub const V_FMAC_F16: u16 = 54;
/// Select the maximum of two half-precision float inputs and store the result into a vector register.
pub const V_MAX_F16: u16 = 57;
/// Select the minimum of two half-precision float inputs and store the result into a vector register.
pub const V_MIN_F16: u16 = 58;
/// Multiply the first input, a floating point value, by an integral power of 2 specified in the second input, a signed i...
pub const V_LDEXP_F16: u16 = 59;
/// Multiply two packed half-precision float inputs component-wise and accumulate the result into the destination registe...
pub const V_PK_FMAC_F16: u16 = 60;

/// All ENC_VOP2 instructions.
pub const TABLE: &[InstrEntry] = &[
    InstrEntry {
        name: "V_CNDMASK_B32",
        opcode: 1,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_DOT2C_F32_F16",
        opcode: 2,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_ADD_F32",
        opcode: 3,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_SUB_F32",
        opcode: 4,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_SUBREV_F32",
        opcode: 5,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_FMAC_LEGACY_F32",
        opcode: 6,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_MUL_LEGACY_F32",
        opcode: 7,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_MUL_F32",
        opcode: 8,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_MUL_I32_I24",
        opcode: 9,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_MUL_HI_I32_I24",
        opcode: 10,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_MUL_U32_U24",
        opcode: 11,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_MUL_HI_U32_U24",
        opcode: 12,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_DOT4C_I32_I8",
        opcode: 13,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_MIN_F32",
        opcode: 15,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_MAX_F32",
        opcode: 16,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_MIN_I32",
        opcode: 17,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_MAX_I32",
        opcode: 18,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_MIN_U32",
        opcode: 19,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_MAX_U32",
        opcode: 20,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_LSHRREV_B32",
        opcode: 22,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_ASHRREV_I32",
        opcode: 24,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_LSHLREV_B32",
        opcode: 26,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_AND_B32",
        opcode: 27,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_OR_B32",
        opcode: 28,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_XOR_B32",
        opcode: 29,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_XNOR_B32",
        opcode: 30,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_ADD_NC_U32",
        opcode: 37,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_SUB_NC_U32",
        opcode: 38,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_SUBREV_NC_U32",
        opcode: 39,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_ADD_CO_CI_U32",
        opcode: 40,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_SUB_CO_CI_U32",
        opcode: 41,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_SUBREV_CO_CI_U32",
        opcode: 42,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_FMAC_F32",
        opcode: 43,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CVT_PKRTZ_F16_F32",
        opcode: 47,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_ADD_F16",
        opcode: 50,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_SUB_F16",
        opcode: 51,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_SUBREV_F16",
        opcode: 52,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_MUL_F16",
        opcode: 53,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_FMAC_F16",
        opcode: 54,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_MAX_F16",
        opcode: 57,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_MIN_F16",
        opcode: 58,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_LDEXP_F16",
        opcode: 59,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_PK_FMAC_F16",
        opcode: 60,
        is_branch: false,
        is_terminator: false,
    },
];

/// Look up an instruction by opcode.
#[must_use]
pub fn lookup(opcode: u16) -> Option<&'static InstrEntry> {
    TABLE.iter().find(|e| e.opcode == opcode)
}
