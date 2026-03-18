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

/// ENC_SOPK encoding fields (32 bits).
pub mod fields {
    use super::BitField;
    pub const SIMM16: BitField = BitField { offset: 0, width: 16 };
    pub const SDST: BitField = BitField { offset: 16, width: 7 };
    pub const OP: BitField = BitField { offset: 23, width: 5 };
    pub const ENCODING: BitField = BitField { offset: 28, width: 4 };
}

/// Sign extend a literal 16-bit constant and store the result into a scalar register.
pub const S_MOVK_I32: u16 = 0;
/// Perform no operation. This opcode is used to specify the microcode version for tools that interpret shader microcode.
pub const S_VERSION: u16 = 1;
/// Move the sign extension of a literal 16-bit constant into a scalar register iff SCC is nonzero.
pub const S_CMOVK_I32: u16 = 2;
/// Set SCC to 1 iff scalar input is equal to the sign extension of a literal 16-bit constant.
pub const S_CMPK_EQ_I32: u16 = 3;
/// Set SCC to 1 iff scalar input is less than or greater than the sign extension of a literal 16-bit constant.
pub const S_CMPK_LG_I32: u16 = 4;
/// Set SCC to 1 iff scalar input is greater than the sign extension of a literal 16-bit constant.
pub const S_CMPK_GT_I32: u16 = 5;
/// Set SCC to 1 iff scalar input is greater than or equal to the sign extension of a literal 16-bit constant.
pub const S_CMPK_GE_I32: u16 = 6;
/// Set SCC to 1 iff scalar input is less than the sign extension of a literal 16-bit constant.
pub const S_CMPK_LT_I32: u16 = 7;
/// Set SCC to 1 iff scalar input is less than or equal to the sign extension of a literal 16-bit constant.
pub const S_CMPK_LE_I32: u16 = 8;
/// Set SCC to 1 iff scalar input is equal to the zero extension of a literal 16-bit constant.
pub const S_CMPK_EQ_U32: u16 = 9;
/// Set SCC to 1 iff scalar input is less than or greater than the zero extension of a literal 16-bit constant.
pub const S_CMPK_LG_U32: u16 = 10;
/// Set SCC to 1 iff scalar input is greater than the zero extension of a literal 16-bit constant.
pub const S_CMPK_GT_U32: u16 = 11;
/// Set SCC to 1 iff scalar input is greater than or equal to the zero extension of a literal 16-bit constant.
pub const S_CMPK_GE_U32: u16 = 12;
/// Set SCC to 1 iff scalar input is less than the zero extension of a literal 16-bit constant.
pub const S_CMPK_LT_U32: u16 = 13;
/// Set SCC to 1 iff scalar input is less than or equal to the zero extension of a literal 16-bit constant.
pub const S_CMPK_LE_U32: u16 = 14;
/// Add a scalar input and the sign extension of a literal 16-bit constant, store the result into a scalar register and s...
pub const S_ADDK_I32: u16 = 15;
/// Multiply a scalar input with the sign extension of a literal 16-bit constant and store the result into a scalar regis...
pub const S_MULK_I32: u16 = 16;
/// Read some or all of a hardware register into the LSBs of destination.
pub const S_GETREG_B32: u16 = 18;
/// Write some or all of the LSBs of source argument into a hardware register.
pub const S_SETREG_B32: u16 = 19;
/// Store the address of the next instruction to a scalar register and then jump to a constant offset relative to the cur...
pub const S_CALL_B64: u16 = 22;
/// Wait for the VSCNT counter to be at or below the specified level. The VSCNT counter tracks the number of outstanding ...
pub const S_WAITCNT_VSCNT: u16 = 23;
/// Wait for the VMCNT counter to be at or below the specified level. The VMCNT counter tracks the number of outstanding ...
pub const S_WAITCNT_VMCNT: u16 = 24;
/// Wait for the EXPCNT counter to be at or below the specified level. The EXPCNT counter tracks the number of outstandin...
pub const S_WAITCNT_EXPCNT: u16 = 25;
/// Wait for the LGKMCNT counter to be at or below the specified level. The LGKMCNT counter tracks the number of outstand...
pub const S_WAITCNT_LGKMCNT: u16 = 26;
/// Begin execution of a subvector block of code.
pub const S_SUBVECTOR_LOOP_BEGIN: u16 = 27;
/// End execution of a subvector block of code.
pub const S_SUBVECTOR_LOOP_END: u16 = 28;

