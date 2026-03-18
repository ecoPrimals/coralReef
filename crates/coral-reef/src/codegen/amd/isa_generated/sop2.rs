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

/// ENC_SOP2 encoding fields (32 bits).
pub mod fields {
    use super::BitField;
    pub const SSRC0: BitField = BitField { offset: 0, width: 8 };
    pub const SSRC1: BitField = BitField { offset: 8, width: 8 };
    pub const SDST: BitField = BitField { offset: 16, width: 7 };
    pub const OP: BitField = BitField { offset: 23, width: 7 };
    pub const ENCODING: BitField = BitField { offset: 30, width: 2 };
}

/// Add two unsigned inputs, store the result into a scalar register and store the carry-out bit into SCC.
pub const S_ADD_U32: u16 = 0;
/// Subtract the second unsigned input from the first input, store the result into a scalar register and store the carry-...
pub const S_SUB_U32: u16 = 1;
/// Add two signed inputs, store the result into a scalar register and store the carry-out bit into SCC.
pub const S_ADD_I32: u16 = 2;
/// Subtract the second signed input from the first input, store the result into a scalar register and store the carry-ou...
pub const S_SUB_I32: u16 = 3;
/// Add two unsigned inputs and a carry-in bit, store the result into a scalar register and store the carry-out bit into ...
pub const S_ADDC_U32: u16 = 4;
/// Subtract the second unsigned input from the first input, subtract the carry-in bit, store the result into a scalar re...
pub const S_SUBB_U32: u16 = 5;
/// Select the minimum of two signed 32-bit integer inputs, store the selected value into a scalar register and set SCC i...
pub const S_MIN_I32: u16 = 6;
/// Select the minimum of two unsigned 32-bit integer inputs, store the selected value into a scalar register and set SCC...
pub const S_MIN_U32: u16 = 7;
/// Select the maximum of two signed 32-bit integer inputs, store the selected value into a scalar register and set SCC i...
pub const S_MAX_I32: u16 = 8;
/// Select the maximum of two unsigned 32-bit integer inputs, store the selected value into a scalar register and set SCC...
pub const S_MAX_U32: u16 = 9;
/// Select the first input if SCC is true otherwise select the second input, then store the selected input into a scalar ...
pub const S_CSELECT_B32: u16 = 10;
/// Select the first input if SCC is true otherwise select the second input, then store the selected input into a scalar ...
pub const S_CSELECT_B64: u16 = 11;
/// Calculate bitwise AND on two scalar inputs, store the result into a scalar register and set SCC iff the result is non...
pub const S_AND_B32: u16 = 14;
/// Calculate bitwise AND on two scalar inputs, store the result into a scalar register and set SCC iff the result is non...
pub const S_AND_B64: u16 = 15;
/// Calculate bitwise OR on two scalar inputs, store the result into a scalar register and set SCC iff the result is nonz...
pub const S_OR_B32: u16 = 16;
/// Calculate bitwise OR on two scalar inputs, store the result into a scalar register and set SCC iff the result is nonz...
pub const S_OR_B64: u16 = 17;
/// Calculate bitwise XOR on two scalar inputs, store the result into a scalar register and set SCC iff the result is non...
pub const S_XOR_B32: u16 = 18;
/// Calculate bitwise XOR on two scalar inputs, store the result into a scalar register and set SCC iff the result is non...
pub const S_XOR_B64: u16 = 19;
/// Calculate bitwise AND with the first input and the negation of the second input, store the result into a scalar regis...
pub const S_ANDN2_B32: u16 = 20;
/// Calculate bitwise AND with the first input and the negation of the second input, store the result into a scalar regis...
pub const S_ANDN2_B64: u16 = 21;
/// Calculate bitwise OR with the first input and the negation of the second input, store the result into a scalar regist...
pub const S_ORN2_B32: u16 = 22;
/// Calculate bitwise OR with the first input and the negation of the second input, store the result into a scalar regist...
pub const S_ORN2_B64: u16 = 23;
/// Calculate bitwise NAND on two scalar inputs, store the result into a scalar register and set SCC if the result is non...
pub const S_NAND_B32: u16 = 24;
/// Calculate bitwise NAND on two scalar inputs, store the result into a scalar register and set SCC if the result is non...
pub const S_NAND_B64: u16 = 25;
/// Calculate bitwise NOR on two scalar inputs, store the result into a scalar register and set SCC if the result is nonz...
pub const S_NOR_B32: u16 = 26;
/// Calculate bitwise NOR on two scalar inputs, store the result into a scalar register and set SCC if the result is nonz...
pub const S_NOR_B64: u16 = 27;
/// Calculate bitwise XNOR on two scalar inputs, store the result into a scalar register and set SCC if the result is non...
pub const S_XNOR_B32: u16 = 28;
/// Calculate bitwise XNOR on two scalar inputs, store the result into a scalar register and set SCC if the result is non...
pub const S_XNOR_B64: u16 = 29;
/// Given a shift count in the second scalar input, calculate the logical shift left of the first scalar input, store the...
pub const S_LSHL_B32: u16 = 30;
/// Given a shift count in the second scalar input, calculate the logical shift left of the first scalar input, store the...
pub const S_LSHL_B64: u16 = 31;
/// Given a shift count in the second scalar input, calculate the logical shift right of the first scalar input, store th...
pub const S_LSHR_B32: u16 = 32;
/// Given a shift count in the second scalar input, calculate the logical shift right of the first scalar input, store th...
pub const S_LSHR_B64: u16 = 33;
/// Given a shift count in the second scalar input, calculate the arithmetic shift right (preserving sign bit) of the fir...
pub const S_ASHR_I32: u16 = 34;
/// Given a shift count in the second scalar input, calculate the arithmetic shift right (preserving sign bit) of the fir...
pub const S_ASHR_I64: u16 = 35;
/// Calculate a bitfield mask given a field offset and size and store the result in a scalar register.
pub const S_BFM_B32: u16 = 36;
/// Calculate a bitfield mask given a field offset and size and store the result in a scalar register.
pub const S_BFM_B64: u16 = 37;
/// Multiply two signed integers and store the result into a scalar register.
pub const S_MUL_I32: u16 = 38;
/// Extract an unsigned bitfield from the first input using field offset and size encoded in the second input, store the ...
pub const S_BFE_U32: u16 = 39;
/// Extract a signed bitfield from the first input using field offset and size encoded in the second input, store the res...
pub const S_BFE_I32: u16 = 40;
/// Extract an unsigned bitfield from the first input using field offset and size encoded in the second input, store the ...
pub const S_BFE_U64: u16 = 41;
/// Extract a signed bitfield from the first input using field offset and size encoded in the second input, store the res...
pub const S_BFE_I64: u16 = 42;
/// Calculate the absolute value of difference between two scalar inputs, store the result into a scalar register and set...
pub const S_ABSDIFF_I32: u16 = 44;
/// Calculate the logical shift left of the first input by 1, then add the second input, store the result into a scalar r...
pub const S_LSHL1_ADD_U32: u16 = 46;
/// Calculate the logical shift left of the first input by 2, then add the second input, store the result into a scalar r...
pub const S_LSHL2_ADD_U32: u16 = 47;
/// Calculate the logical shift left of the first input by 3, then add the second input, store the result into a scalar r...
pub const S_LSHL3_ADD_U32: u16 = 48;
/// Calculate the logical shift left of the first input by 4, then add the second input, store the result into a scalar r...
pub const S_LSHL4_ADD_U32: u16 = 49;
/// Pack two 16-bit scalar values into a scalar register.
pub const S_PACK_LL_B32_B16: u16 = 50;
/// Pack two 16-bit scalar values into a scalar register.
pub const S_PACK_LH_B32_B16: u16 = 51;
/// Pack two 16-bit scalar values into a scalar register.
pub const S_PACK_HH_B32_B16: u16 = 52;
/// Multiply two unsigned integers and store the high 32 bits of the result into a scalar register.
pub const S_MUL_HI_U32: u16 = 53;
/// Multiply two signed integers and store the high 32 bits of the result into a scalar register.
pub const S_MUL_HI_I32: u16 = 54;

