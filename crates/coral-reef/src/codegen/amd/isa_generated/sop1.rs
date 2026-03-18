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

/// ENC_SOP1 encoding fields (32 bits).
pub mod fields {
    use super::BitField;
    pub const SSRC0: BitField = BitField {
        offset: 0,
        width: 8,
    };
    pub const OP: BitField = BitField {
        offset: 8,
        width: 8,
    };
    pub const SDST: BitField = BitField {
        offset: 16,
        width: 7,
    };
    pub const ENCODING: BitField = BitField {
        offset: 23,
        width: 9,
    };
}

/// Move scalar input into a scalar register.
pub const S_MOV_B32: u16 = 3;
/// Move scalar input into a scalar register.
pub const S_MOV_B64: u16 = 4;
/// Move scalar input into a scalar register iff SCC is nonzero.
pub const S_CMOV_B32: u16 = 5;
/// Move scalar input into a scalar register iff SCC is nonzero.
pub const S_CMOV_B64: u16 = 6;
/// Calculate bitwise negation on a scalar input, store the result into a scalar register and set SCC iff the result is n...
pub const S_NOT_B32: u16 = 7;
/// Calculate bitwise negation on a scalar input, store the result into a scalar register and set SCC iff the result is n...
pub const S_NOT_B64: u16 = 8;
/// Given an active pixel mask in a scalar input, calculate whole quad mode mask for that input, store the result into a ...
pub const S_WQM_B32: u16 = 9;
/// Given an active pixel mask in a scalar input, calculate whole quad mode mask for that input, store the result into a ...
pub const S_WQM_B64: u16 = 10;
/// Reverse the order of bits in a scalar input and store the result into a scalar register.
pub const S_BREV_B32: u16 = 11;
/// Reverse the order of bits in a scalar input and store the result into a scalar register.
pub const S_BREV_B64: u16 = 12;
/// Count the number of \"0\" bits in a scalar input, store the result into a scalar register and set SCC iff the result ...
pub const S_BCNT0_I32_B32: u16 = 13;
/// Count the number of \"0\" bits in a scalar input, store the result into a scalar register and set SCC iff the result ...
pub const S_BCNT0_I32_B64: u16 = 14;
/// Count the number of \"1\" bits in a scalar input, store the result into a scalar register and set SCC iff the result ...
pub const S_BCNT1_I32_B32: u16 = 15;
/// Count the number of \"1\" bits in a scalar input, store the result into a scalar register and set SCC iff the result ...
pub const S_BCNT1_I32_B64: u16 = 16;
/// Count the number of trailing \"1\" bits before the first \"0\" in a scalar input and store the result into a scalar r...
pub const S_FF0_I32_B32: u16 = 17;
/// Count the number of trailing \"1\" bits before the first \"0\" in a scalar input and store the result into a scalar r...
pub const S_FF0_I32_B64: u16 = 18;
/// Count the number of trailing \"0\" bits before the first \"1\" in a scalar input and store the result into a scalar r...
pub const S_FF1_I32_B32: u16 = 19;
/// Count the number of trailing \"0\" bits before the first \"1\" in a scalar input and store the result into a scalar r...
pub const S_FF1_I32_B64: u16 = 20;
/// Count the number of leading \"0\" bits before the first \"1\" in a scalar input and store the result into a scalar re...
pub const S_FLBIT_I32_B32: u16 = 21;
/// Count the number of leading \"0\" bits before the first \"1\" in a scalar input and store the result into a scalar re...
pub const S_FLBIT_I32_B64: u16 = 22;
/// Count the number of leading bits that are the same as the sign bit of a scalar input and store the result into a scal...
pub const S_FLBIT_I32: u16 = 23;
/// Count the number of leading bits that are the same as the sign bit of a scalar input and store the result into a scal...
pub const S_FLBIT_I32_I64: u16 = 24;
/// Sign extend a signed 8 bit scalar input to 32 bits and store the result into a scalar register.
pub const S_SEXT_I32_I8: u16 = 25;
/// Sign extend a signed 16 bit scalar input to 32 bits and store the result into a scalar register.
pub const S_SEXT_I32_I16: u16 = 26;
/// Given a bit offset in a scalar input, set the indicated bit in the destination scalar register to 0.
pub const S_BITSET0_B32: u16 = 27;
/// Given a bit offset in a scalar input, set the indicated bit in the destination scalar register to 0.
pub const S_BITSET0_B64: u16 = 28;
/// Given a bit offset in a scalar input, set the indicated bit in the destination scalar register to 1.
pub const S_BITSET1_B32: u16 = 29;
/// Given a bit offset in a scalar input, set the indicated bit in the destination scalar register to 1.
pub const S_BITSET1_B64: u16 = 30;
/// Store the address of the next instruction to a scalar register.
pub const S_GETPC_B64: u16 = 31;
/// Jump to an address specified in a scalar register.
pub const S_SETPC_B64: u16 = 32;
/// Store the address of the next instruction to a scalar register and then jump to an address specified in the scalar in...
pub const S_SWAPPC_B64: u16 = 33;
/// Return from the exception handler. Clear the wave's PRIV bit and then jump to an address specified by the scalar input.
pub const S_RFE_B64: u16 = 34;
/// Calculate bitwise AND on the scalar input and the EXEC mask, store the calculated result into the EXEC mask, set SCC ...
pub const S_AND_SAVEEXEC_B64: u16 = 36;
/// Calculate bitwise OR on the scalar input and the EXEC mask, store the calculated result into the EXEC mask, set SCC i...
pub const S_OR_SAVEEXEC_B64: u16 = 37;
/// Calculate bitwise XOR on the scalar input and the EXEC mask, store the calculated result into the EXEC mask, set SCC ...
pub const S_XOR_SAVEEXEC_B64: u16 = 38;
/// Calculate bitwise AND on the scalar input and the negation of the EXEC mask, store the calculated result into the EXE...
pub const S_ANDN2_SAVEEXEC_B64: u16 = 39;
/// Calculate bitwise OR on the scalar input and the negation of the EXEC mask, store the calculated result into the EXEC...
pub const S_ORN2_SAVEEXEC_B64: u16 = 40;
/// Calculate bitwise NAND on the scalar input and the EXEC mask, store the calculated result into the EXEC mask, set SCC...
pub const S_NAND_SAVEEXEC_B64: u16 = 41;
/// Calculate bitwise NOR on the scalar input and the EXEC mask, store the calculated result into the EXEC mask, set SCC ...
pub const S_NOR_SAVEEXEC_B64: u16 = 42;
/// Calculate bitwise XNOR on the scalar input and the EXEC mask, store the calculated result into the EXEC mask, set SCC...
pub const S_XNOR_SAVEEXEC_B64: u16 = 43;
/// Reduce a pixel mask from the scalar input into a quad mask, store the result in a scalar register and set SCC iff the...
pub const S_QUADMASK_B32: u16 = 44;
/// Reduce a pixel mask from the scalar input into a quad mask, store the result in a scalar register and set SCC iff the...
pub const S_QUADMASK_B64: u16 = 45;
/// Move data from a relatively-indexed scalar register into another scalar register.
pub const S_MOVRELS_B32: u16 = 46;
/// Move data from a relatively-indexed scalar register into another scalar register.
pub const S_MOVRELS_B64: u16 = 47;
/// Move data from a scalar input into a relatively-indexed scalar register.
pub const S_MOVRELD_B32: u16 = 48;
/// Move data from a scalar input into a relatively-indexed scalar register.
pub const S_MOVRELD_B64: u16 = 49;
/// Compute the absolute value of a scalar input, store the result into a scalar register and set SCC iff the result is n...
pub const S_ABS_I32: u16 = 52;
/// Calculate bitwise AND on the EXEC mask and the negation of the scalar input, store the calculated result into the EXE...
pub const S_ANDN1_SAVEEXEC_B64: u16 = 55;
/// Calculate bitwise OR on the EXEC mask and the negation of the scalar input, store the calculated result into the EXEC...
pub const S_ORN1_SAVEEXEC_B64: u16 = 56;
/// Calculate bitwise AND on the EXEC mask and the negation of the scalar input, store the calculated result into the EXE...
pub const S_ANDN1_WREXEC_B64: u16 = 57;
/// Calculate bitwise AND on the scalar input and the negation of the EXEC mask, store the calculated result into the EXE...
pub const S_ANDN2_WREXEC_B64: u16 = 58;
/// Substitute each bit of a 32 bit scalar input with two instances of itself and store the result into a 64 bit scalar r...
pub const S_BITREPLICATE_B64_B32: u16 = 59;
/// Calculate bitwise AND on the scalar input and the EXEC mask, store the calculated result into the EXEC mask, set SCC ...
pub const S_AND_SAVEEXEC_B32: u16 = 60;
/// Calculate bitwise OR on the scalar input and the EXEC mask, store the calculated result into the EXEC mask, set SCC i...
pub const S_OR_SAVEEXEC_B32: u16 = 61;
/// Calculate bitwise XOR on the scalar input and the EXEC mask, store the calculated result into the EXEC mask, set SCC ...
pub const S_XOR_SAVEEXEC_B32: u16 = 62;
/// Calculate bitwise AND on the scalar input and the negation of the EXEC mask, store the calculated result into the EXE...
pub const S_ANDN2_SAVEEXEC_B32: u16 = 63;
/// Calculate bitwise OR on the scalar input and the negation of the EXEC mask, store the calculated result into the EXEC...
pub const S_ORN2_SAVEEXEC_B32: u16 = 64;
/// Calculate bitwise NAND on the scalar input and the EXEC mask, store the calculated result into the EXEC mask, set SCC...
pub const S_NAND_SAVEEXEC_B32: u16 = 65;
/// Calculate bitwise NOR on the scalar input and the EXEC mask, store the calculated result into the EXEC mask, set SCC ...
pub const S_NOR_SAVEEXEC_B32: u16 = 66;
/// Calculate bitwise XNOR on the scalar input and the EXEC mask, store the calculated result into the EXEC mask, set SCC...
pub const S_XNOR_SAVEEXEC_B32: u16 = 67;
/// Calculate bitwise AND on the EXEC mask and the negation of the scalar input, store the calculated result into the EXE...
pub const S_ANDN1_SAVEEXEC_B32: u16 = 68;
/// Calculate bitwise OR on the EXEC mask and the negation of the scalar input, store the calculated result into the EXEC...
pub const S_ORN1_SAVEEXEC_B32: u16 = 69;
/// Calculate bitwise AND on the EXEC mask and the negation of the scalar input, store the calculated result into the EXE...
pub const S_ANDN1_WREXEC_B32: u16 = 70;
/// Calculate bitwise AND on the scalar input and the negation of the EXEC mask, store the calculated result into the EXE...
pub const S_ANDN2_WREXEC_B32: u16 = 71;
/// Move data from a relatively-indexed scalar register into another relatively-indexed scalar register, using different ...
pub const S_MOVRELSD_2_B32: u16 = 73;

