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

/// ENC_VOP1 encoding fields (32 bits).
pub mod fields {
    use super::BitField;
    pub const SRC0: BitField = BitField { offset: 0, width: 9 };
    pub const OP: BitField = BitField { offset: 9, width: 8 };
    pub const VDST: BitField = BitField { offset: 17, width: 8 };
    pub const ENCODING: BitField = BitField { offset: 25, width: 7 };
}

/// Do nothing.
pub const V_NOP: u16 = 0;
/// Move data from a vector input into a vector register.
pub const V_MOV_B32: u16 = 1;
/// Read the scalar value in the lowest active lane of the input vector register and store it into a scalar register.
pub const V_READFIRSTLANE_B32: u16 = 2;
/// Convert from a double-precision float input to a signed 32-bit integer value and store the result into a vector regis...
pub const V_CVT_I32_F64: u16 = 3;
/// Convert from a signed 32-bit integer input to a double-precision float value and store the result into a vector regis...
pub const V_CVT_F64_I32: u16 = 4;
/// Convert from a signed 32-bit integer input to a single-precision float value and store the result into a vector regis...
pub const V_CVT_F32_I32: u16 = 5;
/// Convert from an unsigned 32-bit integer input to a single-precision float value and store the result into a vector re...
pub const V_CVT_F32_U32: u16 = 6;
/// Convert from a single-precision float input to an unsigned 32-bit integer value and store the result into a vector re...
pub const V_CVT_U32_F32: u16 = 7;
/// Convert from a single-precision float input to a signed 32-bit integer value and store the result into a vector regis...
pub const V_CVT_I32_F32: u16 = 8;
/// Convert from a single-precision float input to a half-precision float value and store the result into a vector register.
pub const V_CVT_F16_F32: u16 = 10;
/// Convert from a half-precision float input to a single-precision float value and store the result into a vector register.
pub const V_CVT_F32_F16: u16 = 11;
/// Convert from a single-precision float input to a signed 32-bit integer value using round to nearest integer semantics...
pub const V_CVT_RPI_I32_F32: u16 = 12;
/// Convert from a single-precision float input to a signed 32-bit integer value using round-down semantics (ignore the d...
pub const V_CVT_FLR_I32_F32: u16 = 13;
/// Convert from a signed 4-bit integer input to a single-precision float value using an offset table and store the resul...
pub const V_CVT_OFF_F32_I4: u16 = 14;
/// Convert from a double-precision float input to a single-precision float value and store the result into a vector regi...
pub const V_CVT_F32_F64: u16 = 15;
/// Convert from a single-precision float input to a double-precision float value and store the result into a vector regi...
pub const V_CVT_F64_F32: u16 = 16;
/// Convert an unsigned byte in byte 0 of the input to a single-precision float value and store the result into a vector ...
pub const V_CVT_F32_UBYTE0: u16 = 17;
/// Convert an unsigned byte in byte 1 of the input to a single-precision float value and store the result into a vector ...
pub const V_CVT_F32_UBYTE1: u16 = 18;
/// Convert an unsigned byte in byte 2 of the input to a single-precision float value and store the result into a vector ...
pub const V_CVT_F32_UBYTE2: u16 = 19;
/// Convert an unsigned byte in byte 3 of the input to a single-precision float value and store the result into a vector ...
pub const V_CVT_F32_UBYTE3: u16 = 20;
/// Convert from a double-precision float input to an unsigned 32-bit integer value and store the result into a vector re...
pub const V_CVT_U32_F64: u16 = 21;
/// Convert from an unsigned 32-bit integer input to a double-precision float value and store the result into a vector re...
pub const V_CVT_F64_U32: u16 = 22;
/// Compute the integer part of a double-precision float input using round toward zero semantics and store the result in ...
pub const V_TRUNC_F64: u16 = 23;
/// Round the double-precision float input up to next integer and store the result in floating point format into a vector...
pub const V_CEIL_F64: u16 = 24;
/// Round the double-precision float input to the nearest even integer and store the result in floating point format into...
pub const V_RNDNE_F64: u16 = 25;
/// Round the double-precision float input down to previous integer and store the result in floating point format into a ...
pub const V_FLOOR_F64: u16 = 26;
/// Flush the vector ALU pipeline through the destination cache.
pub const V_PIPEFLUSH: u16 = 27;
/// Compute the fractional portion of a single-precision float input and store the result in floating point format into a...
pub const V_FRACT_F32: u16 = 32;
/// Compute the integer part of a single-precision float input using round toward zero semantics and store the result in ...
pub const V_TRUNC_F32: u16 = 33;
/// Round the single-precision float input up to next integer and store the result in floating point format into a vector...
pub const V_CEIL_F32: u16 = 34;
/// Round the single-precision float input to the nearest even integer and store the result in floating point format into...
pub const V_RNDNE_F32: u16 = 35;
/// Round the single-precision float input down to previous integer and store the result in floating point format into a ...
pub const V_FLOOR_F32: u16 = 36;
/// Calculate 2 raised to the power of the single-precision float input and store the result into a vector register.
pub const V_EXP_F32: u16 = 37;
/// Calculate the base 2 logarithm of the single-precision float input and store the result into a vector register.
pub const V_LOG_F32: u16 = 39;
/// Calculate the reciprocal of the single-precision float input using IEEE rules and store the result into a vector regi...
pub const V_RCP_F32: u16 = 42;
/// Calculate the reciprocal of the vector float input in a manner suitable for integer division and store the result int...
pub const V_RCP_IFLAG_F32: u16 = 43;
/// Calculate the reciprocal of the square root of the single-precision float input using IEEE rules and store the result...
pub const V_RSQ_F32: u16 = 46;
/// Calculate the reciprocal of the double-precision float input using IEEE rules and store the result into a vector regi...
pub const V_RCP_F64: u16 = 47;
/// Calculate the reciprocal of the square root of the double-precision float input using IEEE rules and store the result...
pub const V_RSQ_F64: u16 = 49;
/// Calculate the square root of the single-precision float input using IEEE rules and store the result into a vector reg...
pub const V_SQRT_F32: u16 = 51;
/// Calculate the square root of the double-precision float input using IEEE rules and store the result into a vector reg...
pub const V_SQRT_F64: u16 = 52;
/// Calculate the trigonometric sine of a single-precision float value using IEEE rules and store the result into a vecto...
pub const V_SIN_F32: u16 = 53;
/// Calculate the trigonometric cosine of a single-precision float value using IEEE rules and store the result into a vec...
pub const V_COS_F32: u16 = 54;
/// Calculate bitwise negation on a vector input and store the result into a vector register.
pub const V_NOT_B32: u16 = 55;
/// Reverse the order of bits in a vector input and store the result into a vector register.
pub const V_BFREV_B32: u16 = 56;
/// Count the number of leading \"0\" bits before the first \"1\" in a vector input and store the result into a vector re...
pub const V_FFBH_U32: u16 = 57;
/// Count the number of trailing \"0\" bits before the first \"1\" in a vector input and store the result into a vector r...
pub const V_FFBL_B32: u16 = 58;
/// Count the number of leading bits that are the same as the sign bit of a vector input and store the result into a vect...
pub const V_FFBH_I32: u16 = 59;
/// Extract the exponent of a double-precision float input and store the result as a signed 32-bit integer into a vector ...
pub const V_FREXP_EXP_I32_F64: u16 = 60;
/// Extract the binary significand, or mantissa, of a double-precision float input and store the result as a double-preci...
pub const V_FREXP_MANT_F64: u16 = 61;
/// Compute the fractional portion of a double-precision float input and store the result in floating point format into a...
pub const V_FRACT_F64: u16 = 62;
/// Extract the exponent of a single-precision float input and store the result as a signed 32-bit integer into a vector ...
pub const V_FREXP_EXP_I32_F32: u16 = 63;
/// Extract the binary significand, or mantissa, of a single-precision float input and store the result as a single-preci...
pub const V_FREXP_MANT_F32: u16 = 64;
/// Clear this wave's exception state in the vector ALU.
pub const V_CLREXCP: u16 = 65;
/// Move data from a vector input into a relatively-indexed vector register.
pub const V_MOVRELD_B32: u16 = 66;
/// Move data from a relatively-indexed vector register into another vector register.
pub const V_MOVRELS_B32: u16 = 67;
/// Move data from a relatively-indexed vector register into another relatively-indexed vector register.
pub const V_MOVRELSD_B32: u16 = 68;
/// Move data from a relatively-indexed vector register into another relatively-indexed vector register, using different ...
pub const V_MOVRELSD_2_B32: u16 = 72;
/// Convert from an unsigned 16-bit integer input to a half-precision float value and store the result into a vector regi...
pub const V_CVT_F16_U16: u16 = 80;
/// Convert from a signed 16-bit integer input to a half-precision float value and store the result into a vector register.
pub const V_CVT_F16_I16: u16 = 81;
/// Convert from a half-precision float input to an unsigned 16-bit integer value and store the result into a vector regi...
pub const V_CVT_U16_F16: u16 = 82;
/// Convert from a half-precision float input to a signed 16-bit integer value and store the result into a vector register.
pub const V_CVT_I16_F16: u16 = 83;
/// Calculate the reciprocal of the half-precision float input using IEEE rules and store the result into a vector register.
pub const V_RCP_F16: u16 = 84;
/// Calculate the square root of the half-precision float input using IEEE rules and store the result into a vector regis...
pub const V_SQRT_F16: u16 = 85;
/// Calculate the reciprocal of the square root of the half-precision float input using IEEE rules and store the result i...
pub const V_RSQ_F16: u16 = 86;
/// Calculate the base 2 logarithm of the half-precision float input and store the result into a vector register.
pub const V_LOG_F16: u16 = 87;
/// Calculate 2 raised to the power of the half-precision float input and store the result into a vector register.
pub const V_EXP_F16: u16 = 88;
/// Extract the binary significand, or mantissa, of a half-precision float input and store the result as a half-precision...
pub const V_FREXP_MANT_F16: u16 = 89;
/// Extract the exponent of a half-precision float input and store the result as a signed 16-bit integer into a vector re...
pub const V_FREXP_EXP_I16_F16: u16 = 90;
/// Round the half-precision float input down to previous integer and store the result in floating point format into a ve...
pub const V_FLOOR_F16: u16 = 91;
/// Round the half-precision float input up to next integer and store the result in floating point format into a vector r...
pub const V_CEIL_F16: u16 = 92;
/// Compute the integer part of a half-precision float input using round toward zero semantics and store the result in fl...
pub const V_TRUNC_F16: u16 = 93;
/// Round the half-precision float input to the nearest even integer and store the result in floating point format into a...
pub const V_RNDNE_F16: u16 = 94;
/// Compute the fractional portion of a half-precision float input and store the result in floating point format into a v...
pub const V_FRACT_F16: u16 = 95;
/// Calculate the trigonometric sine of a half-precision float value using IEEE rules and store the result into a vector ...
pub const V_SIN_F16: u16 = 96;
/// Calculate the trigonometric cosine of a half-precision float value using IEEE rules and store the result into a vecto...
pub const V_COS_F16: u16 = 97;
/// Given two 16-bit unsigned integer inputs, saturate each input over an 8-bit unsigned range, pack the resulting values...
pub const V_SAT_PK_U8_I16: u16 = 98;
/// Convert from a half-precision float input to a signed normalized short and store the result into a vector register.
pub const V_CVT_NORM_I16_F16: u16 = 99;
/// Convert from a half-precision float input to an unsigned normalized short and store the result into a vector register.
pub const V_CVT_NORM_U16_F16: u16 = 100;
/// Swap the values in two vector registers.
pub const V_SWAP_B32: u16 = 101;
/// Swap the values in two relatively-indexed vector registers.
pub const V_SWAPREL_B32: u16 = 104;