/// All ENC_SOP2 instructions.
pub const TABLE: &[InstrEntry] = &[
    InstrEntry { name: "S_ADD_U32", opcode: 0, is_branch: false, is_terminator: false },
    InstrEntry { name: "S_SUB_U32", opcode: 1, is_branch: false, is_terminator: false },
    InstrEntry { name: "S_ADD_I32", opcode: 2, is_branch: false, is_terminator: false },
    InstrEntry { name: "S_SUB_I32", opcode: 3, is_branch: false, is_terminator: false },
    InstrEntry { name: "S_ADDC_U32", opcode: 4, is_branch: false, is_terminator: false },
    InstrEntry { name: "S_SUBB_U32", opcode: 5, is_branch: false, is_terminator: false },
    InstrEntry { name: "S_MIN_I32", opcode: 6, is_branch: false, is_terminator: false },
    InstrEntry { name: "S_MIN_U32", opcode: 7, is_branch: false, is_terminator: false },
    InstrEntry { name: "S_MAX_I32", opcode: 8, is_branch: false, is_terminator: false },
    InstrEntry { name: "S_MAX_U32", opcode: 9, is_branch: false, is_terminator: false },
    InstrEntry { name: "S_CSELECT_B32", opcode: 10, is_branch: false, is_terminator: false },
    InstrEntry { name: "S_CSELECT_B64", opcode: 11, is_branch: false, is_terminator: false },
    InstrEntry { name: "S_AND_B32", opcode: 14, is_branch: false, is_terminator: false },
    InstrEntry { name: "S_AND_B64", opcode: 15, is_branch: false, is_terminator: false },
    InstrEntry { name: "S_OR_B32", opcode: 16, is_branch: false, is_terminator: false },
    InstrEntry { name: "S_OR_B64", opcode: 17, is_branch: false, is_terminator: false },
    InstrEntry { name: "S_XOR_B32", opcode: 18, is_branch: false, is_terminator: false },
    InstrEntry { name: "S_XOR_B64", opcode: 19, is_branch: false, is_terminator: false },
    InstrEntry { name: "S_ANDN2_B32", opcode: 20, is_branch: false, is_terminator: false },
    InstrEntry { name: "S_ANDN2_B64", opcode: 21, is_branch: false, is_terminator: false },
    InstrEntry { name: "S_ORN2_B32", opcode: 22, is_branch: false, is_terminator: false },
    InstrEntry { name: "S_ORN2_B64", opcode: 23, is_branch: false, is_terminator: false },
    InstrEntry { name: "S_NAND_B32", opcode: 24, is_branch: false, is_terminator: false },
    InstrEntry { name: "S_NAND_B64", opcode: 25, is_branch: false, is_terminator: false },
    InstrEntry { name: "S_NOR_B32", opcode: 26, is_branch: false, is_terminator: false },
    InstrEntry { name: "S_NOR_B64", opcode: 27, is_branch: false, is_terminator: false },
    InstrEntry { name: "S_XNOR_B32", opcode: 28, is_branch: false, is_terminator: false },
    InstrEntry { name: "S_XNOR_B64", opcode: 29, is_branch: false, is_terminator: false },
    InstrEntry { name: "S_LSHL_B32", opcode: 30, is_branch: false, is_terminator: false },
    InstrEntry { name: "S_LSHL_B64", opcode: 31, is_branch: false, is_terminator: false },
    InstrEntry { name: "S_LSHR_B32", opcode: 32, is_branch: false, is_terminator: false },
    InstrEntry { name: "S_LSHR_B64", opcode: 33, is_branch: false, is_terminator: false },
    InstrEntry { name: "S_ASHR_I32", opcode: 34, is_branch: false, is_terminator: false },
    InstrEntry { name: "S_ASHR_I64", opcode: 35, is_branch: false, is_terminator: false },
    InstrEntry { name: "S_BFM_B32", opcode: 36, is_branch: false, is_terminator: false },
    InstrEntry { name: "S_BFM_B64", opcode: 37, is_branch: false, is_terminator: false },
    InstrEntry { name: "S_MUL_I32", opcode: 38, is_branch: false, is_terminator: false },
    InstrEntry { name: "S_BFE_U32", opcode: 39, is_branch: false, is_terminator: false },
    InstrEntry { name: "S_BFE_I32", opcode: 40, is_branch: false, is_terminator: false },
    InstrEntry { name: "S_BFE_U64", opcode: 41, is_branch: false, is_terminator: false },
    InstrEntry { name: "S_BFE_I64", opcode: 42, is_branch: false, is_terminator: false },
    InstrEntry { name: "S_ABSDIFF_I32", opcode: 44, is_branch: false, is_terminator: false },
    InstrEntry { name: "S_LSHL1_ADD_U32", opcode: 46, is_branch: false, is_terminator: false },
    InstrEntry { name: "S_LSHL2_ADD_U32", opcode: 47, is_branch: false, is_terminator: false },
    InstrEntry { name: "S_LSHL3_ADD_U32", opcode: 48, is_branch: false, is_terminator: false },
    InstrEntry { name: "S_LSHL4_ADD_U32", opcode: 49, is_branch: false, is_terminator: false },
    InstrEntry { name: "S_PACK_LL_B32_B16", opcode: 50, is_branch: false, is_terminator: false },
    InstrEntry { name: "S_PACK_LH_B32_B16", opcode: 51, is_branch: false, is_terminator: false },
    InstrEntry { name: "S_PACK_HH_B32_B16", opcode: 52, is_branch: false, is_terminator: false },
    InstrEntry { name: "S_MUL_HI_U32", opcode: 53, is_branch: false, is_terminator: false },
    InstrEntry { name: "S_MUL_HI_I32", opcode: 54, is_branch: false, is_terminator: false },
];

/// Look up an instruction by opcode.
#[must_use]
pub fn lookup(opcode: u16) -> Option<&'static InstrEntry> {
    TABLE.iter().find(|e| e.opcode == opcode)
}