/// All ENC_SOP1 instructions.
pub const TABLE: &[InstrEntry] = &[
    InstrEntry {
        name: "S_MOV_B32",
        opcode: 3,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_MOV_B64",
        opcode: 4,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_CMOV_B32",
        opcode: 5,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_CMOV_B64",
        opcode: 6,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_NOT_B32",
        opcode: 7,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_NOT_B64",
        opcode: 8,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_WQM_B32",
        opcode: 9,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_WQM_B64",
        opcode: 10,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_BREV_B32",
        opcode: 11,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_BREV_B64",
        opcode: 12,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_BCNT0_I32_B32",
        opcode: 13,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_BCNT0_I32_B64",
        opcode: 14,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_BCNT1_I32_B32",
        opcode: 15,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_BCNT1_I32_B64",
        opcode: 16,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_FF0_I32_B32",
        opcode: 17,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_FF0_I32_B64",
        opcode: 18,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_FF1_I32_B32",
        opcode: 19,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_FF1_I32_B64",
        opcode: 20,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_FLBIT_I32_B32",
        opcode: 21,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_FLBIT_I32_B64",
        opcode: 22,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_FLBIT_I32",
        opcode: 23,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_FLBIT_I32_I64",
        opcode: 24,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_SEXT_I32_I8",
        opcode: 25,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_SEXT_I32_I16",
        opcode: 26,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_BITSET0_B32",
        opcode: 27,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_BITSET0_B64",
        opcode: 28,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_BITSET1_B32",
        opcode: 29,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_BITSET1_B64",
        opcode: 30,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_GETPC_B64",
        opcode: 31,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_SETPC_B64",
        opcode: 32,
        is_branch: true,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_SWAPPC_B64",
        opcode: 33,
        is_branch: true,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_RFE_B64",
        opcode: 34,
        is_branch: true,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_AND_SAVEEXEC_B64",
        opcode: 36,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_OR_SAVEEXEC_B64",
        opcode: 37,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_XOR_SAVEEXEC_B64",
        opcode: 38,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_ANDN2_SAVEEXEC_B64",
        opcode: 39,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_ORN2_SAVEEXEC_B64",
        opcode: 40,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_NAND_SAVEEXEC_B64",
        opcode: 41,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_NOR_SAVEEXEC_B64",
        opcode: 42,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_XNOR_SAVEEXEC_B64",
        opcode: 43,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_QUADMASK_B32",
        opcode: 44,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_QUADMASK_B64",
        opcode: 45,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_MOVRELS_B32",
        opcode: 46,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_MOVRELS_B64",
        opcode: 47,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_MOVRELD_B32",
        opcode: 48,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_MOVRELD_B64",
        opcode: 49,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_ABS_I32",
        opcode: 52,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_ANDN1_SAVEEXEC_B64",
        opcode: 55,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_ORN1_SAVEEXEC_B64",
        opcode: 56,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_ANDN1_WREXEC_B64",
        opcode: 57,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_ANDN2_WREXEC_B64",
        opcode: 58,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_BITREPLICATE_B64_B32",
        opcode: 59,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_AND_SAVEEXEC_B32",
        opcode: 60,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_OR_SAVEEXEC_B32",
        opcode: 61,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_XOR_SAVEEXEC_B32",
        opcode: 62,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_ANDN2_SAVEEXEC_B32",
        opcode: 63,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_ORN2_SAVEEXEC_B32",
        opcode: 64,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_NAND_SAVEEXEC_B32",
        opcode: 65,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_NOR_SAVEEXEC_B32",
        opcode: 66,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_XNOR_SAVEEXEC_B32",
        opcode: 67,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_ANDN1_SAVEEXEC_B32",
        opcode: 68,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_ORN1_SAVEEXEC_B32",
        opcode: 69,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_ANDN1_WREXEC_B32",
        opcode: 70,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_ANDN2_WREXEC_B32",
        opcode: 71,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_MOVRELSD_2_B32",
        opcode: 73,
        is_branch: false,
        is_terminator: false,
    },
];

/// Look up an instruction by opcode.
#[must_use]
pub fn lookup(opcode: u16) -> Option<&'static InstrEntry> {
    TABLE.iter().find(|e| e.opcode == opcode)
}