/// All ENC_VOP1 instructions.
pub const TABLE: &[InstrEntry] = &[
    InstrEntry { name: "V_NOP", opcode: 0, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_MOV_B32", opcode: 1, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_READFIRSTLANE_B32", opcode: 2, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_CVT_I32_F64", opcode: 3, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_CVT_F64_I32", opcode: 4, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_CVT_F32_I32", opcode: 5, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_CVT_F32_U32", opcode: 6, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_CVT_U32_F32", opcode: 7, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_CVT_I32_F32", opcode: 8, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_CVT_F16_F32", opcode: 10, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_CVT_F32_F16", opcode: 11, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_CVT_RPI_I32_F32", opcode: 12, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_CVT_FLR_I32_F32", opcode: 13, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_CVT_OFF_F32_I4", opcode: 14, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_CVT_F32_F64", opcode: 15, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_CVT_F64_F32", opcode: 16, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_CVT_F32_UBYTE0", opcode: 17, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_CVT_F32_UBYTE1", opcode: 18, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_CVT_F32_UBYTE2", opcode: 19, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_CVT_F32_UBYTE3", opcode: 20, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_CVT_U32_F64", opcode: 21, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_CVT_F64_U32", opcode: 22, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_TRUNC_F64", opcode: 23, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_CEIL_F64", opcode: 24, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_RNDNE_F64", opcode: 25, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_FLOOR_F64", opcode: 26, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_PIPEFLUSH", opcode: 27, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_FRACT_F32", opcode: 32, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_TRUNC_F32", opcode: 33, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_CEIL_F32", opcode: 34, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_RNDNE_F32", opcode: 35, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_FLOOR_F32", opcode: 36, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_EXP_F32", opcode: 37, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_LOG_F32", opcode: 39, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_RCP_F32", opcode: 42, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_RCP_IFLAG_F32", opcode: 43, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_RSQ_F32", opcode: 46, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_RCP_F64", opcode: 47, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_RSQ_F64", opcode: 49, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_SQRT_F32", opcode: 51, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_SQRT_F64", opcode: 52, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_SIN_F32", opcode: 53, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_COS_F32", opcode: 54, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_NOT_B32", opcode: 55, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_BFREV_B32", opcode: 56, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_FFBH_U32", opcode: 57, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_FFBL_B32", opcode: 58, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_FFBH_I32", opcode: 59, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_FREXP_EXP_I32_F64", opcode: 60, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_FREXP_MANT_F64", opcode: 61, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_FRACT_F64", opcode: 62, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_FREXP_EXP_I32_F32", opcode: 63, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_FREXP_MANT_F32", opcode: 64, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_CLREXCP", opcode: 65, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_MOVRELD_B32", opcode: 66, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_MOVRELS_B32", opcode: 67, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_MOVRELSD_B32", opcode: 68, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_MOVRELSD_2_B32", opcode: 72, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_CVT_F16_U16", opcode: 80, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_CVT_F16_I16", opcode: 81, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_CVT_U16_F16", opcode: 82, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_CVT_I16_F16", opcode: 83, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_RCP_F16", opcode: 84, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_SQRT_F16", opcode: 85, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_RSQ_F16", opcode: 86, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_LOG_F16", opcode: 87, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_EXP_F16", opcode: 88, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_FREXP_MANT_F16", opcode: 89, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_FREXP_EXP_I16_F16", opcode: 90, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_FLOOR_F16", opcode: 91, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_CEIL_F16", opcode: 92, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_TRUNC_F16", opcode: 93, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_RNDNE_F16", opcode: 94, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_FRACT_F16", opcode: 95, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_SIN_F16", opcode: 96, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_COS_F16", opcode: 97, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_SAT_PK_U8_I16", opcode: 98, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_CVT_NORM_I16_F16", opcode: 99, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_CVT_NORM_U16_F16", opcode: 100, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_SWAP_B32", opcode: 101, is_branch: false, is_terminator: false },
    InstrEntry { name: "V_SWAPREL_B32", opcode: 104, is_branch: false, is_terminator: false },
];

/// Look up an instruction by opcode.
#[must_use]
pub fn lookup(opcode: u16) -> Option<&'static InstrEntry> {
    TABLE.iter().find(|e| e.opcode == opcode)
}

