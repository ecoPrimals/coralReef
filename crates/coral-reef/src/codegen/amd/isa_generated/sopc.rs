// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2026 ecoPrimals
//! AUTO-GENERATED from AMD RDNA2 ISA XML specification.
//!
//! Source: specs/amd/amdgpu_isa_rdna2.xml (MIT license, AMD GPUOpen)
//! Generator: tools/amd-isa-gen (pure Rust, sovereign toolchain)
//!
//! DO NOT EDIT BY HAND. Regenerate with:
//!   cargo run -p amd-isa-gen

use super::isa_types::{BitField, InstrEntry};

/// ENC_SOPC encoding fields (32 bits).
pub mod fields {
    use super::BitField;
    pub const SSRC0: BitField = BitField {
        offset: 0,
        width: 8,
    };
    pub const SSRC1: BitField = BitField {
        offset: 8,
        width: 8,
    };
    pub const OP: BitField = BitField {
        offset: 16,
        width: 7,
    };
    pub const ENCODING: BitField = BitField {
        offset: 23,
        width: 9,
    };
}

/// Set SCC to 1 iff the first scalar input is equal to the second scalar input.
pub const S_CMP_EQ_I32: u16 = 0;
/// Set SCC to 1 iff the first scalar input is less than or greater than the second scalar input.
pub const S_CMP_LG_I32: u16 = 1;
/// Set SCC to 1 iff the first scalar input is greater than the second scalar input.
pub const S_CMP_GT_I32: u16 = 2;
/// Set SCC to 1 iff the first scalar input is greater than or equal to the second scalar input.
pub const S_CMP_GE_I32: u16 = 3;
/// Set SCC to 1 iff the first scalar input is less than the second scalar input.
pub const S_CMP_LT_I32: u16 = 4;
/// Set SCC to 1 iff the first scalar input is less than or equal to the second scalar input.
pub const S_CMP_LE_I32: u16 = 5;
/// Set SCC to 1 iff the first scalar input is equal to the second scalar input.
pub const S_CMP_EQ_U32: u16 = 6;
/// Set SCC to 1 iff the first scalar input is less than or greater than the second scalar input.
pub const S_CMP_LG_U32: u16 = 7;
/// Set SCC to 1 iff the first scalar input is greater than the second scalar input.
pub const S_CMP_GT_U32: u16 = 8;
/// Set SCC to 1 iff the first scalar input is greater than or equal to the second scalar input.
pub const S_CMP_GE_U32: u16 = 9;
/// Set SCC to 1 iff the first scalar input is less than the second scalar input.
pub const S_CMP_LT_U32: u16 = 10;
/// Set SCC to 1 iff the first scalar input is less than or equal to the second scalar input.
pub const S_CMP_LE_U32: u16 = 11;
/// Extract a bit from the first scalar input based on an index in the second scalar input, and set SCC to 1 iff the extr...
pub const S_BITCMP0_B32: u16 = 12;
/// Extract a bit from the first scalar input based on an index in the second scalar input, and set SCC to 1 iff the extr...
pub const S_BITCMP1_B32: u16 = 13;
/// Extract a bit from the first scalar input based on an index in the second scalar input, and set SCC to 1 iff the extr...
pub const S_BITCMP0_B64: u16 = 14;
/// Extract a bit from the first scalar input based on an index in the second scalar input, and set SCC to 1 iff the extr...
pub const S_BITCMP1_B64: u16 = 15;
/// Set SCC to 1 iff the first scalar input is equal to the second scalar input.
pub const S_CMP_EQ_U64: u16 = 18;
/// Set SCC to 1 iff the first scalar input is less than or greater than the second scalar input.
pub const S_CMP_LG_U64: u16 = 19;

/// All ENC_SOPC instructions.
pub const TABLE: &[InstrEntry] = &[
    InstrEntry {
        name: "S_CMP_EQ_I32",
        opcode: 0,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_CMP_LG_I32",
        opcode: 1,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_CMP_GT_I32",
        opcode: 2,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_CMP_GE_I32",
        opcode: 3,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_CMP_LT_I32",
        opcode: 4,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_CMP_LE_I32",
        opcode: 5,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_CMP_EQ_U32",
        opcode: 6,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_CMP_LG_U32",
        opcode: 7,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_CMP_GT_U32",
        opcode: 8,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_CMP_GE_U32",
        opcode: 9,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_CMP_LT_U32",
        opcode: 10,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_CMP_LE_U32",
        opcode: 11,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_BITCMP0_B32",
        opcode: 12,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_BITCMP1_B32",
        opcode: 13,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_BITCMP0_B64",
        opcode: 14,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_BITCMP1_B64",
        opcode: 15,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_CMP_EQ_U64",
        opcode: 18,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_CMP_LG_U64",
        opcode: 19,
        is_branch: false,
        is_terminator: false,
    },
];

/// Look up an instruction by opcode.
#[must_use]
pub fn lookup(opcode: u16) -> Option<&'static InstrEntry> {
    TABLE.iter().find(|e| e.opcode == opcode)
}
