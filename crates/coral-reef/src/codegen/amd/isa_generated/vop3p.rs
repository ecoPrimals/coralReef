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

/// ENC_VOP3P encoding fields (64 bits).
pub mod fields {
    use super::BitField;
    pub const VDST: BitField = BitField {
        offset: 0,
        width: 8,
    };
    pub const NEG_HI: BitField = BitField {
        offset: 8,
        width: 3,
    };
    pub const OP_SEL: BitField = BitField {
        offset: 11,
        width: 3,
    };
    pub const OP_SEL_HI_2: BitField = BitField {
        offset: 14,
        width: 1,
    };
    pub const CLAMP: BitField = BitField {
        offset: 15,
        width: 1,
    };
    pub const OP: BitField = BitField {
        offset: 16,
        width: 7,
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
    pub const OP_SEL_HI: BitField = BitField {
        offset: 59,
        width: 2,
    };
    pub const NEG: BitField = BitField {
        offset: 61,
        width: 3,
    };
}

/// Multiply two packed signed 16-bit integer inputs component-wise, add a packed signed 16-bit integer value from a thir...
pub const V_PK_MAD_I16: u16 = 0;
/// Multiply two packed unsigned 16-bit integer inputs component-wise and store the low bits of each resulting component ...
pub const V_PK_MUL_LO_U16: u16 = 1;
/// Add two packed signed 16-bit integer inputs component-wise and store the result into a vector register. No carry-in o...
pub const V_PK_ADD_I16: u16 = 2;
/// Subtract the second packed signed 16-bit integer input from the first input component-wise and store the result into ...
pub const V_PK_SUB_I16: u16 = 3;
/// Given a packed shift count in the first vector input, calculate the component-wise logical shift left of the second p...
pub const V_PK_LSHLREV_B16: u16 = 4;
/// Given a packed shift count in the first vector input, calculate the component-wise logical shift right of the second ...
pub const V_PK_LSHRREV_B16: u16 = 5;
/// Given a packed shift count in the first vector input, calculate the component-wise arithmetic shift right (preserving...
pub const V_PK_ASHRREV_I16: u16 = 6;
/// Select the component-wise maximum of two packed signed 16-bit integer inputs and store the selected values into a vec...
pub const V_PK_MAX_I16: u16 = 7;
/// Select the component-wise minimum of two packed signed 16-bit integer inputs and store the selected values into a vec...
pub const V_PK_MIN_I16: u16 = 8;
/// Multiply two packed unsigned 16-bit integer inputs component-wise, add a packed unsigned 16-bit integer value from a ...
pub const V_PK_MAD_U16: u16 = 9;
/// Add two packed unsigned 16-bit integer inputs component-wise and store the result into a vector register. No carry-in...
pub const V_PK_ADD_U16: u16 = 10;
/// Subtract the second packed unsigned 16-bit integer input from the first input component-wise and store the result int...
pub const V_PK_SUB_U16: u16 = 11;
/// Select the component-wise maximum of two packed unsigned 16-bit integer inputs and store the selected values into a v...
pub const V_PK_MAX_U16: u16 = 12;
/// Select the component-wise minimum of two packed unsigned 16-bit integer inputs and store the selected values into a v...
pub const V_PK_MIN_U16: u16 = 13;
/// Multiply two packed half-precision float inputs component-wise and add a third input component-wise using fused multi...
pub const V_PK_FMA_F16: u16 = 14;
/// Add two packed half-precision float inputs component-wise and store the result into a vector register. No carry-in or...
pub const V_PK_ADD_F16: u16 = 15;
/// Multiply two packed half-precision float inputs component-wise and store the result into a vector register.
pub const V_PK_MUL_F16: u16 = 16;
/// Select the component-wise minimum of two packed half-precision float inputs and store the result into a vector register.
pub const V_PK_MIN_F16: u16 = 17;
/// Select the component-wise maximum of two packed half-precision float inputs and store the result into a vector register.
pub const V_PK_MAX_F16: u16 = 18;
/// Compute the dot product of two packed 2-D half-precision float inputs in the single-precision float domain, add a sin...
pub const V_DOT2_F32_F16: u16 = 19;
/// Compute the dot product of two packed 2-D signed 16-bit integer inputs in the signed 32-bit integer domain, add a sig...
pub const V_DOT2_I32_I16: u16 = 20;
/// Compute the dot product of two packed 2-D unsigned 16-bit integer inputs in the unsigned 32-bit integer domain, add a...
pub const V_DOT2_U32_U16: u16 = 21;
/// Compute the dot product of two packed 4-D signed 8-bit integer inputs in the signed 32-bit integer domain, add a sign...
pub const V_DOT4_I32_I8: u16 = 22;
/// Compute the dot product of two packed 4-D unsigned 8-bit integer inputs in the unsigned 32-bit integer domain, add an...
pub const V_DOT4_U32_U8: u16 = 23;
/// Compute the dot product of two packed 8-D signed 4-bit integer inputs in the signed 32-bit integer domain, add a sign...
pub const V_DOT8_I32_I4: u16 = 24;
/// Compute the dot product of two packed 8-D unsigned 4-bit integer inputs in the unsigned 32-bit integer domain, add an...
pub const V_DOT8_U32_U4: u16 = 25;
/// Multiply two inputs and add a third input using fused multiply add where the inputs are a mix of 16-bit and 32-bit fl...
pub const V_FMA_MIX_F32: u16 = 32;
/// Multiply two inputs and add a third input using fused multiply add where the inputs are a mix of 16-bit and 32-bit fl...
pub const V_FMA_MIXLO_F16: u16 = 33;
/// Multiply two inputs and add a third input using fused multiply add where the inputs are a mix of 16-bit and 32-bit fl...
pub const V_FMA_MIXHI_F16: u16 = 34;

/// All ENC_VOP3P instructions.
pub const TABLE: &[InstrEntry] = &[
    InstrEntry {
        name: "V_PK_MAD_I16",
        opcode: 0,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_PK_MUL_LO_U16",
        opcode: 1,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_PK_ADD_I16",
        opcode: 2,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_PK_SUB_I16",
        opcode: 3,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_PK_LSHLREV_B16",
        opcode: 4,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_PK_LSHRREV_B16",
        opcode: 5,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_PK_ASHRREV_I16",
        opcode: 6,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_PK_MAX_I16",
        opcode: 7,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_PK_MIN_I16",
        opcode: 8,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_PK_MAD_U16",
        opcode: 9,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_PK_ADD_U16",
        opcode: 10,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_PK_SUB_U16",
        opcode: 11,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_PK_MAX_U16",
        opcode: 12,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_PK_MIN_U16",
        opcode: 13,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_PK_FMA_F16",
        opcode: 14,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_PK_ADD_F16",
        opcode: 15,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_PK_MUL_F16",
        opcode: 16,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_PK_MIN_F16",
        opcode: 17,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_PK_MAX_F16",
        opcode: 18,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_DOT2_F32_F16",
        opcode: 19,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_DOT2_I32_I16",
        opcode: 20,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_DOT2_U32_U16",
        opcode: 21,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_DOT4_I32_I8",
        opcode: 22,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_DOT4_U32_U8",
        opcode: 23,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_DOT8_I32_I4",
        opcode: 24,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_DOT8_U32_U4",
        opcode: 25,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_FMA_MIX_F32",
        opcode: 32,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_FMA_MIXLO_F16",
        opcode: 33,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_FMA_MIXHI_F16",
        opcode: 34,
        is_branch: false,
        is_terminator: false,
    },
];

/// Look up an instruction by opcode.
#[must_use]
pub fn lookup(opcode: u16) -> Option<&'static InstrEntry> {
    TABLE.iter().find(|e| e.opcode == opcode)
}
