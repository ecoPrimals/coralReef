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
        name: "V_CVT_PKRTZ_F16_F32",
        opcode: 303,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_LDEXP_F16",
        opcode: 315,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CVT_PK_U8_F32",
        opcode: 350,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_LDEXP_F64",
        opcode: 360,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_TRIG_PREOP_F64",
        opcode: 372,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CVT_I32_F64",
        opcode: 387,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CVT_F64_I32",
        opcode: 388,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CVT_F32_I32",
        opcode: 389,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CVT_F32_U32",
        opcode: 390,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CVT_U32_F32",
        opcode: 391,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CVT_I32_F32",
        opcode: 392,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CVT_F16_F32",
        opcode: 394,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CVT_F32_F16",
        opcode: 395,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CVT_RPI_I32_F32",
        opcode: 396,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CVT_FLR_I32_F32",
        opcode: 397,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CVT_OFF_F32_I4",
        opcode: 398,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CVT_F32_F64",
        opcode: 399,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CVT_F64_F32",
        opcode: 400,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CVT_F32_UBYTE0",
        opcode: 401,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CVT_F32_UBYTE1",
        opcode: 402,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CVT_F32_UBYTE2",
        opcode: 403,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CVT_F32_UBYTE3",
        opcode: 404,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CVT_U32_F64",
        opcode: 405,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CVT_F64_U32",
        opcode: 406,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_TRUNC_F64",
        opcode: 407,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CEIL_F64",
        opcode: 408,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_RNDNE_F64",
        opcode: 409,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_FLOOR_F64",
        opcode: 410,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_FRACT_F32",
        opcode: 416,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_TRUNC_F32",
        opcode: 417,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CEIL_F32",
        opcode: 418,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_RNDNE_F32",
        opcode: 419,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_FLOOR_F32",
        opcode: 420,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_EXP_F32",
        opcode: 421,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_LOG_F32",
        opcode: 423,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_RCP_F32",
        opcode: 426,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_RCP_IFLAG_F32",
        opcode: 427,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_RSQ_F32",
        opcode: 430,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_RCP_F64",
        opcode: 431,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_RSQ_F64",
        opcode: 433,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_SQRT_F32",
        opcode: 435,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_SQRT_F64",
        opcode: 436,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_SIN_F32",
        opcode: 437,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_COS_F32",
        opcode: 438,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_FREXP_EXP_I32_F64",
        opcode: 444,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_FREXP_MANT_F64",
        opcode: 445,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_FRACT_F64",
        opcode: 446,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_FREXP_EXP_I32_F32",
        opcode: 447,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_FREXP_MANT_F32",
        opcode: 448,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CVT_F16_U16",
        opcode: 464,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CVT_F16_I16",
        opcode: 465,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CVT_U16_F16",
        opcode: 466,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CVT_I16_F16",
        opcode: 467,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_RCP_F16",
        opcode: 468,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_SQRT_F16",
        opcode: 469,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_RSQ_F16",
        opcode: 470,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_LOG_F16",
        opcode: 471,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_EXP_F16",
        opcode: 472,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_FREXP_MANT_F16",
        opcode: 473,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_FREXP_EXP_I16_F16",
        opcode: 474,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_FLOOR_F16",
        opcode: 475,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CEIL_F16",
        opcode: 476,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_TRUNC_F16",
        opcode: 477,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_RNDNE_F16",
        opcode: 478,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_FRACT_F16",
        opcode: 479,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_SIN_F16",
        opcode: 480,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_COS_F16",
        opcode: 481,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CVT_NORM_I16_F16",
        opcode: 483,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CVT_NORM_U16_F16",
        opcode: 484,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CVT_PKNORM_I16_F16",
        opcode: 786,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CVT_PKNORM_U16_F16",
        opcode: 787,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_LDEXP_F32",
        opcode: 866,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CVT_PKNORM_I16_F32",
        opcode: 872,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CVT_PKNORM_U16_F32",
        opcode: 873,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CVT_PK_U16_U32",
        opcode: 874,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "V_CVT_PK_I16_I32",
        opcode: 875,
        is_branch: false,
        is_terminator: false,
    },
];

