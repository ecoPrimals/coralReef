// SPDX-License-Identifier: AGPL-3.0-only
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