/// All ENC_SOPK instructions.
pub const TABLE: &[InstrEntry] = &[
    InstrEntry { name: "S_MOVK_I32", opcode: 0, is_branch: false, is_terminator: false },
    InstrEntry { name: "S_VERSION", opcode: 1, is_branch: false, is_terminator: false },
    InstrEntry { name: "S_CMOVK_I32", opcode: 2, is_branch: false, is_terminator: false },
    InstrEntry { name: "S_CMPK_EQ_I32", opcode: 3, is_branch: false, is_terminator: false },
    InstrEntry { name: "S_CMPK_LG_I32", opcode: 4, is_branch: false, is_terminator: false },
    InstrEntry { name: "S_CMPK_GT_I32", opcode: 5, is_branch: false, is_terminator: false },
    InstrEntry { name: "S_CMPK_GE_I32", opcode: 6, is_branch: false, is_terminator: false },
    InstrEntry { name: "S_CMPK_LT_I32", opcode: 7, is_branch: false, is_terminator: false },
    InstrEntry { name: "S_CMPK_LE_I32", opcode: 8, is_branch: false, is_terminator: false },
    InstrEntry { name: "S_CMPK_EQ_U32", opcode: 9, is_branch: false, is_terminator: false },
    InstrEntry { name: "S_CMPK_LG_U32", opcode: 10, is_branch: false, is_terminator: false },
    InstrEntry { name: "S_CMPK_GT_U32", opcode: 11, is_branch: false, is_terminator: false },
    InstrEntry { name: "S_CMPK_GE_U32", opcode: 12, is_branch: false, is_terminator: false },
    InstrEntry { name: "S_CMPK_LT_U32", opcode: 13, is_branch: false, is_terminator: false },
    InstrEntry { name: "S_CMPK_LE_U32", opcode: 14, is_branch: false, is_terminator: false },
    InstrEntry { name: "S_ADDK_I32", opcode: 15, is_branch: false, is_terminator: false },
    InstrEntry { name: "S_MULK_I32", opcode: 16, is_branch: false, is_terminator: false },
    InstrEntry { name: "S_GETREG_B32", opcode: 18, is_branch: false, is_terminator: false },
    InstrEntry { name: "S_SETREG_B32", opcode: 19, is_branch: false, is_terminator: false },
    InstrEntry { name: "S_CALL_B64", opcode: 22, is_branch: true, is_terminator: false },
    InstrEntry { name: "S_WAITCNT_VSCNT", opcode: 23, is_branch: false, is_terminator: false },
    InstrEntry { name: "S_WAITCNT_VMCNT", opcode: 24, is_branch: false, is_terminator: false },
    InstrEntry { name: "S_WAITCNT_EXPCNT", opcode: 25, is_branch: false, is_terminator: false },
    InstrEntry { name: "S_WAITCNT_LGKMCNT", opcode: 26, is_branch: false, is_terminator: false },
    InstrEntry { name: "S_SUBVECTOR_LOOP_BEGIN", opcode: 27, is_branch: true, is_terminator: false },
    InstrEntry { name: "S_SUBVECTOR_LOOP_END", opcode: 28, is_branch: true, is_terminator: false },
];

/// Look up an instruction by opcode.
#[must_use]
pub fn lookup(opcode: u16) -> Option<&'static InstrEntry> {
    TABLE.iter().find(|e| e.opcode == opcode)
}

