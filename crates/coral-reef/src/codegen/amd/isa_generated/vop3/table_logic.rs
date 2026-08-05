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
        name: "V_LSHRREV_B32",
        opcode: 278,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_ASHRREV_I32",
        opcode: 280,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_LSHLREV_B32",
        opcode: 282,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_AND_B32",
        opcode: 283,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_OR_B32",
        opcode: 284,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_XOR_B32",
        opcode: 285,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_XNOR_B32",
        opcode: 286,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_BFE_U32",
        opcode: 328,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_BFE_I32",
        opcode: 329,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_BFI_B32",
        opcode: 330,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_ALIGNBIT_B32",
        opcode: 334,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_ALIGNBYTE_B32",
        opcode: 335,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_XOR3_B32",
        opcode: 376,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_NOT_B32",
        opcode: 439,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_BFREV_B32",
        opcode: 440,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_FFBH_U32",
        opcode: 441,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_FFBL_B32",
        opcode: 442,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_FFBH_I32",
        opcode: 443,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_LSHLREV_B64",
        opcode: 767,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_LSHRREV_B64",
        opcode: 768,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_ASHRREV_I64",
        opcode: 769,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_LSHRREV_B16",
        opcode: 775,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_ASHRREV_I16",
        opcode: 776,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_LSHLREV_B16",
        opcode: 788,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_PERM_B32",
        opcode: 836,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_BFM_B32",
        opcode: 867,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_BCNT_U32_B32",
        opcode: 868,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_MBCNT_LO_U32_B32",
        opcode: 869,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_MBCNT_HI_U32_B32",
        opcode: 870,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_LSHL_OR_B32",
        opcode: 879,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_AND_OR_B32",
        opcode: 881,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_OR3_B32",
        opcode: 882,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_PERMLANE16_B32",
        opcode: 887,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_PERMLANEX16_B32",
        opcode: 888,
        is_branch: false,
        is_terminator: false,
    },
];

#[must_use]
pub fn lookup(opcode: u16) -> Option<&'static InstrEntry> {
    TABLE.iter().find(|e| e.opcode == opcode)
}

/// Given a shift count in the first vector input, calculate the logical shift right of the second vector input and store...
pub const V_LSHRREV_B32: u16 = 278;
/// Given a shift count in the first vector input, calculate the arithmetic shift right (preserving sign bit) of the seco...
pub const V_ASHRREV_I32: u16 = 280;
/// Given a shift count in the first vector input, calculate the logical shift left of the second vector input and store ...
pub const V_LSHLREV_B32: u16 = 282;
/// Calculate bitwise AND on two vector inputs and store the result into a vector register.
pub const V_AND_B32: u16 = 283;
/// Calculate bitwise OR on two vector inputs and store the result into a vector register.
pub const V_OR_B32: u16 = 284;
/// Calculate bitwise XOR on two vector inputs and store the result into a vector register.
pub const V_XOR_B32: u16 = 285;
/// Calculate bitwise XNOR on two vector inputs and store the result into a vector register.
pub const V_XNOR_B32: u16 = 286;
/// Extract an unsigned bitfield from the first input using field offset from the second input and size from the third in...
pub const V_BFE_U32: u16 = 328;
/// Extract a signed bitfield from the first input using field offset from the second input and size from the third input...
pub const V_BFE_I32: u16 = 329;
/// Overwrite a bitfield in the third input with a bitfield from the second input using a mask from the first input, then...
pub const V_BFI_B32: u16 = 330;
/// Align a 64-bit value encoded in the first two inputs to a bit position specified in the third input, then store the r...
pub const V_ALIGNBIT_B32: u16 = 334;
/// Align a 64-bit value encoded in the first two inputs to a byte position specified in the third input, then store the ...
pub const V_ALIGNBYTE_B32: u16 = 335;
/// Calculate the bitwise XOR of three vector inputs and store the result into a vector register.
pub const V_XOR3_B32: u16 = 376;
/// Calculate bitwise negation on a vector input and store the result into a vector register.
pub const V_NOT_B32: u16 = 439;
/// Reverse the order of bits in a vector input and store the result into a vector register.
pub const V_BFREV_B32: u16 = 440;
/// Count the number of leading \"0\" bits before the first \"1\" in a vector input and store the result into a vector re...
pub const V_FFBH_U32: u16 = 441;
/// Count the number of trailing \"0\" bits before the first \"1\" in a vector input and store the result into a vector r...
pub const V_FFBL_B32: u16 = 442;
/// Count the number of leading bits that are the same as the sign bit of a vector input and store the result into a vect...
pub const V_FFBH_I32: u16 = 443;
/// Given a shift count in the first vector input, calculate the logical shift left of the second vector input and store ...
pub const V_LSHLREV_B64: u16 = 767;
/// Given a shift count in the first vector input, calculate the logical shift right of the second vector input and store...
pub const V_LSHRREV_B64: u16 = 768;
/// Given a shift count in the first vector input, calculate the arithmetic shift right (preserving sign bit) of the seco...
pub const V_ASHRREV_I64: u16 = 769;
/// Given a shift count in the first vector input, calculate the logical shift right of the second vector input and store...
pub const V_LSHRREV_B16: u16 = 775;
/// Given a shift count in the first vector input, calculate the arithmetic shift right (preserving sign bit) of the seco...
pub const V_ASHRREV_I16: u16 = 776;
/// Given a shift count in the first vector input, calculate the logical shift left of the second vector input and store ...
pub const V_LSHLREV_B16: u16 = 788;
/// Permute a 64-bit value constructed from two vector inputs (most significant bits come from the first input) using a p...
pub const V_PERM_B32: u16 = 836;
/// Calculate a bitfield mask given a field offset and size and store the result into a vector register.
pub const V_BFM_B32: u16 = 867;
/// Count the number of \"1\" bits in the vector input and store the result into a vector register.
pub const V_BCNT_U32_B32: u16 = 868;
/// For each lane 0
pub const V_MBCNT_LO_U32_B32: u16 = 869;
/// For each lane 32
pub const V_MBCNT_HI_U32_B32: u16 = 870;
/// Given a shift count in the second input, calculate the logical shift left of the first input, then calculate the bitw...
pub const V_LSHL_OR_B32: u16 = 879;
/// Calculate bitwise AND on the first two vector inputs, then compute the bitwise OR of the intermediate result and the ...
pub const V_AND_OR_B32: u16 = 881;
/// Calculate the bitwise OR of three vector inputs and store the result into a vector register.
pub const V_OR3_B32: u16 = 882;
/// Perform arbitrary gather-style operation within a row (16 contiguous lanes).
pub const V_PERMLANE16_B32: u16 = 887;
/// Perform arbitrary gather-style operation across two rows (each row is 16 contiguous lanes).
pub const V_PERMLANEX16_B32: u16 = 888;