#[must_use]
pub fn lookup(opcode: u16) -> Option<&'static InstrEntry> {
    TABLE.iter().find(|e| e.opcode == opcode)
}

/// Convert two single-precision float inputs to a packed half-precision float value using round toward zero semantics (i...
pub const V_CVT_PKRTZ_F16_F32: u16 = 303;
/// Multiply the first input, a floating point value, by an integral power of 2 specified in the second input, a signed i...
pub const V_LDEXP_F16: u16 = 315;
/// Convert a single-precision float value from the first input to an unsigned 8-bit integer value and pack the result in...
pub const V_CVT_PK_U8_F32: u16 = 350;
/// Multiply the first input, a floating point value, by an integral power of 2 specified in the second input, a signed i...
pub const V_LDEXP_F64: u16 = 360;
/// Look up a 53-bit segment of 2/PI using an integer segment select in the second input. Scale the intermediate result b...
pub const V_TRIG_PREOP_F64: u16 = 372;
/// Convert from a double-precision float input to a signed 32-bit integer value and store the result into a vector regis...
pub const V_CVT_I32_F64: u16 = 387;
/// Convert from a signed 32-bit integer input to a double-precision float value and store the result into a vector regis...
pub const V_CVT_F64_I32: u16 = 388;
/// Convert from a signed 32-bit integer input to a single-precision float value and store the result into a vector regis...
pub const V_CVT_F32_I32: u16 = 389;
/// Convert from an unsigned 32-bit integer input to a single-precision float value and store the result into a vector re...
pub const V_CVT_F32_U32: u16 = 390;
/// Convert from a single-precision float input to an unsigned 32-bit integer value and store the result into a vector re...
pub const V_CVT_U32_F32: u16 = 391;
/// Convert from a single-precision float input to a signed 32-bit integer value and store the result into a vector regis...
pub const V_CVT_I32_F32: u16 = 392;
/// Convert from a single-precision float input to a half-precision float value and store the result into a vector register.
pub const V_CVT_F16_F32: u16 = 394;
/// Convert from a half-precision float input to a single-precision float value and store the result into a vector register.
pub const V_CVT_F32_F16: u16 = 395;
/// Convert from a single-precision float input to a signed 32-bit integer value using round to nearest integer semantics...
pub const V_CVT_RPI_I32_F32: u16 = 396;
/// Convert from a single-precision float input to a signed 32-bit integer value using round-down semantics (ignore the d...
pub const V_CVT_FLR_I32_F32: u16 = 397;
/// Convert from a signed 4-bit integer input to a single-precision float value using an offset table and store the resul...
pub const V_CVT_OFF_F32_I4: u16 = 398;
/// Convert from a double-precision float input to a single-precision float value and store the result into a vector regi...
pub const V_CVT_F32_F64: u16 = 399;
/// Convert from a single-precision float input to a double-precision float value and store the result into a vector regi...
pub const V_CVT_F64_F32: u16 = 400;
/// Convert an unsigned byte in byte 0 of the input to a single-precision float value and store the result into a vector ...
pub const V_CVT_F32_UBYTE0: u16 = 401;
/// Convert an unsigned byte in byte 1 of the input to a single-precision float value and store the result into a vector ...
pub const V_CVT_F32_UBYTE1: u16 = 402;
/// Convert an unsigned byte in byte 2 of the input to a single-precision float value and store the result into a vector ...
pub const V_CVT_F32_UBYTE2: u16 = 403;
/// Convert an unsigned byte in byte 3 of the input to a single-precision float value and store the result into a vector ...
pub const V_CVT_F32_UBYTE3: u16 = 404;
/// Convert from a double-precision float input to an unsigned 32-bit integer value and store the result into a vector re...
pub const V_CVT_U32_F64: u16 = 405;
/// Convert from an unsigned 32-bit integer input to a double-precision float value and store the result into a vector re...
pub const V_CVT_F64_U32: u16 = 406;
/// Compute the integer part of a double-precision float input using round toward zero semantics and store the result in ...
pub const V_TRUNC_F64: u16 = 407;
/// Round the double-precision float input up to next integer and store the result in floating point format into a vector...
pub const V_CEIL_F64: u16 = 408;
/// Round the double-precision float input to the nearest even integer and store the result in floating point format into...
pub const V_RNDNE_F64: u16 = 409;
/// Round the double-precision float input down to previous integer and store the result in floating point format into a ...
pub const V_FLOOR_F64: u16 = 410;
/// Compute the fractional portion of a single-precision float input and store the result in floating point format into a...
pub const V_FRACT_F32: u16 = 416;
/// Compute the integer part of a single-precision float input using round toward zero semantics and store the result in ...
pub const V_TRUNC_F32: u16 = 417;
/// Round the single-precision float input up to next integer and store the result in floating point format into a vector...
pub const V_CEIL_F32: u16 = 418;
/// Round the single-precision float input to the nearest even integer and store the result in floating point format into...
pub const V_RNDNE_F32: u16 = 419;
/// Round the single-precision float input down to previous integer and store the result in floating point format into a ...
pub const V_FLOOR_F32: u16 = 420;
/// Calculate 2 raised to the power of the single-precision float input and store the result into a vector register.
pub const V_EXP_F32: u16 = 421;
/// Calculate the base 2 logarithm of the single-precision float input and store the result into a vector register.
pub const V_LOG_F32: u16 = 423;
/// Calculate the reciprocal of the single-precision float input using IEEE rules and store the result into a vector regi...
pub const V_RCP_F32: u16 = 426;
/// Calculate the reciprocal of the vector float input in a manner suitable for integer division and store the result int...
pub const V_RCP_IFLAG_F32: u16 = 427;
/// Calculate the reciprocal of the square root of the single-precision float input using IEEE rules and store the result...
pub const V_RSQ_F32: u16 = 430;
/// Calculate the reciprocal of the double-precision float input using IEEE rules and store the result into a vector regi...
pub const V_RCP_F64: u16 = 431;
/// Calculate the reciprocal of the square root of the double-precision float input using IEEE rules and store the result...
pub const V_RSQ_F64: u16 = 433;
/// Calculate the square root of the single-precision float input using IEEE rules and store the result into a vector reg...
pub const V_SQRT_F32: u16 = 435;
/// Calculate the square root of the double-precision float input using IEEE rules and store the result into a vector reg...
pub const V_SQRT_F64: u16 = 436;
/// Calculate the trigonometric sine of a single-precision float value using IEEE rules and store the result into a vecto...
pub const V_SIN_F32: u16 = 437;
/// Calculate the trigonometric cosine of a single-precision float value using IEEE rules and store the result into a vec...
pub const V_COS_F32: u16 = 438;
/// Extract the exponent of a double-precision float input and store the result as a signed 32-bit integer into a vector ...
pub const V_FREXP_EXP_I32_F64: u16 = 444;
/// Extract the binary significand, or mantissa, of a double-precision float input and store the result as a double-preci...
pub const V_FREXP_MANT_F64: u16 = 445;
/// Compute the fractional portion of a double-precision float input and store the result in floating point format into a...
pub const V_FRACT_F64: u16 = 446;
/// Extract the exponent of a single-precision float input and store the result as a signed 32-bit integer into a vector ...
pub const V_FREXP_EXP_I32_F32: u16 = 447;
/// Extract the binary significand, or mantissa, of a single-precision float input and store the result as a single-preci...
pub const V_FREXP_MANT_F32: u16 = 448;
/// Convert from an unsigned 16-bit integer input to a half-precision float value and store the result into a vector regi...
pub const V_CVT_F16_U16: u16 = 464;
/// Convert from a signed 16-bit integer input to a half-precision float value and store the result into a vector register.
pub const V_CVT_F16_I16: u16 = 465;
/// Convert from a half-precision float input to an unsigned 16-bit integer value and store the result into a vector regi...
pub const V_CVT_U16_F16: u16 = 466;
/// Convert from a half-precision float input to a signed 16-bit integer value and store the result into a vector register.
pub const V_CVT_I16_F16: u16 = 467;
/// Calculate the reciprocal of the half-precision float input using IEEE rules and store the result into a vector register.
pub const V_RCP_F16: u16 = 468;
/// Calculate the square root of the half-precision float input using IEEE rules and store the result into a vector regis...
pub const V_SQRT_F16: u16 = 469;
/// Calculate the reciprocal of the square root of the half-precision float input using IEEE rules and store the result i...
pub const V_RSQ_F16: u16 = 470;
/// Calculate the base 2 logarithm of the half-precision float input and store the result into a vector register.
pub const V_LOG_F16: u16 = 471;
/// Calculate 2 raised to the power of the half-precision float input and store the result into a vector register.
pub const V_EXP_F16: u16 = 472;
/// Extract the binary significand, or mantissa, of a half-precision float input and store the result as a half-precision...
pub const V_FREXP_MANT_F16: u16 = 473;
/// Extract the exponent of a half-precision float input and store the result as a signed 16-bit integer into a vector re...
pub const V_FREXP_EXP_I16_F16: u16 = 474;
/// Round the half-precision float input down to previous integer and store the result in floating point format into a ve...
pub const V_FLOOR_F16: u16 = 475;
/// Round the half-precision float input up to next integer and store the result in floating point format into a vector r...
pub const V_CEIL_F16: u16 = 476;
/// Compute the integer part of a half-precision float input using round toward zero semantics and store the result in fl...
pub const V_TRUNC_F16: u16 = 477;
/// Round the half-precision float input to the nearest even integer and store the result in floating point format into a...
pub const V_RNDNE_F16: u16 = 478;
/// Compute the fractional portion of a half-precision float input and store the result in floating point format into a v...
pub const V_FRACT_F16: u16 = 479;
/// Calculate the trigonometric sine of a half-precision float value using IEEE rules and store the result into a vector ...
pub const V_SIN_F16: u16 = 480;
/// Calculate the trigonometric cosine of a half-precision float value using IEEE rules and store the result into a vecto...
pub const V_COS_F16: u16 = 481;
/// Convert from a half-precision float input to a signed normalized short and store the result into a vector register.
pub const V_CVT_NORM_I16_F16: u16 = 483;
/// Convert from a half-precision float input to an unsigned normalized short and store the result into a vector register.
pub const V_CVT_NORM_U16_F16: u16 = 484;
/// Convert from two half-precision float inputs to a packed signed normalized short and store the result into a vector r...
pub const V_CVT_PKNORM_I16_F16: u16 = 786;
/// Convert from two half-precision float inputs to a packed unsigned normalized short and store the result into a vector...
pub const V_CVT_PKNORM_U16_F16: u16 = 787;
/// Multiply the first input, a floating point value, by an integral power of 2 specified in the second input, a signed i...
pub const V_LDEXP_F32: u16 = 866;
/// Convert from two single-precision float inputs to a packed signed normalized short and store the result into a vector...
pub const V_CVT_PKNORM_I16_F32: u16 = 872;
/// Convert from two single-precision float inputs to a packed unsigned normalized short and store the result into a vect...
pub const V_CVT_PKNORM_U16_F32: u16 = 873;
/// Convert from two unsigned 32-bit integer inputs to a packed unsigned 16-bit integer value and store the result into a...
pub const V_CVT_PK_U16_U32: u16 = 874;
/// Convert from two signed 32-bit integer inputs to a packed signed 16-bit integer value and store the result into a vec...
pub const V_CVT_PK_I16_I32: u16 = 875;
