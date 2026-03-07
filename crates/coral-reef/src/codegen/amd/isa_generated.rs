// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2026 ecoPrimals
//! AUTO-GENERATED from AMD RDNA2 ISA XML specification.
//!
//! Source: specs/amd/amdgpu_isa_rdna2.xml (MIT license, AMD GPUOpen)
//! Generator: tools/amd-isa-gen (pure Rust, sovereign toolchain)
//!
//! DO NOT EDIT BY HAND. Regenerate with:
//!   cargo run -p amd-isa-gen

#![allow(dead_code, missing_docs)]

/// Bit field within an encoding format.
#[derive(Debug, Clone, Copy)]
pub struct BitField {
    /// Bit offset within the instruction word(s).
    pub offset: u32,
    /// Number of bits.
    pub width: u32,
}

/// ENC_DS encoding fields (64 bits).
pub mod ds_fields {
    use super::BitField;
    pub const OFFSET0: BitField = BitField {
        offset: 0,
        width: 8,
    };
    pub const OFFSET1: BitField = BitField {
        offset: 8,
        width: 8,
    };
    pub const GDS: BitField = BitField {
        offset: 17,
        width: 1,
    };
    pub const OP: BitField = BitField {
        offset: 18,
        width: 8,
    };
    pub const ENCODING: BitField = BitField {
        offset: 26,
        width: 6,
    };
    pub const ADDR: BitField = BitField {
        offset: 32,
        width: 8,
    };
    pub const DATA0: BitField = BitField {
        offset: 40,
        width: 8,
    };
    pub const DATA1: BitField = BitField {
        offset: 48,
        width: 8,
    };
    pub const VDST: BitField = BitField {
        offset: 56,
        width: 8,
    };
}

/// ENC_FLAT encoding fields (64 bits).
pub mod flat_fields {
    use super::BitField;
    pub const OFFSET: BitField = BitField {
        offset: 0,
        width: 12,
    };
    pub const DLC: BitField = BitField {
        offset: 12,
        width: 1,
    };
    pub const LDS: BitField = BitField {
        offset: 13,
        width: 1,
    };
    pub const SEG: BitField = BitField {
        offset: 14,
        width: 2,
    };
    pub const GLC: BitField = BitField {
        offset: 16,
        width: 1,
    };
    pub const SLC: BitField = BitField {
        offset: 17,
        width: 1,
    };
    pub const OP: BitField = BitField {
        offset: 18,
        width: 7,
    };
    pub const ENCODING: BitField = BitField {
        offset: 26,
        width: 6,
    };
    pub const ADDR: BitField = BitField {
        offset: 32,
        width: 8,
    };
    pub const DATA: BitField = BitField {
        offset: 40,
        width: 8,
    };
    pub const SADDR: BitField = BitField {
        offset: 48,
        width: 7,
    };
    pub const VDST: BitField = BitField {
        offset: 56,
        width: 8,
    };
}

/// ENC_FLAT_GLBL encoding fields (64 bits).
pub mod flat_glbl_fields {
    use super::BitField;
    pub const OFFSET: BitField = BitField {
        offset: 0,
        width: 12,
    };
    pub const DLC: BitField = BitField {
        offset: 12,
        width: 1,
    };
    pub const LDS: BitField = BitField {
        offset: 13,
        width: 1,
    };
    pub const SEG: BitField = BitField {
        offset: 14,
        width: 2,
    };
    pub const GLC: BitField = BitField {
        offset: 16,
        width: 1,
    };
    pub const SLC: BitField = BitField {
        offset: 17,
        width: 1,
    };
    pub const OP: BitField = BitField {
        offset: 18,
        width: 7,
    };
    pub const ENCODING: BitField = BitField {
        offset: 26,
        width: 6,
    };
    pub const ADDR: BitField = BitField {
        offset: 32,
        width: 8,
    };
    pub const DATA: BitField = BitField {
        offset: 40,
        width: 8,
    };
    pub const SADDR: BitField = BitField {
        offset: 48,
        width: 7,
    };
    pub const VDST: BitField = BitField {
        offset: 56,
        width: 8,
    };
}

/// ENC_FLAT_SCRATCH encoding fields (64 bits).
pub mod flat_scratch_fields {
    use super::BitField;
    pub const OFFSET: BitField = BitField {
        offset: 0,
        width: 12,
    };
    pub const DLC: BitField = BitField {
        offset: 12,
        width: 1,
    };
    pub const LDS: BitField = BitField {
        offset: 13,
        width: 1,
    };
    pub const SEG: BitField = BitField {
        offset: 14,
        width: 2,
    };
    pub const GLC: BitField = BitField {
        offset: 16,
        width: 1,
    };
    pub const SLC: BitField = BitField {
        offset: 17,
        width: 1,
    };
    pub const OP: BitField = BitField {
        offset: 18,
        width: 7,
    };
    pub const ENCODING: BitField = BitField {
        offset: 26,
        width: 6,
    };
    pub const ADDR: BitField = BitField {
        offset: 32,
        width: 8,
    };
    pub const DATA: BitField = BitField {
        offset: 40,
        width: 8,
    };
    pub const SADDR: BitField = BitField {
        offset: 48,
        width: 7,
    };
    pub const VDST: BitField = BitField {
        offset: 56,
        width: 8,
    };
}

/// ENC_MIMG encoding fields (64 bits).
pub mod mimg_fields {
    use super::BitField;
    pub const OPM: BitField = BitField {
        offset: 0,
        width: 1,
    };
    pub const NSA: BitField = BitField {
        offset: 1,
        width: 2,
    };
    pub const DIM: BitField = BitField {
        offset: 3,
        width: 3,
    };
    pub const DLC: BitField = BitField {
        offset: 7,
        width: 1,
    };
    pub const DMASK: BitField = BitField {
        offset: 8,
        width: 4,
    };
    pub const UNORM: BitField = BitField {
        offset: 12,
        width: 1,
    };
    pub const GLC: BitField = BitField {
        offset: 13,
        width: 1,
    };
    pub const DA: BitField = BitField {
        offset: 14,
        width: 1,
    };
    pub const R128: BitField = BitField {
        offset: 15,
        width: 1,
    };
    pub const TFE: BitField = BitField {
        offset: 16,
        width: 1,
    };
    pub const LWE: BitField = BitField {
        offset: 17,
        width: 1,
    };
    pub const OP: BitField = BitField {
        offset: 18,
        width: 7,
    };
    pub const SLC: BitField = BitField {
        offset: 25,
        width: 1,
    };
    pub const ENCODING: BitField = BitField {
        offset: 26,
        width: 6,
    };
    pub const VADDR: BitField = BitField {
        offset: 32,
        width: 8,
    };
    pub const VDATA: BitField = BitField {
        offset: 40,
        width: 8,
    };
    pub const SRSRC: BitField = BitField {
        offset: 48,
        width: 2,
    };
    pub const SSAMP: BitField = BitField {
        offset: 53,
        width: 2,
    };
    pub const A16: BitField = BitField {
        offset: 62,
        width: 1,
    };
    pub const D16: BitField = BitField {
        offset: 63,
        width: 1,
    };
}

/// ENC_MTBUF encoding fields (64 bits).
pub mod mtbuf_fields {
    use super::BitField;
    pub const OFFSET: BitField = BitField {
        offset: 0,
        width: 12,
    };
    pub const OFFEN: BitField = BitField {
        offset: 12,
        width: 1,
    };
    pub const IDXEN: BitField = BitField {
        offset: 13,
        width: 1,
    };
    pub const GLC: BitField = BitField {
        offset: 14,
        width: 1,
    };
    pub const DLC: BitField = BitField {
        offset: 15,
        width: 1,
    };
    pub const OP: BitField = BitField {
        offset: 16,
        width: 3,
    };
    pub const FORMAT: BitField = BitField {
        offset: 19,
        width: 7,
    };
    pub const ENCODING: BitField = BitField {
        offset: 26,
        width: 6,
    };
    pub const VADDR: BitField = BitField {
        offset: 32,
        width: 8,
    };
    pub const VDATA: BitField = BitField {
        offset: 40,
        width: 8,
    };
    pub const SRSRC: BitField = BitField {
        offset: 48,
        width: 2,
    };
    pub const OPM: BitField = BitField {
        offset: 53,
        width: 1,
    };
    pub const SLC: BitField = BitField {
        offset: 54,
        width: 1,
    };
    pub const TFE: BitField = BitField {
        offset: 55,
        width: 1,
    };
    pub const SOFFSET: BitField = BitField {
        offset: 56,
        width: 8,
    };
}

/// ENC_MUBUF encoding fields (64 bits).
pub mod mubuf_fields {
    use super::BitField;
    pub const OFFSET: BitField = BitField {
        offset: 0,
        width: 12,
    };
    pub const OFFEN: BitField = BitField {
        offset: 12,
        width: 1,
    };
    pub const IDXEN: BitField = BitField {
        offset: 13,
        width: 1,
    };
    pub const GLC: BitField = BitField {
        offset: 14,
        width: 1,
    };
    pub const DLC: BitField = BitField {
        offset: 15,
        width: 1,
    };
    pub const LDS: BitField = BitField {
        offset: 16,
        width: 1,
    };
    pub const OP: BitField = BitField {
        offset: 18,
        width: 7,
    };
    pub const OPM: BitField = BitField {
        offset: 25,
        width: 1,
    };
    pub const ENCODING: BitField = BitField {
        offset: 26,
        width: 6,
    };
    pub const VADDR: BitField = BitField {
        offset: 32,
        width: 8,
    };
    pub const VDATA: BitField = BitField {
        offset: 40,
        width: 8,
    };
    pub const SRSRC: BitField = BitField {
        offset: 48,
        width: 2,
    };
    pub const SLC: BitField = BitField {
        offset: 54,
        width: 1,
    };
    pub const TFE: BitField = BitField {
        offset: 55,
        width: 1,
    };
    pub const SOFFSET: BitField = BitField {
        offset: 56,
        width: 8,
    };
}

/// ENC_SMEM encoding fields (64 bits).
pub mod smem_fields {
    use super::BitField;
    pub const SBASE: BitField = BitField {
        offset: 0,
        width: 1,
    };
    pub const SDATA: BitField = BitField {
        offset: 6,
        width: 7,
    };
    pub const DLC: BitField = BitField {
        offset: 14,
        width: 1,
    };
    pub const GLC: BitField = BitField {
        offset: 16,
        width: 1,
    };
    pub const OP: BitField = BitField {
        offset: 18,
        width: 8,
    };
    pub const ENCODING: BitField = BitField {
        offset: 26,
        width: 6,
    };
    pub const OFFSET: BitField = BitField {
        offset: 32,
        width: 21,
    };
    pub const SOFFSET: BitField = BitField {
        offset: 57,
        width: 7,
    };
}

/// ENC_SOP1 encoding fields (32 bits).
pub mod sop1_fields {
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

/// ENC_SOP2 encoding fields (32 bits).
pub mod sop2_fields {
    use super::BitField;
    pub const SSRC0: BitField = BitField {
        offset: 0,
        width: 8,
    };
    pub const SSRC1: BitField = BitField {
        offset: 8,
        width: 8,
    };
    pub const SDST: BitField = BitField {
        offset: 16,
        width: 7,
    };
    pub const OP: BitField = BitField {
        offset: 23,
        width: 7,
    };
    pub const ENCODING: BitField = BitField {
        offset: 30,
        width: 2,
    };
}

/// ENC_SOPC encoding fields (32 bits).
pub mod sopc_fields {
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

/// ENC_SOPK encoding fields (32 bits).
pub mod sopk_fields {
    use super::BitField;
    pub const SIMM16: BitField = BitField {
        offset: 0,
        width: 16,
    };
    pub const SDST: BitField = BitField {
        offset: 16,
        width: 7,
    };
    pub const OP: BitField = BitField {
        offset: 23,
        width: 5,
    };
    pub const ENCODING: BitField = BitField {
        offset: 28,
        width: 4,
    };
}

/// ENC_SOPP encoding fields (32 bits).
pub mod sopp_fields {
    use super::BitField;
    pub const SIMM16: BitField = BitField {
        offset: 0,
        width: 16,
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

/// ENC_VOP1 encoding fields (32 bits).
pub mod vop1_fields {
    use super::BitField;
    pub const SRC0: BitField = BitField {
        offset: 0,
        width: 9,
    };
    pub const OP: BitField = BitField {
        offset: 9,
        width: 8,
    };
    pub const VDST: BitField = BitField {
        offset: 17,
        width: 8,
    };
    pub const ENCODING: BitField = BitField {
        offset: 25,
        width: 7,
    };
}

/// ENC_VOP2 encoding fields (32 bits).
pub mod vop2_fields {
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

/// ENC_VOP3 encoding fields (64 bits).
pub mod vop3_fields {
    use super::BitField;
    pub const VDST: BitField = BitField {
        offset: 0,
        width: 8,
    };
    pub const ABS: BitField = BitField {
        offset: 8,
        width: 3,
    };
    pub const OP_SEL: BitField = BitField {
        offset: 11,
        width: 4,
    };
    pub const CLAMP: BitField = BitField {
        offset: 15,
        width: 1,
    };
    pub const OP: BitField = BitField {
        offset: 16,
        width: 10,
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
    pub const OMOD: BitField = BitField {
        offset: 59,
        width: 2,
    };
    pub const NEG: BitField = BitField {
        offset: 61,
        width: 3,
    };
}

/// ENC_VOP3P encoding fields (64 bits).
pub mod vop3p_fields {
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

/// ENC_VOPC encoding fields (32 bits).
pub mod vopc_fields {
    use super::BitField;
    pub const SRC0: BitField = BitField {
        offset: 0,
        width: 9,
    };
    pub const VSRC1: BitField = BitField {
        offset: 9,
        width: 8,
    };
    pub const OP: BitField = BitField {
        offset: 17,
        width: 8,
    };
    pub const ENCODING: BitField = BitField {
        offset: 25,
        width: 7,
    };
}

/// Instruction entry in the opcode table.
#[derive(Debug, Clone, Copy)]
pub struct InstrEntry {
    /// Instruction mnemonic.
    pub name: &'static str,
    /// Numeric opcode within the encoding format.
    pub opcode: u16,
    /// Whether this instruction is a branch.
    pub is_branch: bool,
    /// Whether this instruction terminates the program.
    pub is_terminator: bool,
}

/// ENC_DS opcodes (123 instructions).
pub mod ds {
    /// Add two unsigned 32-bit integer values stored in the data register and a location in a data share.
    pub const DS_ADD_U32: u16 = 0;
    /// Subtract an unsigned 32-bit integer value stored in the data register from a value stored in a location in a data share.
    pub const DS_SUB_U32: u16 = 1;
    /// Subtract an unsigned 32-bit integer value stored in a location in a data share from a value stored in the data register.
    pub const DS_RSUB_U32: u16 = 2;
    /// Increment an unsigned 32-bit integer value from a location in a data share with wraparound to 0 if the value exceeds ...
    pub const DS_INC_U32: u16 = 3;
    /// Decrement an unsigned 32-bit integer value from a location in a data share with wraparound to a value in the data reg...
    pub const DS_DEC_U32: u16 = 4;
    /// Select the minimum of two signed 32-bit integer inputs, given two values stored in the data register and a location i...
    pub const DS_MIN_I32: u16 = 5;
    /// Select the maximum of two signed 32-bit integer inputs, given two values stored in the data register and a location i...
    pub const DS_MAX_I32: u16 = 6;
    /// Select the minimum of two unsigned 32-bit integer inputs, given two values stored in the data register and a location...
    pub const DS_MIN_U32: u16 = 7;
    /// Select the maximum of two unsigned 32-bit integer inputs, given two values stored in the data register and a location...
    pub const DS_MAX_U32: u16 = 8;
    /// Calculate bitwise AND given two unsigned 32-bit integer values stored in the data register and a location in a data s...
    pub const DS_AND_B32: u16 = 9;
    /// Calculate bitwise OR given two unsigned 32-bit integer values stored in the data register and a location in a data sh...
    pub const DS_OR_B32: u16 = 10;
    /// Calculate bitwise XOR given two unsigned 32-bit integer values stored in the data register and a location in a data s...
    pub const DS_XOR_B32: u16 = 11;
    /// Calculate masked bitwise OR on an unsigned 32-bit integer location in a data share, given mask value and bits to OR i...
    pub const DS_MSKOR_B32: u16 = 12;
    /// Store 32 bits of data from a vector input register into a data share.
    pub const DS_WRITE_B32: u16 = 13;
    /// Store 32 bits of data from one vector input register and then 32 bits of data from a second vector input register int...
    pub const DS_WRITE2_B32: u16 = 14;
    /// Store 32 bits of data from one vector input register and then 32 bits of data from a second vector input register int...
    pub const DS_WRITE2ST64_B32: u16 = 15;
    /// Compare an unsigned 32-bit integer value in the data comparison register with a location in a data share, and modify ...
    pub const DS_CMPST_B32: u16 = 16;
    /// Compare a single-precision float value in the data comparison register with a location in a data share, and modify th...
    pub const DS_CMPST_F32: u16 = 17;
    /// Select the minimum of two single-precision float inputs, given two values stored in the data register and a location ...
    pub const DS_MIN_F32: u16 = 18;
    /// Select the maximum of two single-precision float inputs, given two values stored in the data register and a location ...
    pub const DS_MAX_F32: u16 = 19;
    /// Do nothing.
    pub const DS_NOP: u16 = 20;
    /// Add two single-precision float values stored in the data register and a location in a data share.
    pub const DS_ADD_F32: u16 = 21;
    /// GDS Only: The GWS resource (rid) indicated processes this opcode by updating the counter and labeling the specified r...
    pub const DS_GWS_SEMA_RELEASE_ALL: u16 = 24;
    /// GDS Only: Initialize a barrier or semaphore resource.
    pub const DS_GWS_INIT: u16 = 25;
    /// GDS Only: The GWS resource indicated processes this opcode by updating the counter and labeling the resource as a sem...
    pub const DS_GWS_SEMA_V: u16 = 26;
    /// GDS Only: The GWS resource indicated processes this opcode by updating the counter by the bulk release delivered coun...
    pub const DS_GWS_SEMA_BR: u16 = 27;
    /// GDS Only: The GWS resource indicated processes this opcode by queueing it until counter enables a release and then de...
    pub const DS_GWS_SEMA_P: u16 = 28;
    /// GDS Only: The GWS resource indicated processes this opcode by queueing it until barrier is satisfied. The number of w...
    pub const DS_GWS_BARRIER: u16 = 29;
    /// Store 8 bits of data from a vector register into a data share.
    pub const DS_WRITE_B8: u16 = 30;
    /// Store 16 bits of data from a vector register into a data share.
    pub const DS_WRITE_B16: u16 = 31;
    /// Add two unsigned 32-bit integer values stored in the data register and a location in a data share. Store the original...
    pub const DS_ADD_RTN_U32: u16 = 32;
    /// Subtract an unsigned 32-bit integer value stored in the data register from a value stored in a location in a data sha...
    pub const DS_SUB_RTN_U32: u16 = 33;
    /// Subtract an unsigned 32-bit integer value stored in a location in a data share from a value stored in the data regist...
    pub const DS_RSUB_RTN_U32: u16 = 34;
    /// Increment an unsigned 32-bit integer value from a location in a data share with wraparound to 0 if the value exceeds ...
    pub const DS_INC_RTN_U32: u16 = 35;
    /// Decrement an unsigned 32-bit integer value from a location in a data share with wraparound to a value in the data reg...
    pub const DS_DEC_RTN_U32: u16 = 36;
    /// Select the minimum of two signed 32-bit integer inputs, given two values stored in the data register and a location i...
    pub const DS_MIN_RTN_I32: u16 = 37;
    /// Select the maximum of two signed 32-bit integer inputs, given two values stored in the data register and a location i...
    pub const DS_MAX_RTN_I32: u16 = 38;
    /// Select the minimum of two unsigned 32-bit integer inputs, given two values stored in the data register and a location...
    pub const DS_MIN_RTN_U32: u16 = 39;
    /// Select the maximum of two unsigned 32-bit integer inputs, given two values stored in the data register and a location...
    pub const DS_MAX_RTN_U32: u16 = 40;
    /// Calculate bitwise AND given two unsigned 32-bit integer values stored in the data register and a location in a data s...
    pub const DS_AND_RTN_B32: u16 = 41;
    /// Calculate bitwise OR given two unsigned 32-bit integer values stored in the data register and a location in a data sh...
    pub const DS_OR_RTN_B32: u16 = 42;
    /// Calculate bitwise XOR given two unsigned 32-bit integer values stored in the data register and a location in a data s...
    pub const DS_XOR_RTN_B32: u16 = 43;
    /// Calculate masked bitwise OR on an unsigned 32-bit integer location in a data share, given mask value and bits to OR i...
    pub const DS_MSKOR_RTN_B32: u16 = 44;
    /// Swap an unsigned 32-bit integer value in the data register with a location in a data share.
    pub const DS_WRXCHG_RTN_B32: u16 = 45;
    /// Swap two unsigned 32-bit integer values in the data registers with two locations in a data share.
    pub const DS_WRXCHG2_RTN_B32: u16 = 46;
    /// Swap two unsigned 32-bit integer values in the data registers with two locations in a data share. Treat each offset a...
    pub const DS_WRXCHG2ST64_RTN_B32: u16 = 47;
    /// Compare an unsigned 32-bit integer value in the data comparison register with a location in a data share, and modify ...
    pub const DS_CMPST_RTN_B32: u16 = 48;
    /// Compare a single-precision float value in the data comparison register with a location in a data share, and modify th...
    pub const DS_CMPST_RTN_F32: u16 = 49;
    /// Select the minimum of two single-precision float inputs, given two values stored in the data register and a location ...
    pub const DS_MIN_RTN_F32: u16 = 50;
    /// Select the maximum of two single-precision float inputs, given two values stored in the data register and a location ...
    pub const DS_MAX_RTN_F32: u16 = 51;
    /// Given a minuend from a location in data share and a subtrahend from a vector register, subtract the two values iff th...
    pub const DS_WRAP_RTN_B32: u16 = 52;
    /// Dword swizzle, no data is written to LDS memory.
    pub const DS_SWIZZLE_B32: u16 = 53;
    /// Load 32 bits of data from a data share into a vector register.
    pub const DS_READ_B32: u16 = 54;
    /// Load 32 bits of data from one location in a data share and then 32 bits of data from a second location in a data shar...
    pub const DS_READ2_B32: u16 = 55;
    /// Load 32 bits of data from one location in a data share and then 32 bits of data from a second location in a data shar...
    pub const DS_READ2ST64_B32: u16 = 56;
    /// Load 8 bits of signed data from a data share, sign extend to 32 bits and store the result into a vector register.
    pub const DS_READ_I8: u16 = 57;
    /// Load 8 bits of unsigned data from a data share, zero extend to 32 bits and store the result into a vector register.
    pub const DS_READ_U8: u16 = 58;
    /// Load 16 bits of signed data from a data share, sign extend to 32 bits and store the result into a vector register.
    pub const DS_READ_I16: u16 = 59;
    /// Load 16 bits of unsigned data from a data share, zero extend to 32 bits and store the result into a vector register.
    pub const DS_READ_U16: u16 = 60;
    /// LDS & GDS. Subtract (count_bits(exec_mask)) from the value stored in DS memory at (M0.base + instr_offset). Return th...
    pub const DS_CONSUME: u16 = 61;
    /// LDS & GDS. Add (count_bits(exec_mask)) to the value stored in DS memory at (M0.base + instr_offset). Return the pre-o...
    pub const DS_APPEND: u16 = 62;
    /// GDS-only. Add (count_bits(exec_mask)) to one of 4 dedicated ordered-count counters (aka 'packers'). Additional bits o...
    pub const DS_ORDERED_COUNT: u16 = 63;
    /// Add two unsigned 64-bit integer values stored in the data register and a location in a data share.
    pub const DS_ADD_U64: u16 = 64;
    /// Subtract an unsigned 64-bit integer value stored in the data register from a value stored in a location in a data share.
    pub const DS_SUB_U64: u16 = 65;
    /// Subtract an unsigned 64-bit integer value stored in a location in a data share from a value stored in the data register.
    pub const DS_RSUB_U64: u16 = 66;
    /// Increment an unsigned 64-bit integer value from a location in a data share with wraparound to 0 if the value exceeds ...
    pub const DS_INC_U64: u16 = 67;
    /// Decrement an unsigned 64-bit integer value from a location in a data share with wraparound to a value in the data reg...
    pub const DS_DEC_U64: u16 = 68;
    /// Select the minimum of two signed 64-bit integer inputs, given two values stored in the data register and a location i...
    pub const DS_MIN_I64: u16 = 69;
    /// Select the maximum of two signed 64-bit integer inputs, given two values stored in the data register and a location i...
    pub const DS_MAX_I64: u16 = 70;
    /// Select the minimum of two unsigned 64-bit integer inputs, given two values stored in the data register and a location...
    pub const DS_MIN_U64: u16 = 71;
    /// Select the maximum of two unsigned 64-bit integer inputs, given two values stored in the data register and a location...
    pub const DS_MAX_U64: u16 = 72;
    /// Calculate bitwise AND given two unsigned 64-bit integer values stored in the data register and a location in a data s...
    pub const DS_AND_B64: u16 = 73;
    /// Calculate bitwise OR given two unsigned 64-bit integer values stored in the data register and a location in a data sh...
    pub const DS_OR_B64: u16 = 74;
    /// Calculate bitwise XOR given two unsigned 64-bit integer values stored in the data register and a location in a data s...
    pub const DS_XOR_B64: u16 = 75;
    /// Calculate masked bitwise OR on an unsigned 64-bit integer location in a data share, given mask value and bits to OR i...
    pub const DS_MSKOR_B64: u16 = 76;
    /// Store 64 bits of data from a vector input register into a data share.
    pub const DS_WRITE_B64: u16 = 77;
    /// Store 64 bits of data from one vector input register and then 64 bits of data from a second vector input register int...
    pub const DS_WRITE2_B64: u16 = 78;
    /// Store 64 bits of data from one vector input register and then 64 bits of data from a second vector input register int...
    pub const DS_WRITE2ST64_B64: u16 = 79;
    /// Compare an unsigned 64-bit integer value in the data comparison register with a location in a data share, and modify ...
    pub const DS_CMPST_B64: u16 = 80;
    /// Compare a double-precision float value in the data comparison register with a location in a data share, and modify th...
    pub const DS_CMPST_F64: u16 = 81;
    /// Select the minimum of two double-precision float inputs, given two values stored in the data register and a location ...
    pub const DS_MIN_F64: u16 = 82;
    /// Select the maximum of two double-precision float inputs, given two values stored in the data register and a location ...
    pub const DS_MAX_F64: u16 = 83;
    /// Add two single-precision float values stored in the data register and a location in a data share. Store the original ...
    pub const DS_ADD_RTN_F32: u16 = 85;
    /// Add two unsigned 64-bit integer values stored in the data register and a location in a data share. Store the original...
    pub const DS_ADD_RTN_U64: u16 = 96;
    /// Subtract an unsigned 64-bit integer value stored in the data register from a value stored in a location in a data sha...
    pub const DS_SUB_RTN_U64: u16 = 97;
    /// Subtract an unsigned 64-bit integer value stored in a location in a data share from a value stored in the data regist...
    pub const DS_RSUB_RTN_U64: u16 = 98;
    /// Increment an unsigned 64-bit integer value from a location in a data share with wraparound to 0 if the value exceeds ...
    pub const DS_INC_RTN_U64: u16 = 99;
    /// Decrement an unsigned 64-bit integer value from a location in a data share with wraparound to a value in the data reg...
    pub const DS_DEC_RTN_U64: u16 = 100;
    /// Select the minimum of two signed 64-bit integer inputs, given two values stored in the data register and a location i...
    pub const DS_MIN_RTN_I64: u16 = 101;
    /// Select the maximum of two signed 64-bit integer inputs, given two values stored in the data register and a location i...
    pub const DS_MAX_RTN_I64: u16 = 102;
    /// Select the minimum of two unsigned 64-bit integer inputs, given two values stored in the data register and a location...
    pub const DS_MIN_RTN_U64: u16 = 103;
    /// Select the maximum of two unsigned 64-bit integer inputs, given two values stored in the data register and a location...
    pub const DS_MAX_RTN_U64: u16 = 104;
    /// Calculate bitwise AND given two unsigned 64-bit integer values stored in the data register and a location in a data s...
    pub const DS_AND_RTN_B64: u16 = 105;
    /// Calculate bitwise OR given two unsigned 64-bit integer values stored in the data register and a location in a data sh...
    pub const DS_OR_RTN_B64: u16 = 106;
    /// Calculate bitwise XOR given two unsigned 64-bit integer values stored in the data register and a location in a data s...
    pub const DS_XOR_RTN_B64: u16 = 107;
    /// Calculate masked bitwise OR on an unsigned 64-bit integer location in a data share, given mask value and bits to OR i...
    pub const DS_MSKOR_RTN_B64: u16 = 108;
    /// Swap an unsigned 64-bit integer value in the data register with a location in a data share.
    pub const DS_WRXCHG_RTN_B64: u16 = 109;
    /// Swap two unsigned 64-bit integer values in the data registers with two locations in a data share.
    pub const DS_WRXCHG2_RTN_B64: u16 = 110;
    /// Swap two unsigned 64-bit integer values in the data registers with two locations in a data share. Treat each offset a...
    pub const DS_WRXCHG2ST64_RTN_B64: u16 = 111;
    /// Compare an unsigned 64-bit integer value in the data comparison register with a location in a data share, and modify ...
    pub const DS_CMPST_RTN_B64: u16 = 112;
    /// Compare a double-precision float value in the data comparison register with a location in a data share, and modify th...
    pub const DS_CMPST_RTN_F64: u16 = 113;
    /// Select the minimum of two double-precision float inputs, given two values stored in the data register and a location ...
    pub const DS_MIN_RTN_F64: u16 = 114;
    /// Select the maximum of two double-precision float inputs, given two values stored in the data register and a location ...
    pub const DS_MAX_RTN_F64: u16 = 115;
    /// Load 64 bits of data from a data share into a vector register.
    pub const DS_READ_B64: u16 = 118;
    /// Load 64 bits of data from one location in a data share and then 64 bits of data from a second location in a data shar...
    pub const DS_READ2_B64: u16 = 119;
    /// Load 64 bits of data from one location in a data share and then 64 bits of data from a second location in a data shar...
    pub const DS_READ2ST64_B64: u16 = 120;
    /// Perform 2 conditional write exchanges, where each conditional write exchange writes a 32 bit value from a data regist...
    pub const DS_CONDXCHG32_RTN_B64: u16 = 126;
    /// Store 8 bits of data from the high bits of a vector register into a data share.
    pub const DS_WRITE_B8_D16_HI: u16 = 160;
    /// Store 16 bits of data from the high bits of a vector register into a data share.
    pub const DS_WRITE_B16_D16_HI: u16 = 161;
    /// Load 8 bits of unsigned data from a data share, zero extend to 16 bits and store the result into the low 16 bits of a...
    pub const DS_READ_U8_D16: u16 = 162;
    /// Load 8 bits of unsigned data from a data share, zero extend to 16 bits and store the result into the high 16 bits of ...
    pub const DS_READ_U8_D16_HI: u16 = 163;
    /// Load 8 bits of signed data from a data share, sign extend to 16 bits and store the result into the low 16 bits of a v...
    pub const DS_READ_I8_D16: u16 = 164;
    /// Load 8 bits of signed data from a data share, sign extend to 16 bits and store the result into the high 16 bits of a ...
    pub const DS_READ_I8_D16_HI: u16 = 165;
    /// Load 16 bits of unsigned data from a data share and store the result into the low 16 bits of a vector register.
    pub const DS_READ_U16_D16: u16 = 166;
    /// Load 16 bits of unsigned data from a data share and store the result into the high 16 bits of a vector register.
    pub const DS_READ_U16_D16_HI: u16 = 167;
    /// Store 32 bits of data from a vector input register into a data share. The memory base address is provided as an immed...
    pub const DS_WRITE_ADDTID_B32: u16 = 176;
    /// Load 32 bits of data from a data share into a vector register. The memory base address is provided as an immediate va...
    pub const DS_READ_ADDTID_B32: u16 = 177;
    /// Forward permute. This does not access LDS memory and may be called even if no LDS memory is allocated to the wave. It...
    pub const DS_PERMUTE_B32: u16 = 178;
    /// Backward permute. This does not access LDS memory and may be called even if no LDS memory is allocated to the wave. I...
    pub const DS_BPERMUTE_B32: u16 = 179;
    /// Store 96 bits of data from a vector input register into a data share.
    pub const DS_WRITE_B96: u16 = 222;
    /// Store 128 bits of data from a vector input register into a data share.
    pub const DS_WRITE_B128: u16 = 223;
    /// Load 96 bits of data from a data share into a vector register.
    pub const DS_READ_B96: u16 = 254;
    /// Load 128 bits of data from a data share into a vector register.
    pub const DS_READ_B128: u16 = 255;

    /// All ENC_DS instructions.
    pub const TABLE: &[super::InstrEntry] = &[
        super::InstrEntry {
            name: "DS_ADD_U32",
            opcode: 0,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_SUB_U32",
            opcode: 1,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_RSUB_U32",
            opcode: 2,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_INC_U32",
            opcode: 3,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_DEC_U32",
            opcode: 4,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_MIN_I32",
            opcode: 5,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_MAX_I32",
            opcode: 6,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_MIN_U32",
            opcode: 7,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_MAX_U32",
            opcode: 8,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_AND_B32",
            opcode: 9,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_OR_B32",
            opcode: 10,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_XOR_B32",
            opcode: 11,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_MSKOR_B32",
            opcode: 12,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_WRITE_B32",
            opcode: 13,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_WRITE2_B32",
            opcode: 14,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_WRITE2ST64_B32",
            opcode: 15,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_CMPST_B32",
            opcode: 16,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_CMPST_F32",
            opcode: 17,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_MIN_F32",
            opcode: 18,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_MAX_F32",
            opcode: 19,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_NOP",
            opcode: 20,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_ADD_F32",
            opcode: 21,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_GWS_SEMA_RELEASE_ALL",
            opcode: 24,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_GWS_INIT",
            opcode: 25,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_GWS_SEMA_V",
            opcode: 26,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_GWS_SEMA_BR",
            opcode: 27,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_GWS_SEMA_P",
            opcode: 28,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_GWS_BARRIER",
            opcode: 29,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_WRITE_B8",
            opcode: 30,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_WRITE_B16",
            opcode: 31,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_ADD_RTN_U32",
            opcode: 32,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_SUB_RTN_U32",
            opcode: 33,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_RSUB_RTN_U32",
            opcode: 34,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_INC_RTN_U32",
            opcode: 35,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_DEC_RTN_U32",
            opcode: 36,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_MIN_RTN_I32",
            opcode: 37,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_MAX_RTN_I32",
            opcode: 38,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_MIN_RTN_U32",
            opcode: 39,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_MAX_RTN_U32",
            opcode: 40,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_AND_RTN_B32",
            opcode: 41,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_OR_RTN_B32",
            opcode: 42,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_XOR_RTN_B32",
            opcode: 43,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_MSKOR_RTN_B32",
            opcode: 44,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_WRXCHG_RTN_B32",
            opcode: 45,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_WRXCHG2_RTN_B32",
            opcode: 46,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_WRXCHG2ST64_RTN_B32",
            opcode: 47,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_CMPST_RTN_B32",
            opcode: 48,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_CMPST_RTN_F32",
            opcode: 49,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_MIN_RTN_F32",
            opcode: 50,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_MAX_RTN_F32",
            opcode: 51,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_WRAP_RTN_B32",
            opcode: 52,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_SWIZZLE_B32",
            opcode: 53,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_READ_B32",
            opcode: 54,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_READ2_B32",
            opcode: 55,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_READ2ST64_B32",
            opcode: 56,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_READ_I8",
            opcode: 57,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_READ_U8",
            opcode: 58,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_READ_I16",
            opcode: 59,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_READ_U16",
            opcode: 60,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_CONSUME",
            opcode: 61,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_APPEND",
            opcode: 62,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_ORDERED_COUNT",
            opcode: 63,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_ADD_U64",
            opcode: 64,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_SUB_U64",
            opcode: 65,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_RSUB_U64",
            opcode: 66,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_INC_U64",
            opcode: 67,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_DEC_U64",
            opcode: 68,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_MIN_I64",
            opcode: 69,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_MAX_I64",
            opcode: 70,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_MIN_U64",
            opcode: 71,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_MAX_U64",
            opcode: 72,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_AND_B64",
            opcode: 73,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_OR_B64",
            opcode: 74,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_XOR_B64",
            opcode: 75,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_MSKOR_B64",
            opcode: 76,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_WRITE_B64",
            opcode: 77,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_WRITE2_B64",
            opcode: 78,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_WRITE2ST64_B64",
            opcode: 79,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_CMPST_B64",
            opcode: 80,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_CMPST_F64",
            opcode: 81,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_MIN_F64",
            opcode: 82,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_MAX_F64",
            opcode: 83,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_ADD_RTN_F32",
            opcode: 85,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_ADD_RTN_U64",
            opcode: 96,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_SUB_RTN_U64",
            opcode: 97,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_RSUB_RTN_U64",
            opcode: 98,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_INC_RTN_U64",
            opcode: 99,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_DEC_RTN_U64",
            opcode: 100,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_MIN_RTN_I64",
            opcode: 101,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_MAX_RTN_I64",
            opcode: 102,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_MIN_RTN_U64",
            opcode: 103,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_MAX_RTN_U64",
            opcode: 104,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_AND_RTN_B64",
            opcode: 105,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_OR_RTN_B64",
            opcode: 106,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_XOR_RTN_B64",
            opcode: 107,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_MSKOR_RTN_B64",
            opcode: 108,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_WRXCHG_RTN_B64",
            opcode: 109,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_WRXCHG2_RTN_B64",
            opcode: 110,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_WRXCHG2ST64_RTN_B64",
            opcode: 111,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_CMPST_RTN_B64",
            opcode: 112,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_CMPST_RTN_F64",
            opcode: 113,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_MIN_RTN_F64",
            opcode: 114,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_MAX_RTN_F64",
            opcode: 115,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_READ_B64",
            opcode: 118,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_READ2_B64",
            opcode: 119,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_READ2ST64_B64",
            opcode: 120,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_CONDXCHG32_RTN_B64",
            opcode: 126,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_WRITE_B8_D16_HI",
            opcode: 160,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_WRITE_B16_D16_HI",
            opcode: 161,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_READ_U8_D16",
            opcode: 162,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_READ_U8_D16_HI",
            opcode: 163,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_READ_I8_D16",
            opcode: 164,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_READ_I8_D16_HI",
            opcode: 165,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_READ_U16_D16",
            opcode: 166,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_READ_U16_D16_HI",
            opcode: 167,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_WRITE_ADDTID_B32",
            opcode: 176,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_READ_ADDTID_B32",
            opcode: 177,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_PERMUTE_B32",
            opcode: 178,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_BPERMUTE_B32",
            opcode: 179,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_WRITE_B96",
            opcode: 222,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_WRITE_B128",
            opcode: 223,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_READ_B96",
            opcode: 254,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "DS_READ_B128",
            opcode: 255,
            is_branch: false,
            is_terminator: false,
        },
    ];

    /// Look up an instruction by opcode.
    pub fn lookup(opcode: u16) -> Option<&'static super::InstrEntry> {
        TABLE.iter().find(|e| e.opcode == opcode)
    }
}

/// ENC_FLAT opcodes (54 instructions).
pub mod flat {
    /// Load 8 bits of unsigned data from the flat aperture, zero extend to 32 bits and store the result into a vector register.
    pub const FLAT_LOAD_UBYTE: u16 = 8;
    /// Load 8 bits of signed data from the flat aperture, sign extend to 32 bits and store the result into a vector register.
    pub const FLAT_LOAD_SBYTE: u16 = 9;
    /// Load 16 bits of unsigned data from the flat aperture, zero extend to 32 bits and store the result into a vector regis...
    pub const FLAT_LOAD_USHORT: u16 = 10;
    /// Load 16 bits of signed data from the flat aperture, sign extend to 32 bits and store the result into a vector register.
    pub const FLAT_LOAD_SSHORT: u16 = 11;
    /// Load 32 bits of data from the flat aperture into a vector register.
    pub const FLAT_LOAD_DWORD: u16 = 12;
    /// Load 64 bits of data from the flat aperture into a vector register.
    pub const FLAT_LOAD_DWORDX2: u16 = 13;
    /// Load 128 bits of data from the flat aperture into a vector register.
    pub const FLAT_LOAD_DWORDX4: u16 = 14;
    /// Load 96 bits of data from the flat aperture into a vector register.
    pub const FLAT_LOAD_DWORDX3: u16 = 15;
    /// Store 8 bits of data from a vector register into the flat aperture.
    pub const FLAT_STORE_BYTE: u16 = 24;
    /// Store 8 bits of data from the high 16 bits of a 32-bit vector register into the flat aperture.
    pub const FLAT_STORE_BYTE_D16_HI: u16 = 25;
    /// Store 16 bits of data from a vector register into the flat aperture.
    pub const FLAT_STORE_SHORT: u16 = 26;
    /// Store 16 bits of data from the high 16 bits of a 32-bit vector register into the flat aperture.
    pub const FLAT_STORE_SHORT_D16_HI: u16 = 27;
    /// Store 32 bits of data from vector input registers into the flat aperture.
    pub const FLAT_STORE_DWORD: u16 = 28;
    /// Store 64 bits of data from vector input registers into the flat aperture.
    pub const FLAT_STORE_DWORDX2: u16 = 29;
    /// Store 128 bits of data from vector input registers into the flat aperture.
    pub const FLAT_STORE_DWORDX4: u16 = 30;
    /// Store 96 bits of data from vector input registers into the flat aperture.
    pub const FLAT_STORE_DWORDX3: u16 = 31;
    /// Load 8 bits of unsigned data from the flat aperture, zero extend to 16 bits and store the result into the low 16 bits...
    pub const FLAT_LOAD_UBYTE_D16: u16 = 32;
    /// Load 8 bits of unsigned data from the flat aperture, zero extend to 16 bits and store the result into the high 16 bit...
    pub const FLAT_LOAD_UBYTE_D16_HI: u16 = 33;
    /// Load 8 bits of signed data from the flat aperture, sign extend to 16 bits and store the result into the low 16 bits o...
    pub const FLAT_LOAD_SBYTE_D16: u16 = 34;
    /// Load 8 bits of signed data from the flat aperture, sign extend to 16 bits and store the result into the high 16 bits ...
    pub const FLAT_LOAD_SBYTE_D16_HI: u16 = 35;
    /// Load 16 bits of unsigned data from the flat aperture and store the result into the low 16 bits of a 32-bit vector reg...
    pub const FLAT_LOAD_SHORT_D16: u16 = 36;
    /// Load 16 bits of unsigned data from the flat aperture and store the result into the high 16 bits of a 32-bit vector re...
    pub const FLAT_LOAD_SHORT_D16_HI: u16 = 37;
    /// Swap an unsigned 32-bit integer value in the data register with a location in the flat aperture. Store the original v...
    pub const FLAT_ATOMIC_SWAP: u16 = 48;
    /// Compare two unsigned 32-bit integer values stored in the data comparison register and a location in the flat aperture...
    pub const FLAT_ATOMIC_CMPSWAP: u16 = 49;
    /// Add two unsigned 32-bit integer values stored in the data register and a location in the flat aperture. Store the ori...
    pub const FLAT_ATOMIC_ADD: u16 = 50;
    /// Subtract an unsigned 32-bit integer value stored in the data register from a value stored in a location in the flat a...
    pub const FLAT_ATOMIC_SUB: u16 = 51;
    /// Select the minimum of two signed 32-bit integer inputs, given two values stored in the data register and a location i...
    pub const FLAT_ATOMIC_SMIN: u16 = 53;
    /// Select the minimum of two unsigned 32-bit integer inputs, given two values stored in the data register and a location...
    pub const FLAT_ATOMIC_UMIN: u16 = 54;
    /// Select the maximum of two signed 32-bit integer inputs, given two values stored in the data register and a location i...
    pub const FLAT_ATOMIC_SMAX: u16 = 55;
    /// Select the maximum of two unsigned 32-bit integer inputs, given two values stored in the data register and a location...
    pub const FLAT_ATOMIC_UMAX: u16 = 56;
    /// Calculate bitwise AND given two unsigned 32-bit integer values stored in the data register and a location in the flat...
    pub const FLAT_ATOMIC_AND: u16 = 57;
    /// Calculate bitwise OR given two unsigned 32-bit integer values stored in the data register and a location in the flat ...
    pub const FLAT_ATOMIC_OR: u16 = 58;
    /// Calculate bitwise XOR given two unsigned 32-bit integer values stored in the data register and a location in the flat...
    pub const FLAT_ATOMIC_XOR: u16 = 59;
    /// Increment an unsigned 32-bit integer value from a location in the flat aperture with wraparound to 0 if the value exc...
    pub const FLAT_ATOMIC_INC: u16 = 60;
    /// Decrement an unsigned 32-bit integer value from a location in the flat aperture with wraparound to a value in the dat...
    pub const FLAT_ATOMIC_DEC: u16 = 61;
    /// Compare two single-precision float values stored in the data comparison register and a location in the flat aperture....
    pub const FLAT_ATOMIC_FCMPSWAP: u16 = 62;
    /// Select the minimum of two single-precision float inputs, given two values stored in the data register and a location ...
    pub const FLAT_ATOMIC_FMIN: u16 = 63;
    /// Select the maximum of two single-precision float inputs, given two values stored in the data register and a location ...
    pub const FLAT_ATOMIC_FMAX: u16 = 64;
    /// Swap an unsigned 64-bit integer value in the data register with a location in the flat aperture. Store the original v...
    pub const FLAT_ATOMIC_SWAP_X2: u16 = 80;
    /// Compare two unsigned 64-bit integer values stored in the data comparison register and a location in the flat aperture...
    pub const FLAT_ATOMIC_CMPSWAP_X2: u16 = 81;
    /// Add two unsigned 64-bit integer values stored in the data register and a location in the flat aperture. Store the ori...
    pub const FLAT_ATOMIC_ADD_X2: u16 = 82;
    /// Subtract an unsigned 64-bit integer value stored in the data register from a value stored in a location in the flat a...
    pub const FLAT_ATOMIC_SUB_X2: u16 = 83;
    /// Select the minimum of two signed 64-bit integer inputs, given two values stored in the data register and a location i...
    pub const FLAT_ATOMIC_SMIN_X2: u16 = 85;
    /// Select the minimum of two unsigned 64-bit integer inputs, given two values stored in the data register and a location...
    pub const FLAT_ATOMIC_UMIN_X2: u16 = 86;
    /// Select the maximum of two signed 64-bit integer inputs, given two values stored in the data register and a location i...
    pub const FLAT_ATOMIC_SMAX_X2: u16 = 87;
    /// Select the maximum of two unsigned 64-bit integer inputs, given two values stored in the data register and a location...
    pub const FLAT_ATOMIC_UMAX_X2: u16 = 88;
    /// Calculate bitwise AND given two unsigned 64-bit integer values stored in the data register and a location in the flat...
    pub const FLAT_ATOMIC_AND_X2: u16 = 89;
    /// Calculate bitwise OR given two unsigned 64-bit integer values stored in the data register and a location in the flat ...
    pub const FLAT_ATOMIC_OR_X2: u16 = 90;
    /// Calculate bitwise XOR given two unsigned 64-bit integer values stored in the data register and a location in the flat...
    pub const FLAT_ATOMIC_XOR_X2: u16 = 91;
    /// Increment an unsigned 64-bit integer value from a location in the flat aperture with wraparound to 0 if the value exc...
    pub const FLAT_ATOMIC_INC_X2: u16 = 92;
    /// Decrement an unsigned 64-bit integer value from a location in the flat aperture with wraparound to a value in the dat...
    pub const FLAT_ATOMIC_DEC_X2: u16 = 93;
    /// Compare two double-precision float values stored in the data comparison register and a location in the flat aperture....
    pub const FLAT_ATOMIC_FCMPSWAP_X2: u16 = 94;
    /// Select the minimum of two double-precision float inputs, given two values stored in the data register and a location ...
    pub const FLAT_ATOMIC_FMIN_X2: u16 = 95;
    /// Select the maximum of two double-precision float inputs, given two values stored in the data register and a location ...
    pub const FLAT_ATOMIC_FMAX_X2: u16 = 96;

    /// All ENC_FLAT instructions.
    pub const TABLE: &[super::InstrEntry] = &[
        super::InstrEntry {
            name: "FLAT_LOAD_UBYTE",
            opcode: 8,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "FLAT_LOAD_SBYTE",
            opcode: 9,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "FLAT_LOAD_USHORT",
            opcode: 10,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "FLAT_LOAD_SSHORT",
            opcode: 11,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "FLAT_LOAD_DWORD",
            opcode: 12,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "FLAT_LOAD_DWORDX2",
            opcode: 13,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "FLAT_LOAD_DWORDX4",
            opcode: 14,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "FLAT_LOAD_DWORDX3",
            opcode: 15,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "FLAT_STORE_BYTE",
            opcode: 24,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "FLAT_STORE_BYTE_D16_HI",
            opcode: 25,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "FLAT_STORE_SHORT",
            opcode: 26,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "FLAT_STORE_SHORT_D16_HI",
            opcode: 27,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "FLAT_STORE_DWORD",
            opcode: 28,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "FLAT_STORE_DWORDX2",
            opcode: 29,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "FLAT_STORE_DWORDX4",
            opcode: 30,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "FLAT_STORE_DWORDX3",
            opcode: 31,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "FLAT_LOAD_UBYTE_D16",
            opcode: 32,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "FLAT_LOAD_UBYTE_D16_HI",
            opcode: 33,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "FLAT_LOAD_SBYTE_D16",
            opcode: 34,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "FLAT_LOAD_SBYTE_D16_HI",
            opcode: 35,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "FLAT_LOAD_SHORT_D16",
            opcode: 36,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "FLAT_LOAD_SHORT_D16_HI",
            opcode: 37,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "FLAT_ATOMIC_SWAP",
            opcode: 48,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "FLAT_ATOMIC_CMPSWAP",
            opcode: 49,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "FLAT_ATOMIC_ADD",
            opcode: 50,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "FLAT_ATOMIC_SUB",
            opcode: 51,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "FLAT_ATOMIC_SMIN",
            opcode: 53,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "FLAT_ATOMIC_UMIN",
            opcode: 54,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "FLAT_ATOMIC_SMAX",
            opcode: 55,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "FLAT_ATOMIC_UMAX",
            opcode: 56,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "FLAT_ATOMIC_AND",
            opcode: 57,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "FLAT_ATOMIC_OR",
            opcode: 58,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "FLAT_ATOMIC_XOR",
            opcode: 59,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "FLAT_ATOMIC_INC",
            opcode: 60,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "FLAT_ATOMIC_DEC",
            opcode: 61,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "FLAT_ATOMIC_FCMPSWAP",
            opcode: 62,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "FLAT_ATOMIC_FMIN",
            opcode: 63,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "FLAT_ATOMIC_FMAX",
            opcode: 64,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "FLAT_ATOMIC_SWAP_X2",
            opcode: 80,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "FLAT_ATOMIC_CMPSWAP_X2",
            opcode: 81,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "FLAT_ATOMIC_ADD_X2",
            opcode: 82,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "FLAT_ATOMIC_SUB_X2",
            opcode: 83,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "FLAT_ATOMIC_SMIN_X2",
            opcode: 85,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "FLAT_ATOMIC_UMIN_X2",
            opcode: 86,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "FLAT_ATOMIC_SMAX_X2",
            opcode: 87,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "FLAT_ATOMIC_UMAX_X2",
            opcode: 88,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "FLAT_ATOMIC_AND_X2",
            opcode: 89,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "FLAT_ATOMIC_OR_X2",
            opcode: 90,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "FLAT_ATOMIC_XOR_X2",
            opcode: 91,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "FLAT_ATOMIC_INC_X2",
            opcode: 92,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "FLAT_ATOMIC_DEC_X2",
            opcode: 93,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "FLAT_ATOMIC_FCMPSWAP_X2",
            opcode: 94,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "FLAT_ATOMIC_FMIN_X2",
            opcode: 95,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "FLAT_ATOMIC_FMAX_X2",
            opcode: 96,
            is_branch: false,
            is_terminator: false,
        },
    ];

    /// Look up an instruction by opcode.
    pub fn lookup(opcode: u16) -> Option<&'static super::InstrEntry> {
        TABLE.iter().find(|e| e.opcode == opcode)
    }
}

/// ENC_FLAT_GLBL opcodes (57 instructions).
pub mod flat_glbl {
    /// Load 8 bits of unsigned data from the global aperture, zero extend to 32 bits and store the result into a vector regi...
    pub const GLOBAL_LOAD_UBYTE: u16 = 8;
    /// Load 8 bits of signed data from the global aperture, sign extend to 32 bits and store the result into a vector register.
    pub const GLOBAL_LOAD_SBYTE: u16 = 9;
    /// Load 16 bits of unsigned data from the global aperture, zero extend to 32 bits and store the result into a vector reg...
    pub const GLOBAL_LOAD_USHORT: u16 = 10;
    /// Load 16 bits of signed data from the global aperture, sign extend to 32 bits and store the result into a vector regis...
    pub const GLOBAL_LOAD_SSHORT: u16 = 11;
    /// Load 32 bits of data from the global aperture into a vector register.
    pub const GLOBAL_LOAD_DWORD: u16 = 12;
    /// Load 64 bits of data from the global aperture into a vector register.
    pub const GLOBAL_LOAD_DWORDX2: u16 = 13;
    /// Load 128 bits of data from the global aperture into a vector register.
    pub const GLOBAL_LOAD_DWORDX4: u16 = 14;
    /// Load 96 bits of data from the global aperture into a vector register.
    pub const GLOBAL_LOAD_DWORDX3: u16 = 15;
    /// Load 32 bits of data from the global aperture into a vector register. The memory base address is provided in a scalar...
    pub const GLOBAL_LOAD_DWORD_ADDTID: u16 = 22;
    /// Store 32 bits of data from a vector input register into the global aperture. The memory base address is provided as a...
    pub const GLOBAL_STORE_DWORD_ADDTID: u16 = 23;
    /// Store 8 bits of data from a vector register into the global aperture.
    pub const GLOBAL_STORE_BYTE: u16 = 24;
    /// Store 8 bits of data from the high 16 bits of a 32-bit vector register into the global aperture.
    pub const GLOBAL_STORE_BYTE_D16_HI: u16 = 25;
    /// Store 16 bits of data from a vector register into the global aperture.
    pub const GLOBAL_STORE_SHORT: u16 = 26;
    /// Store 16 bits of data from the high 16 bits of a 32-bit vector register into the global aperture.
    pub const GLOBAL_STORE_SHORT_D16_HI: u16 = 27;
    /// Store 32 bits of data from vector input registers into the global aperture.
    pub const GLOBAL_STORE_DWORD: u16 = 28;
    /// Store 64 bits of data from vector input registers into the global aperture.
    pub const GLOBAL_STORE_DWORDX2: u16 = 29;
    /// Store 128 bits of data from vector input registers into the global aperture.
    pub const GLOBAL_STORE_DWORDX4: u16 = 30;
    /// Store 96 bits of data from vector input registers into the global aperture.
    pub const GLOBAL_STORE_DWORDX3: u16 = 31;
    /// Load 8 bits of unsigned data from the global aperture, zero extend to 16 bits and store the result into the low 16 bi...
    pub const GLOBAL_LOAD_UBYTE_D16: u16 = 32;
    /// Load 8 bits of unsigned data from the global aperture, zero extend to 16 bits and store the result into the high 16 b...
    pub const GLOBAL_LOAD_UBYTE_D16_HI: u16 = 33;
    /// Load 8 bits of signed data from the global aperture, sign extend to 16 bits and store the result into the low 16 bits...
    pub const GLOBAL_LOAD_SBYTE_D16: u16 = 34;
    /// Load 8 bits of signed data from the global aperture, sign extend to 16 bits and store the result into the high 16 bit...
    pub const GLOBAL_LOAD_SBYTE_D16_HI: u16 = 35;
    /// Load 16 bits of unsigned data from the global aperture and store the result into the low 16 bits of a 32-bit vector r...
    pub const GLOBAL_LOAD_SHORT_D16: u16 = 36;
    /// Load 16 bits of unsigned data from the global aperture and store the result into the high 16 bits of a 32-bit vector ...
    pub const GLOBAL_LOAD_SHORT_D16_HI: u16 = 37;
    /// Swap an unsigned 32-bit integer value in the data register with a location in the global aperture. Store the original...
    pub const GLOBAL_ATOMIC_SWAP: u16 = 48;
    /// Compare two unsigned 32-bit integer values stored in the data comparison register and a location in the global apertu...
    pub const GLOBAL_ATOMIC_CMPSWAP: u16 = 49;
    /// Add two unsigned 32-bit integer values stored in the data register and a location in the global aperture. Store the o...
    pub const GLOBAL_ATOMIC_ADD: u16 = 50;
    /// Subtract an unsigned 32-bit integer value stored in the data register from a value stored in a location in the global...
    pub const GLOBAL_ATOMIC_SUB: u16 = 51;
    /// Subtract an unsigned 32-bit integer location in the global aperture from a value in the data register and clamp the r...
    pub const GLOBAL_ATOMIC_CSUB: u16 = 52;
    /// Select the minimum of two signed 32-bit integer inputs, given two values stored in the data register and a location i...
    pub const GLOBAL_ATOMIC_SMIN: u16 = 53;
    /// Select the minimum of two unsigned 32-bit integer inputs, given two values stored in the data register and a location...
    pub const GLOBAL_ATOMIC_UMIN: u16 = 54;
    /// Select the maximum of two signed 32-bit integer inputs, given two values stored in the data register and a location i...
    pub const GLOBAL_ATOMIC_SMAX: u16 = 55;
    /// Select the maximum of two unsigned 32-bit integer inputs, given two values stored in the data register and a location...
    pub const GLOBAL_ATOMIC_UMAX: u16 = 56;
    /// Calculate bitwise AND given two unsigned 32-bit integer values stored in the data register and a location in the glob...
    pub const GLOBAL_ATOMIC_AND: u16 = 57;
    /// Calculate bitwise OR given two unsigned 32-bit integer values stored in the data register and a location in the globa...
    pub const GLOBAL_ATOMIC_OR: u16 = 58;
    /// Calculate bitwise XOR given two unsigned 32-bit integer values stored in the data register and a location in the glob...
    pub const GLOBAL_ATOMIC_XOR: u16 = 59;
    /// Increment an unsigned 32-bit integer value from a location in the global aperture with wraparound to 0 if the value e...
    pub const GLOBAL_ATOMIC_INC: u16 = 60;
    /// Decrement an unsigned 32-bit integer value from a location in the global aperture with wraparound to a value in the d...
    pub const GLOBAL_ATOMIC_DEC: u16 = 61;
    /// Compare two single-precision float values stored in the data comparison register and a location in the global apertur...
    pub const GLOBAL_ATOMIC_FCMPSWAP: u16 = 62;
    /// Select the minimum of two single-precision float inputs, given two values stored in the data register and a location ...
    pub const GLOBAL_ATOMIC_FMIN: u16 = 63;
    /// Select the maximum of two single-precision float inputs, given two values stored in the data register and a location ...
    pub const GLOBAL_ATOMIC_FMAX: u16 = 64;
    /// Swap an unsigned 64-bit integer value in the data register with a location in the global aperture. Store the original...
    pub const GLOBAL_ATOMIC_SWAP_X2: u16 = 80;
    /// Compare two unsigned 64-bit integer values stored in the data comparison register and a location in the global apertu...
    pub const GLOBAL_ATOMIC_CMPSWAP_X2: u16 = 81;
    /// Add two unsigned 64-bit integer values stored in the data register and a location in the global aperture. Store the o...
    pub const GLOBAL_ATOMIC_ADD_X2: u16 = 82;
    /// Subtract an unsigned 64-bit integer value stored in the data register from a value stored in a location in the global...
    pub const GLOBAL_ATOMIC_SUB_X2: u16 = 83;
    /// Select the minimum of two signed 64-bit integer inputs, given two values stored in the data register and a location i...
    pub const GLOBAL_ATOMIC_SMIN_X2: u16 = 85;
    /// Select the minimum of two unsigned 64-bit integer inputs, given two values stored in the data register and a location...
    pub const GLOBAL_ATOMIC_UMIN_X2: u16 = 86;
    /// Select the maximum of two signed 64-bit integer inputs, given two values stored in the data register and a location i...
    pub const GLOBAL_ATOMIC_SMAX_X2: u16 = 87;
    /// Select the maximum of two unsigned 64-bit integer inputs, given two values stored in the data register and a location...
    pub const GLOBAL_ATOMIC_UMAX_X2: u16 = 88;
    /// Calculate bitwise AND given two unsigned 64-bit integer values stored in the data register and a location in the glob...
    pub const GLOBAL_ATOMIC_AND_X2: u16 = 89;
    /// Calculate bitwise OR given two unsigned 64-bit integer values stored in the data register and a location in the globa...
    pub const GLOBAL_ATOMIC_OR_X2: u16 = 90;
    /// Calculate bitwise XOR given two unsigned 64-bit integer values stored in the data register and a location in the glob...
    pub const GLOBAL_ATOMIC_XOR_X2: u16 = 91;
    /// Increment an unsigned 64-bit integer value from a location in the global aperture with wraparound to 0 if the value e...
    pub const GLOBAL_ATOMIC_INC_X2: u16 = 92;
    /// Decrement an unsigned 64-bit integer value from a location in the global aperture with wraparound to a value in the d...
    pub const GLOBAL_ATOMIC_DEC_X2: u16 = 93;
    /// Compare two double-precision float values stored in the data comparison register and a location in the global apertur...
    pub const GLOBAL_ATOMIC_FCMPSWAP_X2: u16 = 94;
    /// Select the minimum of two double-precision float inputs, given two values stored in the data register and a location ...
    pub const GLOBAL_ATOMIC_FMIN_X2: u16 = 95;
    /// Select the maximum of two double-precision float inputs, given two values stored in the data register and a location ...
    pub const GLOBAL_ATOMIC_FMAX_X2: u16 = 96;

    /// All ENC_FLAT_GLBL instructions.
    pub const TABLE: &[super::InstrEntry] = &[
        super::InstrEntry {
            name: "GLOBAL_LOAD_UBYTE",
            opcode: 8,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "GLOBAL_LOAD_SBYTE",
            opcode: 9,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "GLOBAL_LOAD_USHORT",
            opcode: 10,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "GLOBAL_LOAD_SSHORT",
            opcode: 11,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "GLOBAL_LOAD_DWORD",
            opcode: 12,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "GLOBAL_LOAD_DWORDX2",
            opcode: 13,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "GLOBAL_LOAD_DWORDX4",
            opcode: 14,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "GLOBAL_LOAD_DWORDX3",
            opcode: 15,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "GLOBAL_LOAD_DWORD_ADDTID",
            opcode: 22,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "GLOBAL_STORE_DWORD_ADDTID",
            opcode: 23,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "GLOBAL_STORE_BYTE",
            opcode: 24,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "GLOBAL_STORE_BYTE_D16_HI",
            opcode: 25,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "GLOBAL_STORE_SHORT",
            opcode: 26,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "GLOBAL_STORE_SHORT_D16_HI",
            opcode: 27,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "GLOBAL_STORE_DWORD",
            opcode: 28,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "GLOBAL_STORE_DWORDX2",
            opcode: 29,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "GLOBAL_STORE_DWORDX4",
            opcode: 30,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "GLOBAL_STORE_DWORDX3",
            opcode: 31,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "GLOBAL_LOAD_UBYTE_D16",
            opcode: 32,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "GLOBAL_LOAD_UBYTE_D16_HI",
            opcode: 33,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "GLOBAL_LOAD_SBYTE_D16",
            opcode: 34,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "GLOBAL_LOAD_SBYTE_D16_HI",
            opcode: 35,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "GLOBAL_LOAD_SHORT_D16",
            opcode: 36,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "GLOBAL_LOAD_SHORT_D16_HI",
            opcode: 37,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "GLOBAL_ATOMIC_SWAP",
            opcode: 48,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "GLOBAL_ATOMIC_CMPSWAP",
            opcode: 49,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "GLOBAL_ATOMIC_ADD",
            opcode: 50,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "GLOBAL_ATOMIC_SUB",
            opcode: 51,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "GLOBAL_ATOMIC_CSUB",
            opcode: 52,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "GLOBAL_ATOMIC_SMIN",
            opcode: 53,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "GLOBAL_ATOMIC_UMIN",
            opcode: 54,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "GLOBAL_ATOMIC_SMAX",
            opcode: 55,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "GLOBAL_ATOMIC_UMAX",
            opcode: 56,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "GLOBAL_ATOMIC_AND",
            opcode: 57,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "GLOBAL_ATOMIC_OR",
            opcode: 58,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "GLOBAL_ATOMIC_XOR",
            opcode: 59,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "GLOBAL_ATOMIC_INC",
            opcode: 60,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "GLOBAL_ATOMIC_DEC",
            opcode: 61,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "GLOBAL_ATOMIC_FCMPSWAP",
            opcode: 62,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "GLOBAL_ATOMIC_FMIN",
            opcode: 63,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "GLOBAL_ATOMIC_FMAX",
            opcode: 64,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "GLOBAL_ATOMIC_SWAP_X2",
            opcode: 80,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "GLOBAL_ATOMIC_CMPSWAP_X2",
            opcode: 81,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "GLOBAL_ATOMIC_ADD_X2",
            opcode: 82,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "GLOBAL_ATOMIC_SUB_X2",
            opcode: 83,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "GLOBAL_ATOMIC_SMIN_X2",
            opcode: 85,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "GLOBAL_ATOMIC_UMIN_X2",
            opcode: 86,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "GLOBAL_ATOMIC_SMAX_X2",
            opcode: 87,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "GLOBAL_ATOMIC_UMAX_X2",
            opcode: 88,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "GLOBAL_ATOMIC_AND_X2",
            opcode: 89,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "GLOBAL_ATOMIC_OR_X2",
            opcode: 90,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "GLOBAL_ATOMIC_XOR_X2",
            opcode: 91,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "GLOBAL_ATOMIC_INC_X2",
            opcode: 92,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "GLOBAL_ATOMIC_DEC_X2",
            opcode: 93,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "GLOBAL_ATOMIC_FCMPSWAP_X2",
            opcode: 94,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "GLOBAL_ATOMIC_FMIN_X2",
            opcode: 95,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "GLOBAL_ATOMIC_FMAX_X2",
            opcode: 96,
            is_branch: false,
            is_terminator: false,
        },
    ];

    /// Look up an instruction by opcode.
    pub fn lookup(opcode: u16) -> Option<&'static super::InstrEntry> {
        TABLE.iter().find(|e| e.opcode == opcode)
    }
}

/// ENC_FLAT_SCRATCH opcodes (22 instructions).
pub mod flat_scratch {
    /// Load 8 bits of unsigned data from the scratch aperture, zero extend to 32 bits and store the result into a vector reg...
    pub const SCRATCH_LOAD_UBYTE: u16 = 8;
    /// Load 8 bits of signed data from the scratch aperture, sign extend to 32 bits and store the result into a vector regis...
    pub const SCRATCH_LOAD_SBYTE: u16 = 9;
    /// Load 16 bits of unsigned data from the scratch aperture, zero extend to 32 bits and store the result into a vector re...
    pub const SCRATCH_LOAD_USHORT: u16 = 10;
    /// Load 16 bits of signed data from the scratch aperture, sign extend to 32 bits and store the result into a vector regi...
    pub const SCRATCH_LOAD_SSHORT: u16 = 11;
    /// Load 32 bits of data from the scratch aperture into a vector register.
    pub const SCRATCH_LOAD_DWORD: u16 = 12;
    /// Load 64 bits of data from the scratch aperture into a vector register.
    pub const SCRATCH_LOAD_DWORDX2: u16 = 13;
    /// Load 128 bits of data from the scratch aperture into a vector register.
    pub const SCRATCH_LOAD_DWORDX4: u16 = 14;
    /// Load 96 bits of data from the scratch aperture into a vector register.
    pub const SCRATCH_LOAD_DWORDX3: u16 = 15;
    /// Store 8 bits of data from a vector register into the scratch aperture.
    pub const SCRATCH_STORE_BYTE: u16 = 24;
    /// Store 8 bits of data from the high 16 bits of a 32-bit vector register into the scratch aperture.
    pub const SCRATCH_STORE_BYTE_D16_HI: u16 = 25;
    /// Store 16 bits of data from a vector register into the scratch aperture.
    pub const SCRATCH_STORE_SHORT: u16 = 26;
    /// Store 16 bits of data from the high 16 bits of a 32-bit vector register into the scratch aperture.
    pub const SCRATCH_STORE_SHORT_D16_HI: u16 = 27;
    /// Store 32 bits of data from vector input registers into the scratch aperture.
    pub const SCRATCH_STORE_DWORD: u16 = 28;
    /// Store 64 bits of data from vector input registers into the scratch aperture.
    pub const SCRATCH_STORE_DWORDX2: u16 = 29;
    /// Store 128 bits of data from vector input registers into the scratch aperture.
    pub const SCRATCH_STORE_DWORDX4: u16 = 30;
    /// Store 96 bits of data from vector input registers into the scratch aperture.
    pub const SCRATCH_STORE_DWORDX3: u16 = 31;
    /// Load 8 bits of unsigned data from the scratch aperture, zero extend to 16 bits and store the result into the low 16 b...
    pub const SCRATCH_LOAD_UBYTE_D16: u16 = 32;
    /// Load 8 bits of unsigned data from the scratch aperture, zero extend to 16 bits and store the result into the high 16 ...
    pub const SCRATCH_LOAD_UBYTE_D16_HI: u16 = 33;
    /// Load 8 bits of signed data from the scratch aperture, sign extend to 16 bits and store the result into the low 16 bit...
    pub const SCRATCH_LOAD_SBYTE_D16: u16 = 34;
    /// Load 8 bits of signed data from the scratch aperture, sign extend to 16 bits and store the result into the high 16 bi...
    pub const SCRATCH_LOAD_SBYTE_D16_HI: u16 = 35;
    /// Load 16 bits of unsigned data from the scratch aperture and store the result into the low 16 bits of a 32-bit vector ...
    pub const SCRATCH_LOAD_SHORT_D16: u16 = 36;
    /// Load 16 bits of unsigned data from the scratch aperture and store the result into the high 16 bits of a 32-bit vector...
    pub const SCRATCH_LOAD_SHORT_D16_HI: u16 = 37;

    /// All ENC_FLAT_SCRATCH instructions.
    pub const TABLE: &[super::InstrEntry] = &[
        super::InstrEntry {
            name: "SCRATCH_LOAD_UBYTE",
            opcode: 8,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "SCRATCH_LOAD_SBYTE",
            opcode: 9,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "SCRATCH_LOAD_USHORT",
            opcode: 10,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "SCRATCH_LOAD_SSHORT",
            opcode: 11,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "SCRATCH_LOAD_DWORD",
            opcode: 12,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "SCRATCH_LOAD_DWORDX2",
            opcode: 13,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "SCRATCH_LOAD_DWORDX4",
            opcode: 14,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "SCRATCH_LOAD_DWORDX3",
            opcode: 15,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "SCRATCH_STORE_BYTE",
            opcode: 24,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "SCRATCH_STORE_BYTE_D16_HI",
            opcode: 25,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "SCRATCH_STORE_SHORT",
            opcode: 26,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "SCRATCH_STORE_SHORT_D16_HI",
            opcode: 27,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "SCRATCH_STORE_DWORD",
            opcode: 28,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "SCRATCH_STORE_DWORDX2",
            opcode: 29,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "SCRATCH_STORE_DWORDX4",
            opcode: 30,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "SCRATCH_STORE_DWORDX3",
            opcode: 31,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "SCRATCH_LOAD_UBYTE_D16",
            opcode: 32,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "SCRATCH_LOAD_UBYTE_D16_HI",
            opcode: 33,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "SCRATCH_LOAD_SBYTE_D16",
            opcode: 34,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "SCRATCH_LOAD_SBYTE_D16_HI",
            opcode: 35,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "SCRATCH_LOAD_SHORT_D16",
            opcode: 36,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "SCRATCH_LOAD_SHORT_D16_HI",
            opcode: 37,
            is_branch: false,
            is_terminator: false,
        },
    ];

    /// Look up an instruction by opcode.
    pub fn lookup(opcode: u16) -> Option<&'static super::InstrEntry> {
        TABLE.iter().find(|e| e.opcode == opcode)
    }
}

/// ENC_MIMG opcodes (130 instructions).
pub mod mimg {
    /// Load a texel from the largest miplevel in an image surface and store the result into a vector register. Perform the f...
    pub const IMAGE_LOAD: u16 = 0;
    /// Load a texel from a user-specified miplevel in an image surface and store the result into a vector register. Perform ...
    pub const IMAGE_LOAD_MIP: u16 = 1;
    /// Load a texel from the largest miplevel in an image surface and store the result into a vector register. 8- and 16-bit...
    pub const IMAGE_LOAD_PCK: u16 = 2;
    /// Load a texel from the largest miplevel in an image surface and store the result into a vector register. 8- and 16-bit...
    pub const IMAGE_LOAD_PCK_SGN: u16 = 3;
    /// Load a texel from a user-specified miplevel in an image surface and store the result into a vector register. 8- and 1...
    pub const IMAGE_LOAD_MIP_PCK: u16 = 4;
    /// Load a texel from a user-specified miplevel in an image surface and store the result into a vector register. 8- and 1...
    pub const IMAGE_LOAD_MIP_PCK_SGN: u16 = 5;
    /// Store a texel from a vector register to the largest miplevel in an image surface. The texel data is converted using t...
    pub const IMAGE_STORE: u16 = 8;
    /// Store a texel from a vector register to a user-specified miplevel in an image surface. The texel data is converted us...
    pub const IMAGE_STORE_MIP: u16 = 9;
    /// Store a texel from a vector register to the largest miplevel in an image surface. The texel data is already packed an...
    pub const IMAGE_STORE_PCK: u16 = 10;
    /// Store a texel from a vector register to a user-specified miplevel in an image surface. The texel data is already pack...
    pub const IMAGE_STORE_MIP_PCK: u16 = 11;
    /// Gather resource information for a given miplevel provided in the address register. Returns 4 integer values into regi...
    pub const IMAGE_GET_RESINFO: u16 = 14;
    /// Swap an unsigned 32-bit integer value in the data register with a location in an image surface. Store the original va...
    pub const IMAGE_ATOMIC_SWAP: u16 = 15;
    /// Compare two unsigned 32-bit integer values stored in the data comparison register and a location in an image surface....
    pub const IMAGE_ATOMIC_CMPSWAP: u16 = 16;
    /// Add two unsigned 32-bit integer values stored in the data register and a location in an image surface. Store the orig...
    pub const IMAGE_ATOMIC_ADD: u16 = 17;
    /// Subtract an unsigned 32-bit integer value stored in the data register from a value stored in a location in an image s...
    pub const IMAGE_ATOMIC_SUB: u16 = 18;
    /// Select the minimum of two signed 32-bit integer inputs, given two values stored in the data register and a location i...
    pub const IMAGE_ATOMIC_SMIN: u16 = 20;
    /// Select the minimum of two unsigned 32-bit integer inputs, given two values stored in the data register and a location...
    pub const IMAGE_ATOMIC_UMIN: u16 = 21;
    /// Select the maximum of two signed 32-bit integer inputs, given two values stored in the data register and a location i...
    pub const IMAGE_ATOMIC_SMAX: u16 = 22;
    /// Select the maximum of two unsigned 32-bit integer inputs, given two values stored in the data register and a location...
    pub const IMAGE_ATOMIC_UMAX: u16 = 23;
    /// Calculate bitwise AND given two unsigned 32-bit integer values stored in the data register and a location in an image...
    pub const IMAGE_ATOMIC_AND: u16 = 24;
    /// Calculate bitwise OR given two unsigned 32-bit integer values stored in the data register and a location in an image ...
    pub const IMAGE_ATOMIC_OR: u16 = 25;
    /// Calculate bitwise XOR given two unsigned 32-bit integer values stored in the data register and a location in an image...
    pub const IMAGE_ATOMIC_XOR: u16 = 26;
    /// Increment an unsigned 32-bit integer value from a location in an image surface with wraparound to 0 if the value exce...
    pub const IMAGE_ATOMIC_INC: u16 = 27;
    /// Decrement an unsigned 32-bit integer value from a location in an image surface with wraparound to a value in the data...
    pub const IMAGE_ATOMIC_DEC: u16 = 28;
    /// Compare two single-precision float values stored in the data comparison register and a location in an image surface. ...
    pub const IMAGE_ATOMIC_FCMPSWAP: u16 = 29;
    /// Select the minimum of two single-precision float inputs, given two values stored in the data register and a location ...
    pub const IMAGE_ATOMIC_FMIN: u16 = 30;
    /// Select the maximum of two single-precision float inputs, given two values stored in the data register and a location ...
    pub const IMAGE_ATOMIC_FMAX: u16 = 31;
    /// Sample texels from an image surface using texel coordinates provided by the address input registers and store the res...
    pub const IMAGE_SAMPLE: u16 = 32;
    /// Sample texels from an image surface using texel coordinates provided by the address input registers and store the res...
    pub const IMAGE_SAMPLE_CL: u16 = 33;
    /// Sample texels from an image surface using texel coordinates provided by the address input registers and store the res...
    pub const IMAGE_SAMPLE_D: u16 = 34;
    /// Sample texels from an image surface using texel coordinates provided by the address input registers and store the res...
    pub const IMAGE_SAMPLE_D_CL: u16 = 35;
    /// Sample texels from an image surface using texel coordinates provided by the address input registers and store the res...
    pub const IMAGE_SAMPLE_L: u16 = 36;
    /// Sample texels from an image surface using texel coordinates provided by the address input registers and store the res...
    pub const IMAGE_SAMPLE_B: u16 = 37;
    /// Sample texels from an image surface using texel coordinates provided by the address input registers and store the res...
    pub const IMAGE_SAMPLE_B_CL: u16 = 38;
    /// Sample texels from an image surface using texel coordinates provided by the address input registers and store the res...
    pub const IMAGE_SAMPLE_LZ: u16 = 39;
    /// Sample texels from an image surface using texel coordinates provided by the address input registers and store the res...
    pub const IMAGE_SAMPLE_C: u16 = 40;
    /// Sample texels from an image surface using texel coordinates provided by the address input registers and store the res...
    pub const IMAGE_SAMPLE_C_CL: u16 = 41;
    /// Sample texels from an image surface using texel coordinates provided by the address input registers and store the res...
    pub const IMAGE_SAMPLE_C_D: u16 = 42;
    /// Sample texels from an image surface using texel coordinates provided by the address input registers and store the res...
    pub const IMAGE_SAMPLE_C_D_CL: u16 = 43;
    /// Sample texels from an image surface using texel coordinates provided by the address input registers and store the res...
    pub const IMAGE_SAMPLE_C_L: u16 = 44;
    /// Sample texels from an image surface using texel coordinates provided by the address input registers and store the res...
    pub const IMAGE_SAMPLE_C_B: u16 = 45;
    /// Sample texels from an image surface using texel coordinates provided by the address input registers and store the res...
    pub const IMAGE_SAMPLE_C_B_CL: u16 = 46;
    /// Sample texels from an image surface using texel coordinates provided by the address input registers and store the res...
    pub const IMAGE_SAMPLE_C_LZ: u16 = 47;
    /// Sample texels from an image surface using texel coordinates provided by the address input registers and store the res...
    pub const IMAGE_SAMPLE_O: u16 = 48;
    /// Sample texels from an image surface using texel coordinates provided by the address input registers and store the res...
    pub const IMAGE_SAMPLE_CL_O: u16 = 49;
    /// Sample texels from an image surface using texel coordinates provided by the address input registers and store the res...
    pub const IMAGE_SAMPLE_D_O: u16 = 50;
    /// Sample texels from an image surface using texel coordinates provided by the address input registers and store the res...
    pub const IMAGE_SAMPLE_D_CL_O: u16 = 51;
    /// Sample texels from an image surface using texel coordinates provided by the address input registers and store the res...
    pub const IMAGE_SAMPLE_L_O: u16 = 52;
    /// Sample texels from an image surface using texel coordinates provided by the address input registers and store the res...
    pub const IMAGE_SAMPLE_B_O: u16 = 53;
    /// Sample texels from an image surface using texel coordinates provided by the address input registers and store the res...
    pub const IMAGE_SAMPLE_B_CL_O: u16 = 54;
    /// Sample texels from an image surface using texel coordinates provided by the address input registers and store the res...
    pub const IMAGE_SAMPLE_LZ_O: u16 = 55;
    /// Sample texels from an image surface using texel coordinates provided by the address input registers and store the res...
    pub const IMAGE_SAMPLE_C_O: u16 = 56;
    /// Sample texels from an image surface using texel coordinates provided by the address input registers and store the res...
    pub const IMAGE_SAMPLE_C_CL_O: u16 = 57;
    /// Sample texels from an image surface using texel coordinates provided by the address input registers and store the res...
    pub const IMAGE_SAMPLE_C_D_O: u16 = 58;
    /// Sample texels from an image surface using texel coordinates provided by the address input registers and store the res...
    pub const IMAGE_SAMPLE_C_D_CL_O: u16 = 59;
    /// Sample texels from an image surface using texel coordinates provided by the address input registers and store the res...
    pub const IMAGE_SAMPLE_C_L_O: u16 = 60;
    /// Sample texels from an image surface using texel coordinates provided by the address input registers and store the res...
    pub const IMAGE_SAMPLE_C_B_O: u16 = 61;
    /// Sample texels from an image surface using texel coordinates provided by the address input registers and store the res...
    pub const IMAGE_SAMPLE_C_B_CL_O: u16 = 62;
    /// Sample texels from an image surface using texel coordinates provided by the address input registers and store the res...
    pub const IMAGE_SAMPLE_C_LZ_O: u16 = 63;
    /// Gather 4 single-component texels from a 2x2 matrix on an image surface. Store the result into vector registers. The D...
    pub const IMAGE_GATHER4: u16 = 64;
    /// Gather 4 single-component texels from a 2x2 matrix on an image surface. Store the result into vector registers. The D...
    pub const IMAGE_GATHER4_CL: u16 = 65;
    /// Load 2 horizontal elements from the largest miplevel in an image surface and store the result into a vector register....
    pub const IMAGE_LOAD_BY2: u16 = 66;
    /// Load 4 horizontal elements from the largest miplevel in an image surface and store the result into a vector register....
    pub const IMAGE_LOAD_BY4: u16 = 67;
    /// Gather 4 single-component texels from a 2x2 matrix on an image surface. Store the result into vector registers. The D...
    pub const IMAGE_GATHER4_L: u16 = 68;
    /// Gather 4 single-component texels from a 2x2 matrix on an image surface. Store the result into vector registers. The D...
    pub const IMAGE_GATHER4_B: u16 = 69;
    /// Gather 4 single-component texels from a 2x2 matrix on an image surface. Store the result into vector registers. The D...
    pub const IMAGE_GATHER4_B_CL: u16 = 70;
    /// Gather 4 single-component texels from a 2x2 matrix on an image surface. Store the result into vector registers. The D...
    pub const IMAGE_GATHER4_LZ: u16 = 71;
    /// Gather 4 single-component texels from a 2x2 matrix on an image surface. Store the result into vector registers. The D...
    pub const IMAGE_GATHER4_C: u16 = 72;
    /// Gather 4 single-component texels from a 2x2 matrix on an image surface. Store the result into vector registers. The D...
    pub const IMAGE_GATHER4_C_CL: u16 = 73;
    /// Load 2 horizontal elements from a user-specified miplevel in an image surface and store the result into a vector regi...
    pub const IMAGE_LOAD_MIP_BY2: u16 = 74;
    /// Load 4 horizontal elements from a user-specified miplevel in an image surface and store the result into a vector regi...
    pub const IMAGE_LOAD_MIP_BY4: u16 = 75;
    /// Gather 4 single-component texels from a 2x2 matrix on an image surface. Store the result into vector registers. The D...
    pub const IMAGE_GATHER4_C_L: u16 = 76;
    /// Gather 4 single-component texels from a 2x2 matrix on an image surface. Store the result into vector registers. The D...
    pub const IMAGE_GATHER4_C_B: u16 = 77;
    /// Gather 4 single-component texels from a 2x2 matrix on an image surface. Store the result into vector registers. The D...
    pub const IMAGE_GATHER4_C_B_CL: u16 = 78;
    /// Gather 4 single-component texels from a 2x2 matrix on an image surface. Store the result into vector registers. The D...
    pub const IMAGE_GATHER4_C_LZ: u16 = 79;
    /// Gather 4 single-component texels from a 2x2 matrix on an image surface. Store the result into vector registers. The D...
    pub const IMAGE_GATHER4_O: u16 = 80;
    /// Gather 4 single-component texels from a 2x2 matrix on an image surface. Store the result into vector registers. The D...
    pub const IMAGE_GATHER4_CL_O: u16 = 81;
    /// Store 2 horizontal elements from a vector register to the largest miplevel in an image surface. The texel data is con...
    pub const IMAGE_STORE_BY2: u16 = 82;
    /// Store 4 horizontal elements from a vector register to the largest miplevel in an image surface. The texel data is con...
    pub const IMAGE_STORE_BY4: u16 = 83;
    /// Gather 4 single-component texels from a 2x2 matrix on an image surface. Store the result into vector registers. The D...
    pub const IMAGE_GATHER4_L_O: u16 = 84;
    /// Gather 4 single-component texels from a 2x2 matrix on an image surface. Store the result into vector registers. The D...
    pub const IMAGE_GATHER4_B_O: u16 = 85;
    /// Gather 4 single-component texels from a 2x2 matrix on an image surface. Store the result into vector registers. The D...
    pub const IMAGE_GATHER4_B_CL_O: u16 = 86;
    /// Gather 4 single-component texels from a 2x2 matrix on an image surface. Store the result into vector registers. The D...
    pub const IMAGE_GATHER4_LZ_O: u16 = 87;
    /// Gather 4 single-component texels from a 2x2 matrix on an image surface. Store the result into vector registers. The D...
    pub const IMAGE_GATHER4_C_O: u16 = 88;
    /// Gather 4 single-component texels from a 2x2 matrix on an image surface. Store the result into vector registers. The D...
    pub const IMAGE_GATHER4_C_CL_O: u16 = 89;
    /// Store 2 horizontal elements from a vector register to a user-specified miplevel in an image surface. The texel data i...
    pub const IMAGE_STORE_MIP_BY2: u16 = 90;
    /// Store 4 horizontal elements from a vector register to a user-specified miplevel in an image surface. The texel data i...
    pub const IMAGE_STORE_MIP_BY4: u16 = 91;
    /// Gather 4 single-component texels from a 2x2 matrix on an image surface. Store the result into vector registers. The D...
    pub const IMAGE_GATHER4_C_L_O: u16 = 92;
    /// Gather 4 single-component texels from a 2x2 matrix on an image surface. Store the result into vector registers. The D...
    pub const IMAGE_GATHER4_C_B_O: u16 = 93;
    /// Gather 4 single-component texels from a 2x2 matrix on an image surface. Store the result into vector registers. The D...
    pub const IMAGE_GATHER4_C_B_CL_O: u16 = 94;
    /// Gather 4 single-component texels from a 2x2 matrix on an image surface. Store the result into vector registers. The D...
    pub const IMAGE_GATHER4_C_LZ_O: u16 = 95;
    /// Return the calculated level of detail (LOD) for the provided input as two single-precision float values. No memory ac...
    pub const IMAGE_GET_LOD: u16 = 96;
    /// Gather 4 single-component texels from a 4x1 row vector on an image surface. Store the result into vector registers. T...
    pub const IMAGE_GATHER4H: u16 = 97;
    /// Gather all components of 4 texels from a 4x1 row vector on an image surface. Store the result into vector registers. ...
    pub const IMAGE_GATHER4H_PCK: u16 = 98;
    /// Gather all components of 8 texels from a 8x1 row vector on an image surface. Store the result into vector registers. ...
    pub const IMAGE_GATHER8H_PCK: u16 = 99;
    /// Sample texels from an image surface using texel coordinates provided by the address input registers and store the res...
    pub const IMAGE_SAMPLE_CD: u16 = 104;
    /// Sample texels from an image surface using texel coordinates provided by the address input registers and store the res...
    pub const IMAGE_SAMPLE_CD_CL: u16 = 105;
    /// Sample texels from an image surface using texel coordinates provided by the address input registers and store the res...
    pub const IMAGE_SAMPLE_C_CD: u16 = 106;
    /// Sample texels from an image surface using texel coordinates provided by the address input registers and store the res...
    pub const IMAGE_SAMPLE_C_CD_CL: u16 = 107;
    /// Sample texels from an image surface using texel coordinates provided by the address input registers and store the res...
    pub const IMAGE_SAMPLE_CD_O: u16 = 108;
    /// Sample texels from an image surface using texel coordinates provided by the address input registers and store the res...
    pub const IMAGE_SAMPLE_CD_CL_O: u16 = 109;
    /// Sample texels from an image surface using texel coordinates provided by the address input registers and store the res...
    pub const IMAGE_SAMPLE_C_CD_O: u16 = 110;
    /// Sample texels from an image surface using texel coordinates provided by the address input registers and store the res...
    pub const IMAGE_SAMPLE_C_CD_CL_O: u16 = 111;
    /// Load 2 horizontal elements from the largest miplevel in an image surface and store the result into a vector register....
    pub const IMAGE_LOAD_PCK2: u16 = 112;
    /// Load 4 horizontal elements from the largest miplevel in an image surface and store the result into a vector register....
    pub const IMAGE_LOAD_PCK4: u16 = 113;
    /// Load 2 horizontal elements from a user-specified miplevel in an image surface and store the result into a vector regi...
    pub const IMAGE_LOAD_MIP_PCK2: u16 = 115;
    /// Load 4 horizontal elements from a user-specified miplevel in an image surface and store the result into a vector regi...
    pub const IMAGE_LOAD_MIP_PCK4: u16 = 116;
    /// Store 2 horizontal elements from a vector register to the largest miplevel in an image surface. The texel data is alr...
    pub const IMAGE_STORE_PCK2: u16 = 118;
    /// Store 4 horizontal elements from a vector register to the largest miplevel in an image surface. The texel data is alr...
    pub const IMAGE_STORE_PCK4: u16 = 119;
    /// Store 2 horizontal elements from a vector register to a user-specified miplevel in an image surface. The texel data i...
    pub const IMAGE_STORE_MIP_PCK2: u16 = 121;
    /// Store 4 horizontal elements from a vector register to a user-specified miplevel in an image surface. The texel data i...
    pub const IMAGE_STORE_MIP_PCK4: u16 = 122;
    /// Load up to 4 samples of 1 component from an MSAA resource with a user-specified fragment ID. No sampling is performed.
    pub const IMAGE_MSAA_LOAD: u16 = 128;
    /// Sample texels from an image surface using texel coordinates provided by the address input registers and store the res...
    pub const IMAGE_SAMPLE_D_G16: u16 = 162;
    /// Sample texels from an image surface using texel coordinates provided by the address input registers and store the res...
    pub const IMAGE_SAMPLE_D_CL_G16: u16 = 163;
    /// Sample texels from an image surface using texel coordinates provided by the address input registers and store the res...
    pub const IMAGE_SAMPLE_C_D_G16: u16 = 170;
    /// Sample texels from an image surface using texel coordinates provided by the address input registers and store the res...
    pub const IMAGE_SAMPLE_C_D_CL_G16: u16 = 171;
    /// Sample texels from an image surface using texel coordinates provided by the address input registers and store the res...
    pub const IMAGE_SAMPLE_D_O_G16: u16 = 178;
    /// Sample texels from an image surface using texel coordinates provided by the address input registers and store the res...
    pub const IMAGE_SAMPLE_D_CL_O_G16: u16 = 179;
    /// Sample texels from an image surface using texel coordinates provided by the address input registers and store the res...
    pub const IMAGE_SAMPLE_C_D_O_G16: u16 = 186;
    /// Sample texels from an image surface using texel coordinates provided by the address input registers and store the res...
    pub const IMAGE_SAMPLE_C_D_CL_O_G16: u16 = 187;
    /// Test the intersection of rays with either box nodes or triangle nodes within a bounded volume hierarchy using 32 bit ...
    pub const IMAGE_BVH_INTERSECT_RAY: u16 = 230;
    /// Test the intersection of rays with either box nodes or triangle nodes within a bounded volume hierarchy using 64 bit ...
    pub const IMAGE_BVH64_INTERSECT_RAY: u16 = 231;
    /// Sample texels from an image surface using texel coordinates provided by the address input registers and store the res...
    pub const IMAGE_SAMPLE_CD_G16: u16 = 232;
    /// Sample texels from an image surface using texel coordinates provided by the address input registers and store the res...
    pub const IMAGE_SAMPLE_CD_CL_G16: u16 = 233;
    /// Sample texels from an image surface using texel coordinates provided by the address input registers and store the res...
    pub const IMAGE_SAMPLE_C_CD_G16: u16 = 234;
    /// Sample texels from an image surface using texel coordinates provided by the address input registers and store the res...
    pub const IMAGE_SAMPLE_C_CD_CL_G16: u16 = 235;
    /// Sample texels from an image surface using texel coordinates provided by the address input registers and store the res...
    pub const IMAGE_SAMPLE_CD_O_G16: u16 = 236;
    /// Sample texels from an image surface using texel coordinates provided by the address input registers and store the res...
    pub const IMAGE_SAMPLE_CD_CL_O_G16: u16 = 237;
    /// Sample texels from an image surface using texel coordinates provided by the address input registers and store the res...
    pub const IMAGE_SAMPLE_C_CD_O_G16: u16 = 238;
    /// Sample texels from an image surface using texel coordinates provided by the address input registers and store the res...
    pub const IMAGE_SAMPLE_C_CD_CL_O_G16: u16 = 239;

    /// All ENC_MIMG instructions.
    pub const TABLE: &[super::InstrEntry] = &[
        super::InstrEntry {
            name: "IMAGE_LOAD",
            opcode: 0,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_LOAD_MIP",
            opcode: 1,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_LOAD_PCK",
            opcode: 2,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_LOAD_PCK_SGN",
            opcode: 3,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_LOAD_MIP_PCK",
            opcode: 4,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_LOAD_MIP_PCK_SGN",
            opcode: 5,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_STORE",
            opcode: 8,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_STORE_MIP",
            opcode: 9,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_STORE_PCK",
            opcode: 10,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_STORE_MIP_PCK",
            opcode: 11,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_GET_RESINFO",
            opcode: 14,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_ATOMIC_SWAP",
            opcode: 15,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_ATOMIC_CMPSWAP",
            opcode: 16,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_ATOMIC_ADD",
            opcode: 17,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_ATOMIC_SUB",
            opcode: 18,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_ATOMIC_SMIN",
            opcode: 20,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_ATOMIC_UMIN",
            opcode: 21,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_ATOMIC_SMAX",
            opcode: 22,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_ATOMIC_UMAX",
            opcode: 23,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_ATOMIC_AND",
            opcode: 24,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_ATOMIC_OR",
            opcode: 25,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_ATOMIC_XOR",
            opcode: 26,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_ATOMIC_INC",
            opcode: 27,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_ATOMIC_DEC",
            opcode: 28,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_ATOMIC_FCMPSWAP",
            opcode: 29,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_ATOMIC_FMIN",
            opcode: 30,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_ATOMIC_FMAX",
            opcode: 31,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_SAMPLE",
            opcode: 32,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_SAMPLE_CL",
            opcode: 33,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_SAMPLE_D",
            opcode: 34,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_SAMPLE_D_CL",
            opcode: 35,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_SAMPLE_L",
            opcode: 36,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_SAMPLE_B",
            opcode: 37,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_SAMPLE_B_CL",
            opcode: 38,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_SAMPLE_LZ",
            opcode: 39,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_SAMPLE_C",
            opcode: 40,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_SAMPLE_C_CL",
            opcode: 41,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_SAMPLE_C_D",
            opcode: 42,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_SAMPLE_C_D_CL",
            opcode: 43,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_SAMPLE_C_L",
            opcode: 44,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_SAMPLE_C_B",
            opcode: 45,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_SAMPLE_C_B_CL",
            opcode: 46,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_SAMPLE_C_LZ",
            opcode: 47,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_SAMPLE_O",
            opcode: 48,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_SAMPLE_CL_O",
            opcode: 49,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_SAMPLE_D_O",
            opcode: 50,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_SAMPLE_D_CL_O",
            opcode: 51,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_SAMPLE_L_O",
            opcode: 52,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_SAMPLE_B_O",
            opcode: 53,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_SAMPLE_B_CL_O",
            opcode: 54,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_SAMPLE_LZ_O",
            opcode: 55,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_SAMPLE_C_O",
            opcode: 56,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_SAMPLE_C_CL_O",
            opcode: 57,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_SAMPLE_C_D_O",
            opcode: 58,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_SAMPLE_C_D_CL_O",
            opcode: 59,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_SAMPLE_C_L_O",
            opcode: 60,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_SAMPLE_C_B_O",
            opcode: 61,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_SAMPLE_C_B_CL_O",
            opcode: 62,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_SAMPLE_C_LZ_O",
            opcode: 63,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_GATHER4",
            opcode: 64,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_GATHER4_CL",
            opcode: 65,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_LOAD_BY2",
            opcode: 66,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_LOAD_BY4",
            opcode: 67,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_GATHER4_L",
            opcode: 68,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_GATHER4_B",
            opcode: 69,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_GATHER4_B_CL",
            opcode: 70,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_GATHER4_LZ",
            opcode: 71,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_GATHER4_C",
            opcode: 72,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_GATHER4_C_CL",
            opcode: 73,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_LOAD_MIP_BY2",
            opcode: 74,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_LOAD_MIP_BY4",
            opcode: 75,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_GATHER4_C_L",
            opcode: 76,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_GATHER4_C_B",
            opcode: 77,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_GATHER4_C_B_CL",
            opcode: 78,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_GATHER4_C_LZ",
            opcode: 79,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_GATHER4_O",
            opcode: 80,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_GATHER4_CL_O",
            opcode: 81,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_STORE_BY2",
            opcode: 82,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_STORE_BY4",
            opcode: 83,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_GATHER4_L_O",
            opcode: 84,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_GATHER4_B_O",
            opcode: 85,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_GATHER4_B_CL_O",
            opcode: 86,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_GATHER4_LZ_O",
            opcode: 87,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_GATHER4_C_O",
            opcode: 88,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_GATHER4_C_CL_O",
            opcode: 89,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_STORE_MIP_BY2",
            opcode: 90,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_STORE_MIP_BY4",
            opcode: 91,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_GATHER4_C_L_O",
            opcode: 92,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_GATHER4_C_B_O",
            opcode: 93,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_GATHER4_C_B_CL_O",
            opcode: 94,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_GATHER4_C_LZ_O",
            opcode: 95,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_GET_LOD",
            opcode: 96,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_GATHER4H",
            opcode: 97,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_GATHER4H_PCK",
            opcode: 98,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_GATHER8H_PCK",
            opcode: 99,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_SAMPLE_CD",
            opcode: 104,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_SAMPLE_CD_CL",
            opcode: 105,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_SAMPLE_C_CD",
            opcode: 106,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_SAMPLE_C_CD_CL",
            opcode: 107,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_SAMPLE_CD_O",
            opcode: 108,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_SAMPLE_CD_CL_O",
            opcode: 109,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_SAMPLE_C_CD_O",
            opcode: 110,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_SAMPLE_C_CD_CL_O",
            opcode: 111,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_LOAD_PCK2",
            opcode: 112,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_LOAD_PCK4",
            opcode: 113,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_LOAD_MIP_PCK2",
            opcode: 115,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_LOAD_MIP_PCK4",
            opcode: 116,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_STORE_PCK2",
            opcode: 118,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_STORE_PCK4",
            opcode: 119,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_STORE_MIP_PCK2",
            opcode: 121,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_STORE_MIP_PCK4",
            opcode: 122,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_MSAA_LOAD",
            opcode: 128,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_SAMPLE_D_G16",
            opcode: 162,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_SAMPLE_D_CL_G16",
            opcode: 163,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_SAMPLE_C_D_G16",
            opcode: 170,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_SAMPLE_C_D_CL_G16",
            opcode: 171,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_SAMPLE_D_O_G16",
            opcode: 178,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_SAMPLE_D_CL_O_G16",
            opcode: 179,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_SAMPLE_C_D_O_G16",
            opcode: 186,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_SAMPLE_C_D_CL_O_G16",
            opcode: 187,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_BVH_INTERSECT_RAY",
            opcode: 230,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_BVH64_INTERSECT_RAY",
            opcode: 231,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_SAMPLE_CD_G16",
            opcode: 232,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_SAMPLE_CD_CL_G16",
            opcode: 233,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_SAMPLE_C_CD_G16",
            opcode: 234,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_SAMPLE_C_CD_CL_G16",
            opcode: 235,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_SAMPLE_CD_O_G16",
            opcode: 236,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_SAMPLE_CD_CL_O_G16",
            opcode: 237,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_SAMPLE_C_CD_O_G16",
            opcode: 238,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "IMAGE_SAMPLE_C_CD_CL_O_G16",
            opcode: 239,
            is_branch: false,
            is_terminator: false,
        },
    ];

    /// Look up an instruction by opcode.
    pub fn lookup(opcode: u16) -> Option<&'static super::InstrEntry> {
        TABLE.iter().find(|e| e.opcode == opcode)
    }
}

/// ENC_MTBUF opcodes (16 instructions).
pub mod mtbuf {
    /// Load 1-component formatted data from a buffer surface, convert the data to 32 bit integral or floating point format, ...
    pub const TBUFFER_LOAD_FORMAT_X: u16 = 0;
    /// Load 2-component formatted data from a buffer surface, convert the data to 32 bit integral or floating point format, ...
    pub const TBUFFER_LOAD_FORMAT_XY: u16 = 1;
    /// Load 3-component formatted data from a buffer surface, convert the data to 32 bit integral or floating point format, ...
    pub const TBUFFER_LOAD_FORMAT_XYZ: u16 = 2;
    /// Load 4-component formatted data from a buffer surface, convert the data to 32 bit integral or floating point format, ...
    pub const TBUFFER_LOAD_FORMAT_XYZW: u16 = 3;
    /// Convert 32 bits of data from vector input registers into 1-component formatted data and store the data into a buffer ...
    pub const TBUFFER_STORE_FORMAT_X: u16 = 4;
    /// Convert 64 bits of data from vector input registers into 2-component formatted data and store the data into a buffer ...
    pub const TBUFFER_STORE_FORMAT_XY: u16 = 5;
    /// Convert 96 bits of data from vector input registers into 3-component formatted data and store the data into a buffer ...
    pub const TBUFFER_STORE_FORMAT_XYZ: u16 = 6;
    /// Convert 128 bits of data from vector input registers into 4-component formatted data and store the data into a buffer...
    pub const TBUFFER_STORE_FORMAT_XYZW: u16 = 7;
    /// Load 1-component formatted data from a buffer surface, convert the data to packed 16 bit integral or floating point f...
    pub const TBUFFER_LOAD_FORMAT_D16_X: u16 = 8;
    /// Load 2-component formatted data from a buffer surface, convert the data to packed 16 bit integral or floating point f...
    pub const TBUFFER_LOAD_FORMAT_D16_XY: u16 = 9;
    /// Load 3-component formatted data from a buffer surface, convert the data to packed 16 bit integral or floating point f...
    pub const TBUFFER_LOAD_FORMAT_D16_XYZ: u16 = 10;
    /// Load 4-component formatted data from a buffer surface, convert the data to packed 16 bit integral or floating point f...
    pub const TBUFFER_LOAD_FORMAT_D16_XYZW: u16 = 11;
    /// Convert 16 bits of data from vector input registers into 1-component formatted data and store the data into a buffer ...
    pub const TBUFFER_STORE_FORMAT_D16_X: u16 = 12;
    /// Convert 32 bits of data from vector input registers into 2-component formatted data and store the data into a buffer ...
    pub const TBUFFER_STORE_FORMAT_D16_XY: u16 = 13;
    /// Convert 48 bits of data from vector input registers into 3-component formatted data and store the data into a buffer ...
    pub const TBUFFER_STORE_FORMAT_D16_XYZ: u16 = 14;
    /// Convert 64 bits of data from vector input registers into 4-component formatted data and store the data into a buffer ...
    pub const TBUFFER_STORE_FORMAT_D16_XYZW: u16 = 15;

    /// All ENC_MTBUF instructions.
    pub const TABLE: &[super::InstrEntry] = &[
        super::InstrEntry {
            name: "TBUFFER_LOAD_FORMAT_X",
            opcode: 0,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "TBUFFER_LOAD_FORMAT_XY",
            opcode: 1,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "TBUFFER_LOAD_FORMAT_XYZ",
            opcode: 2,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "TBUFFER_LOAD_FORMAT_XYZW",
            opcode: 3,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "TBUFFER_STORE_FORMAT_X",
            opcode: 4,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "TBUFFER_STORE_FORMAT_XY",
            opcode: 5,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "TBUFFER_STORE_FORMAT_XYZ",
            opcode: 6,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "TBUFFER_STORE_FORMAT_XYZW",
            opcode: 7,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "TBUFFER_LOAD_FORMAT_D16_X",
            opcode: 8,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "TBUFFER_LOAD_FORMAT_D16_XY",
            opcode: 9,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "TBUFFER_LOAD_FORMAT_D16_XYZ",
            opcode: 10,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "TBUFFER_LOAD_FORMAT_D16_XYZW",
            opcode: 11,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "TBUFFER_STORE_FORMAT_D16_X",
            opcode: 12,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "TBUFFER_STORE_FORMAT_D16_XY",
            opcode: 13,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "TBUFFER_STORE_FORMAT_D16_XYZ",
            opcode: 14,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "TBUFFER_STORE_FORMAT_D16_XYZW",
            opcode: 15,
            is_branch: false,
            is_terminator: false,
        },
    ];

    /// Look up an instruction by opcode.
    pub fn lookup(opcode: u16) -> Option<&'static super::InstrEntry> {
        TABLE.iter().find(|e| e.opcode == opcode)
    }
}

/// ENC_MUBUF opcodes (75 instructions).
pub mod mubuf {
    /// Load 1-component formatted data from a buffer surface, convert the data to 32 bit integral or floating point format, ...
    pub const BUFFER_LOAD_FORMAT_X: u16 = 0;
    /// Load 2-component formatted data from a buffer surface, convert the data to 32 bit integral or floating point format, ...
    pub const BUFFER_LOAD_FORMAT_XY: u16 = 1;
    /// Load 3-component formatted data from a buffer surface, convert the data to 32 bit integral or floating point format, ...
    pub const BUFFER_LOAD_FORMAT_XYZ: u16 = 2;
    /// Load 4-component formatted data from a buffer surface, convert the data to 32 bit integral or floating point format, ...
    pub const BUFFER_LOAD_FORMAT_XYZW: u16 = 3;
    /// Convert 32 bits of data from vector input registers into 1-component formatted data and store the data into a buffer ...
    pub const BUFFER_STORE_FORMAT_X: u16 = 4;
    /// Convert 64 bits of data from vector input registers into 2-component formatted data and store the data into a buffer ...
    pub const BUFFER_STORE_FORMAT_XY: u16 = 5;
    /// Convert 96 bits of data from vector input registers into 3-component formatted data and store the data into a buffer ...
    pub const BUFFER_STORE_FORMAT_XYZ: u16 = 6;
    /// Convert 128 bits of data from vector input registers into 4-component formatted data and store the data into a buffer...
    pub const BUFFER_STORE_FORMAT_XYZW: u16 = 7;
    /// Load 8 bits of unsigned data from a buffer surface, zero extend to 32 bits and store the result into a vector register.
    pub const BUFFER_LOAD_UBYTE: u16 = 8;
    /// Load 8 bits of signed data from a buffer surface, sign extend to 32 bits and store the result into a vector register.
    pub const BUFFER_LOAD_SBYTE: u16 = 9;
    /// Load 16 bits of unsigned data from a buffer surface, zero extend to 32 bits and store the result into a vector register.
    pub const BUFFER_LOAD_USHORT: u16 = 10;
    /// Load 16 bits of signed data from a buffer surface, sign extend to 32 bits and store the result into a vector register.
    pub const BUFFER_LOAD_SSHORT: u16 = 11;
    /// Load 32 bits of data from a buffer surface into a vector register.
    pub const BUFFER_LOAD_DWORD: u16 = 12;
    /// Load 64 bits of data from a buffer surface into a vector register.
    pub const BUFFER_LOAD_DWORDX2: u16 = 13;
    /// Load 128 bits of data from a buffer surface into a vector register.
    pub const BUFFER_LOAD_DWORDX4: u16 = 14;
    /// Load 96 bits of data from a buffer surface into a vector register.
    pub const BUFFER_LOAD_DWORDX3: u16 = 15;
    /// Store 8 bits of data from a vector register into a buffer surface.
    pub const BUFFER_STORE_BYTE: u16 = 24;
    /// Store 8 bits of data from the high 16 bits of a 32-bit vector register into a buffer surface.
    pub const BUFFER_STORE_BYTE_D16_HI: u16 = 25;
    /// Store 16 bits of data from a vector register into a buffer surface.
    pub const BUFFER_STORE_SHORT: u16 = 26;
    /// Store 16 bits of data from the high 16 bits of a 32-bit vector register into a buffer surface.
    pub const BUFFER_STORE_SHORT_D16_HI: u16 = 27;
    /// Store 32 bits of data from vector input registers into a buffer surface.
    pub const BUFFER_STORE_DWORD: u16 = 28;
    /// Store 64 bits of data from vector input registers into a buffer surface.
    pub const BUFFER_STORE_DWORDX2: u16 = 29;
    /// Store 128 bits of data from vector input registers into a buffer surface.
    pub const BUFFER_STORE_DWORDX4: u16 = 30;
    /// Store 96 bits of data from vector input registers into a buffer surface.
    pub const BUFFER_STORE_DWORDX3: u16 = 31;
    /// Load 8 bits of unsigned data from a buffer surface, zero extend to 16 bits and store the result into the low 16 bits ...
    pub const BUFFER_LOAD_UBYTE_D16: u16 = 32;
    /// Load 8 bits of unsigned data from a buffer surface, zero extend to 16 bits and store the result into the high 16 bits...
    pub const BUFFER_LOAD_UBYTE_D16_HI: u16 = 33;
    /// Load 8 bits of signed data from a buffer surface, sign extend to 16 bits and store the result into the low 16 bits of...
    pub const BUFFER_LOAD_SBYTE_D16: u16 = 34;
    /// Load 8 bits of signed data from a buffer surface, sign extend to 16 bits and store the result into the high 16 bits o...
    pub const BUFFER_LOAD_SBYTE_D16_HI: u16 = 35;
    /// Load 16 bits of unsigned data from a buffer surface and store the result into the low 16 bits of a 32-bit vector regi...
    pub const BUFFER_LOAD_SHORT_D16: u16 = 36;
    /// Load 16 bits of unsigned data from a buffer surface and store the result into the high 16 bits of a 32-bit vector reg...
    pub const BUFFER_LOAD_SHORT_D16_HI: u16 = 37;
    /// Load 1-component formatted data from a buffer surface, convert the data to packed 16 bit integral or floating point f...
    pub const BUFFER_LOAD_FORMAT_D16_HI_X: u16 = 38;
    /// Convert 16 bits of data from the high 16 bits of a 32-bit vector input register into 1-component formatted data and s...
    pub const BUFFER_STORE_FORMAT_D16_HI_X: u16 = 39;
    /// Swap an unsigned 32-bit integer value in the data register with a location in a buffer surface. Store the original va...
    pub const BUFFER_ATOMIC_SWAP: u16 = 48;
    /// Compare two unsigned 32-bit integer values stored in the data comparison register and a location in a buffer surface....
    pub const BUFFER_ATOMIC_CMPSWAP: u16 = 49;
    /// Add two unsigned 32-bit integer values stored in the data register and a location in a buffer surface. Store the orig...
    pub const BUFFER_ATOMIC_ADD: u16 = 50;
    /// Subtract an unsigned 32-bit integer value stored in the data register from a value stored in a location in a buffer s...
    pub const BUFFER_ATOMIC_SUB: u16 = 51;
    /// Subtract an unsigned 32-bit integer location in a buffer surface from a value in the data register and clamp the resu...
    pub const BUFFER_ATOMIC_CSUB: u16 = 52;
    /// Select the minimum of two signed 32-bit integer inputs, given two values stored in the data register and a location i...
    pub const BUFFER_ATOMIC_SMIN: u16 = 53;
    /// Select the minimum of two unsigned 32-bit integer inputs, given two values stored in the data register and a location...
    pub const BUFFER_ATOMIC_UMIN: u16 = 54;
    /// Select the maximum of two signed 32-bit integer inputs, given two values stored in the data register and a location i...
    pub const BUFFER_ATOMIC_SMAX: u16 = 55;
    /// Select the maximum of two unsigned 32-bit integer inputs, given two values stored in the data register and a location...
    pub const BUFFER_ATOMIC_UMAX: u16 = 56;
    /// Calculate bitwise AND given two unsigned 32-bit integer values stored in the data register and a location in a buffer...
    pub const BUFFER_ATOMIC_AND: u16 = 57;
    /// Calculate bitwise OR given two unsigned 32-bit integer values stored in the data register and a location in a buffer ...
    pub const BUFFER_ATOMIC_OR: u16 = 58;
    /// Calculate bitwise XOR given two unsigned 32-bit integer values stored in the data register and a location in a buffer...
    pub const BUFFER_ATOMIC_XOR: u16 = 59;
    /// Increment an unsigned 32-bit integer value from a location in a buffer surface with wraparound to 0 if the value exce...
    pub const BUFFER_ATOMIC_INC: u16 = 60;
    /// Decrement an unsigned 32-bit integer value from a location in a buffer surface with wraparound to a value in the data...
    pub const BUFFER_ATOMIC_DEC: u16 = 61;
    /// Compare two single-precision float values stored in the data comparison register and a location in a buffer surface. ...
    pub const BUFFER_ATOMIC_FCMPSWAP: u16 = 62;
    /// Select the minimum of two single-precision float inputs, given two values stored in the data register and a location ...
    pub const BUFFER_ATOMIC_FMIN: u16 = 63;
    /// Select the maximum of two single-precision float inputs, given two values stored in the data register and a location ...
    pub const BUFFER_ATOMIC_FMAX: u16 = 64;
    /// Swap an unsigned 64-bit integer value in the data register with a location in a buffer surface. Store the original va...
    pub const BUFFER_ATOMIC_SWAP_X2: u16 = 80;
    /// Compare two unsigned 64-bit integer values stored in the data comparison register and a location in a buffer surface....
    pub const BUFFER_ATOMIC_CMPSWAP_X2: u16 = 81;
    /// Add two unsigned 64-bit integer values stored in the data register and a location in a buffer surface. Store the orig...
    pub const BUFFER_ATOMIC_ADD_X2: u16 = 82;
    /// Subtract an unsigned 64-bit integer value stored in the data register from a value stored in a location in a buffer s...
    pub const BUFFER_ATOMIC_SUB_X2: u16 = 83;
    /// Select the minimum of two signed 64-bit integer inputs, given two values stored in the data register and a location i...
    pub const BUFFER_ATOMIC_SMIN_X2: u16 = 85;
    /// Select the minimum of two unsigned 64-bit integer inputs, given two values stored in the data register and a location...
    pub const BUFFER_ATOMIC_UMIN_X2: u16 = 86;
    /// Select the maximum of two signed 64-bit integer inputs, given two values stored in the data register and a location i...
    pub const BUFFER_ATOMIC_SMAX_X2: u16 = 87;
    /// Select the maximum of two unsigned 64-bit integer inputs, given two values stored in the data register and a location...
    pub const BUFFER_ATOMIC_UMAX_X2: u16 = 88;
    /// Calculate bitwise AND given two unsigned 64-bit integer values stored in the data register and a location in a buffer...
    pub const BUFFER_ATOMIC_AND_X2: u16 = 89;
    /// Calculate bitwise OR given two unsigned 64-bit integer values stored in the data register and a location in a buffer ...
    pub const BUFFER_ATOMIC_OR_X2: u16 = 90;
    /// Calculate bitwise XOR given two unsigned 64-bit integer values stored in the data register and a location in a buffer...
    pub const BUFFER_ATOMIC_XOR_X2: u16 = 91;
    /// Increment an unsigned 64-bit integer value from a location in a buffer surface with wraparound to 0 if the value exce...
    pub const BUFFER_ATOMIC_INC_X2: u16 = 92;
    /// Decrement an unsigned 64-bit integer value from a location in a buffer surface with wraparound to a value in the data...
    pub const BUFFER_ATOMIC_DEC_X2: u16 = 93;
    /// Compare two double-precision float values stored in the data comparison register and a location in a buffer surface. ...
    pub const BUFFER_ATOMIC_FCMPSWAP_X2: u16 = 94;
    /// Select the minimum of two double-precision float inputs, given two values stored in the data register and a location ...
    pub const BUFFER_ATOMIC_FMIN_X2: u16 = 95;
    /// Select the maximum of two double-precision float inputs, given two values stored in the data register and a location ...
    pub const BUFFER_ATOMIC_FMAX_X2: u16 = 96;
    /// Write back and invalidate the shader L0. Returns ACK to shader.
    pub const BUFFER_GL0_INV: u16 = 113;
    /// Invalidate the GL1 cache only. Returns ACK to shader.
    pub const BUFFER_GL1_INV: u16 = 114;
    /// Load 1-component formatted data from a buffer surface, convert the data to packed 16 bit integral or floating point f...
    pub const BUFFER_LOAD_FORMAT_D16_X: u16 = 128;
    /// Load 2-component formatted data from a buffer surface, convert the data to packed 16 bit integral or floating point f...
    pub const BUFFER_LOAD_FORMAT_D16_XY: u16 = 129;
    /// Load 3-component formatted data from a buffer surface, convert the data to packed 16 bit integral or floating point f...
    pub const BUFFER_LOAD_FORMAT_D16_XYZ: u16 = 130;
    /// Load 4-component formatted data from a buffer surface, convert the data to packed 16 bit integral or floating point f...
    pub const BUFFER_LOAD_FORMAT_D16_XYZW: u16 = 131;
    /// Convert 16 bits of data from the low 16 bits of a 32-bit vector input register into 1-component formatted data and st...
    pub const BUFFER_STORE_FORMAT_D16_X: u16 = 132;
    /// Convert 32 bits of data from vector input registers into 2-component formatted data and store the data into a buffer ...
    pub const BUFFER_STORE_FORMAT_D16_XY: u16 = 133;
    /// Convert 48 bits of data from vector input registers into 3-component formatted data and store the data into a buffer ...
    pub const BUFFER_STORE_FORMAT_D16_XYZ: u16 = 134;
    /// Convert 64 bits of data from vector input registers into 4-component formatted data and store the data into a buffer ...
    pub const BUFFER_STORE_FORMAT_D16_XYZW: u16 = 135;

    /// All ENC_MUBUF instructions.
    pub const TABLE: &[super::InstrEntry] = &[
        super::InstrEntry {
            name: "BUFFER_LOAD_FORMAT_X",
            opcode: 0,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "BUFFER_LOAD_FORMAT_XY",
            opcode: 1,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "BUFFER_LOAD_FORMAT_XYZ",
            opcode: 2,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "BUFFER_LOAD_FORMAT_XYZW",
            opcode: 3,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "BUFFER_STORE_FORMAT_X",
            opcode: 4,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "BUFFER_STORE_FORMAT_XY",
            opcode: 5,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "BUFFER_STORE_FORMAT_XYZ",
            opcode: 6,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "BUFFER_STORE_FORMAT_XYZW",
            opcode: 7,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "BUFFER_LOAD_UBYTE",
            opcode: 8,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "BUFFER_LOAD_SBYTE",
            opcode: 9,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "BUFFER_LOAD_USHORT",
            opcode: 10,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "BUFFER_LOAD_SSHORT",
            opcode: 11,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "BUFFER_LOAD_DWORD",
            opcode: 12,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "BUFFER_LOAD_DWORDX2",
            opcode: 13,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "BUFFER_LOAD_DWORDX4",
            opcode: 14,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "BUFFER_LOAD_DWORDX3",
            opcode: 15,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "BUFFER_STORE_BYTE",
            opcode: 24,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "BUFFER_STORE_BYTE_D16_HI",
            opcode: 25,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "BUFFER_STORE_SHORT",
            opcode: 26,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "BUFFER_STORE_SHORT_D16_HI",
            opcode: 27,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "BUFFER_STORE_DWORD",
            opcode: 28,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "BUFFER_STORE_DWORDX2",
            opcode: 29,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "BUFFER_STORE_DWORDX4",
            opcode: 30,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "BUFFER_STORE_DWORDX3",
            opcode: 31,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "BUFFER_LOAD_UBYTE_D16",
            opcode: 32,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "BUFFER_LOAD_UBYTE_D16_HI",
            opcode: 33,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "BUFFER_LOAD_SBYTE_D16",
            opcode: 34,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "BUFFER_LOAD_SBYTE_D16_HI",
            opcode: 35,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "BUFFER_LOAD_SHORT_D16",
            opcode: 36,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "BUFFER_LOAD_SHORT_D16_HI",
            opcode: 37,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "BUFFER_LOAD_FORMAT_D16_HI_X",
            opcode: 38,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "BUFFER_STORE_FORMAT_D16_HI_X",
            opcode: 39,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "BUFFER_ATOMIC_SWAP",
            opcode: 48,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "BUFFER_ATOMIC_CMPSWAP",
            opcode: 49,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "BUFFER_ATOMIC_ADD",
            opcode: 50,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "BUFFER_ATOMIC_SUB",
            opcode: 51,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "BUFFER_ATOMIC_CSUB",
            opcode: 52,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "BUFFER_ATOMIC_SMIN",
            opcode: 53,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "BUFFER_ATOMIC_UMIN",
            opcode: 54,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "BUFFER_ATOMIC_SMAX",
            opcode: 55,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "BUFFER_ATOMIC_UMAX",
            opcode: 56,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "BUFFER_ATOMIC_AND",
            opcode: 57,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "BUFFER_ATOMIC_OR",
            opcode: 58,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "BUFFER_ATOMIC_XOR",
            opcode: 59,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "BUFFER_ATOMIC_INC",
            opcode: 60,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "BUFFER_ATOMIC_DEC",
            opcode: 61,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "BUFFER_ATOMIC_FCMPSWAP",
            opcode: 62,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "BUFFER_ATOMIC_FMIN",
            opcode: 63,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "BUFFER_ATOMIC_FMAX",
            opcode: 64,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "BUFFER_ATOMIC_SWAP_X2",
            opcode: 80,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "BUFFER_ATOMIC_CMPSWAP_X2",
            opcode: 81,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "BUFFER_ATOMIC_ADD_X2",
            opcode: 82,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "BUFFER_ATOMIC_SUB_X2",
            opcode: 83,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "BUFFER_ATOMIC_SMIN_X2",
            opcode: 85,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "BUFFER_ATOMIC_UMIN_X2",
            opcode: 86,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "BUFFER_ATOMIC_SMAX_X2",
            opcode: 87,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "BUFFER_ATOMIC_UMAX_X2",
            opcode: 88,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "BUFFER_ATOMIC_AND_X2",
            opcode: 89,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "BUFFER_ATOMIC_OR_X2",
            opcode: 90,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "BUFFER_ATOMIC_XOR_X2",
            opcode: 91,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "BUFFER_ATOMIC_INC_X2",
            opcode: 92,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "BUFFER_ATOMIC_DEC_X2",
            opcode: 93,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "BUFFER_ATOMIC_FCMPSWAP_X2",
            opcode: 94,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "BUFFER_ATOMIC_FMIN_X2",
            opcode: 95,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "BUFFER_ATOMIC_FMAX_X2",
            opcode: 96,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "BUFFER_GL0_INV",
            opcode: 113,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "BUFFER_GL1_INV",
            opcode: 114,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "BUFFER_LOAD_FORMAT_D16_X",
            opcode: 128,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "BUFFER_LOAD_FORMAT_D16_XY",
            opcode: 129,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "BUFFER_LOAD_FORMAT_D16_XYZ",
            opcode: 130,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "BUFFER_LOAD_FORMAT_D16_XYZW",
            opcode: 131,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "BUFFER_STORE_FORMAT_D16_X",
            opcode: 132,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "BUFFER_STORE_FORMAT_D16_XY",
            opcode: 133,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "BUFFER_STORE_FORMAT_D16_XYZ",
            opcode: 134,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "BUFFER_STORE_FORMAT_D16_XYZW",
            opcode: 135,
            is_branch: false,
            is_terminator: false,
        },
    ];

    /// Look up an instruction by opcode.
    pub fn lookup(opcode: u16) -> Option<&'static super::InstrEntry> {
        TABLE.iter().find(|e| e.opcode == opcode)
    }
}

/// ENC_SMEM opcodes (14 instructions).
pub mod smem {
    /// Load 32 bits of data from the scalar memory into a scalar register.
    pub const S_LOAD_DWORD: u16 = 0;
    /// Load 64 bits of data from the scalar memory into a scalar register.
    pub const S_LOAD_DWORDX2: u16 = 1;
    /// Load 128 bits of data from the scalar memory into a scalar register.
    pub const S_LOAD_DWORDX4: u16 = 2;
    /// Load 256 bits of data from the scalar memory into a scalar register.
    pub const S_LOAD_DWORDX8: u16 = 3;
    /// Load 512 bits of data from the scalar memory into a scalar register.
    pub const S_LOAD_DWORDX16: u16 = 4;
    /// Load 32 bits of data from a scalar buffer surface into a scalar register.
    pub const S_BUFFER_LOAD_DWORD: u16 = 8;
    /// Load 64 bits of data from a scalar buffer surface into a scalar register.
    pub const S_BUFFER_LOAD_DWORDX2: u16 = 9;
    /// Load 128 bits of data from a scalar buffer surface into a scalar register.
    pub const S_BUFFER_LOAD_DWORDX4: u16 = 10;
    /// Load 256 bits of data from a scalar buffer surface into a scalar register.
    pub const S_BUFFER_LOAD_DWORDX8: u16 = 11;
    /// Load 512 bits of data from a scalar buffer surface into a scalar register.
    pub const S_BUFFER_LOAD_DWORDX16: u16 = 12;
    /// Invalidate the GL1 cache only.
    pub const S_GL1_INV: u16 = 31;
    /// Invalidate the scalar data L0 cache.
    pub const S_DCACHE_INV: u16 = 32;
    /// Return current 64-bit timestamp.
    pub const S_MEMTIME: u16 = 36;
    /// Return current 64-bit RTC.
    pub const S_MEMREALTIME: u16 = 37;

    /// All ENC_SMEM instructions.
    pub const TABLE: &[super::InstrEntry] = &[
        super::InstrEntry {
            name: "S_LOAD_DWORD",
            opcode: 0,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_LOAD_DWORDX2",
            opcode: 1,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_LOAD_DWORDX4",
            opcode: 2,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_LOAD_DWORDX8",
            opcode: 3,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_LOAD_DWORDX16",
            opcode: 4,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_BUFFER_LOAD_DWORD",
            opcode: 8,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_BUFFER_LOAD_DWORDX2",
            opcode: 9,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_BUFFER_LOAD_DWORDX4",
            opcode: 10,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_BUFFER_LOAD_DWORDX8",
            opcode: 11,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_BUFFER_LOAD_DWORDX16",
            opcode: 12,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_GL1_INV",
            opcode: 31,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_DCACHE_INV",
            opcode: 32,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_MEMTIME",
            opcode: 36,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_MEMREALTIME",
            opcode: 37,
            is_branch: false,
            is_terminator: false,
        },
    ];

    /// Look up an instruction by opcode.
    pub fn lookup(opcode: u16) -> Option<&'static super::InstrEntry> {
        TABLE.iter().find(|e| e.opcode == opcode)
    }
}

/// ENC_SOP1 opcodes (65 instructions).
pub mod sop1 {
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
    pub const TABLE: &[super::InstrEntry] = &[
        super::InstrEntry {
            name: "S_MOV_B32",
            opcode: 3,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_MOV_B64",
            opcode: 4,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_CMOV_B32",
            opcode: 5,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_CMOV_B64",
            opcode: 6,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_NOT_B32",
            opcode: 7,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_NOT_B64",
            opcode: 8,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_WQM_B32",
            opcode: 9,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_WQM_B64",
            opcode: 10,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_BREV_B32",
            opcode: 11,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_BREV_B64",
            opcode: 12,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_BCNT0_I32_B32",
            opcode: 13,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_BCNT0_I32_B64",
            opcode: 14,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_BCNT1_I32_B32",
            opcode: 15,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_BCNT1_I32_B64",
            opcode: 16,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_FF0_I32_B32",
            opcode: 17,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_FF0_I32_B64",
            opcode: 18,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_FF1_I32_B32",
            opcode: 19,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_FF1_I32_B64",
            opcode: 20,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_FLBIT_I32_B32",
            opcode: 21,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_FLBIT_I32_B64",
            opcode: 22,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_FLBIT_I32",
            opcode: 23,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_FLBIT_I32_I64",
            opcode: 24,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_SEXT_I32_I8",
            opcode: 25,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_SEXT_I32_I16",
            opcode: 26,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_BITSET0_B32",
            opcode: 27,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_BITSET0_B64",
            opcode: 28,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_BITSET1_B32",
            opcode: 29,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_BITSET1_B64",
            opcode: 30,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_GETPC_B64",
            opcode: 31,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_SETPC_B64",
            opcode: 32,
            is_branch: true,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_SWAPPC_B64",
            opcode: 33,
            is_branch: true,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_RFE_B64",
            opcode: 34,
            is_branch: true,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_AND_SAVEEXEC_B64",
            opcode: 36,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_OR_SAVEEXEC_B64",
            opcode: 37,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_XOR_SAVEEXEC_B64",
            opcode: 38,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_ANDN2_SAVEEXEC_B64",
            opcode: 39,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_ORN2_SAVEEXEC_B64",
            opcode: 40,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_NAND_SAVEEXEC_B64",
            opcode: 41,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_NOR_SAVEEXEC_B64",
            opcode: 42,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_XNOR_SAVEEXEC_B64",
            opcode: 43,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_QUADMASK_B32",
            opcode: 44,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_QUADMASK_B64",
            opcode: 45,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_MOVRELS_B32",
            opcode: 46,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_MOVRELS_B64",
            opcode: 47,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_MOVRELD_B32",
            opcode: 48,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_MOVRELD_B64",
            opcode: 49,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_ABS_I32",
            opcode: 52,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_ANDN1_SAVEEXEC_B64",
            opcode: 55,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_ORN1_SAVEEXEC_B64",
            opcode: 56,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_ANDN1_WREXEC_B64",
            opcode: 57,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_ANDN2_WREXEC_B64",
            opcode: 58,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_BITREPLICATE_B64_B32",
            opcode: 59,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_AND_SAVEEXEC_B32",
            opcode: 60,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_OR_SAVEEXEC_B32",
            opcode: 61,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_XOR_SAVEEXEC_B32",
            opcode: 62,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_ANDN2_SAVEEXEC_B32",
            opcode: 63,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_ORN2_SAVEEXEC_B32",
            opcode: 64,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_NAND_SAVEEXEC_B32",
            opcode: 65,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_NOR_SAVEEXEC_B32",
            opcode: 66,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_XNOR_SAVEEXEC_B32",
            opcode: 67,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_ANDN1_SAVEEXEC_B32",
            opcode: 68,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_ORN1_SAVEEXEC_B32",
            opcode: 69,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_ANDN1_WREXEC_B32",
            opcode: 70,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_ANDN2_WREXEC_B32",
            opcode: 71,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_MOVRELSD_2_B32",
            opcode: 73,
            is_branch: false,
            is_terminator: false,
        },
    ];

    /// Look up an instruction by opcode.
    pub fn lookup(opcode: u16) -> Option<&'static super::InstrEntry> {
        TABLE.iter().find(|e| e.opcode == opcode)
    }
}

/// ENC_SOP2 opcodes (51 instructions).
pub mod sop2 {
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
    pub const TABLE: &[super::InstrEntry] = &[
        super::InstrEntry {
            name: "S_ADD_U32",
            opcode: 0,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_SUB_U32",
            opcode: 1,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_ADD_I32",
            opcode: 2,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_SUB_I32",
            opcode: 3,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_ADDC_U32",
            opcode: 4,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_SUBB_U32",
            opcode: 5,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_MIN_I32",
            opcode: 6,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_MIN_U32",
            opcode: 7,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_MAX_I32",
            opcode: 8,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_MAX_U32",
            opcode: 9,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_CSELECT_B32",
            opcode: 10,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_CSELECT_B64",
            opcode: 11,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_AND_B32",
            opcode: 14,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_AND_B64",
            opcode: 15,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_OR_B32",
            opcode: 16,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_OR_B64",
            opcode: 17,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_XOR_B32",
            opcode: 18,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_XOR_B64",
            opcode: 19,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_ANDN2_B32",
            opcode: 20,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_ANDN2_B64",
            opcode: 21,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_ORN2_B32",
            opcode: 22,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_ORN2_B64",
            opcode: 23,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_NAND_B32",
            opcode: 24,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_NAND_B64",
            opcode: 25,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_NOR_B32",
            opcode: 26,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_NOR_B64",
            opcode: 27,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_XNOR_B32",
            opcode: 28,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_XNOR_B64",
            opcode: 29,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_LSHL_B32",
            opcode: 30,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_LSHL_B64",
            opcode: 31,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_LSHR_B32",
            opcode: 32,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_LSHR_B64",
            opcode: 33,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_ASHR_I32",
            opcode: 34,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_ASHR_I64",
            opcode: 35,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_BFM_B32",
            opcode: 36,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_BFM_B64",
            opcode: 37,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_MUL_I32",
            opcode: 38,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_BFE_U32",
            opcode: 39,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_BFE_I32",
            opcode: 40,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_BFE_U64",
            opcode: 41,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_BFE_I64",
            opcode: 42,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_ABSDIFF_I32",
            opcode: 44,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_LSHL1_ADD_U32",
            opcode: 46,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_LSHL2_ADD_U32",
            opcode: 47,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_LSHL3_ADD_U32",
            opcode: 48,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_LSHL4_ADD_U32",
            opcode: 49,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_PACK_LL_B32_B16",
            opcode: 50,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_PACK_LH_B32_B16",
            opcode: 51,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_PACK_HH_B32_B16",
            opcode: 52,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_MUL_HI_U32",
            opcode: 53,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_MUL_HI_I32",
            opcode: 54,
            is_branch: false,
            is_terminator: false,
        },
    ];

    /// Look up an instruction by opcode.
    pub fn lookup(opcode: u16) -> Option<&'static super::InstrEntry> {
        TABLE.iter().find(|e| e.opcode == opcode)
    }
}

/// ENC_SOPC opcodes (18 instructions).
pub mod sopc {
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
    pub const TABLE: &[super::InstrEntry] = &[
        super::InstrEntry {
            name: "S_CMP_EQ_I32",
            opcode: 0,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_CMP_LG_I32",
            opcode: 1,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_CMP_GT_I32",
            opcode: 2,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_CMP_GE_I32",
            opcode: 3,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_CMP_LT_I32",
            opcode: 4,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_CMP_LE_I32",
            opcode: 5,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_CMP_EQ_U32",
            opcode: 6,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_CMP_LG_U32",
            opcode: 7,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_CMP_GT_U32",
            opcode: 8,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_CMP_GE_U32",
            opcode: 9,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_CMP_LT_U32",
            opcode: 10,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_CMP_LE_U32",
            opcode: 11,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_BITCMP0_B32",
            opcode: 12,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_BITCMP1_B32",
            opcode: 13,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_BITCMP0_B64",
            opcode: 14,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_BITCMP1_B64",
            opcode: 15,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_CMP_EQ_U64",
            opcode: 18,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_CMP_LG_U64",
            opcode: 19,
            is_branch: false,
            is_terminator: false,
        },
    ];

    /// Look up an instruction by opcode.
    pub fn lookup(opcode: u16) -> Option<&'static super::InstrEntry> {
        TABLE.iter().find(|e| e.opcode == opcode)
    }
}

/// ENC_SOPK opcodes (26 instructions).
pub mod sopk {
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
    pub const TABLE: &[super::InstrEntry] = &[
        super::InstrEntry {
            name: "S_MOVK_I32",
            opcode: 0,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_VERSION",
            opcode: 1,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_CMOVK_I32",
            opcode: 2,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_CMPK_EQ_I32",
            opcode: 3,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_CMPK_LG_I32",
            opcode: 4,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_CMPK_GT_I32",
            opcode: 5,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_CMPK_GE_I32",
            opcode: 6,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_CMPK_LT_I32",
            opcode: 7,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_CMPK_LE_I32",
            opcode: 8,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_CMPK_EQ_U32",
            opcode: 9,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_CMPK_LG_U32",
            opcode: 10,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_CMPK_GT_U32",
            opcode: 11,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_CMPK_GE_U32",
            opcode: 12,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_CMPK_LT_U32",
            opcode: 13,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_CMPK_LE_U32",
            opcode: 14,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_ADDK_I32",
            opcode: 15,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_MULK_I32",
            opcode: 16,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_GETREG_B32",
            opcode: 18,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_SETREG_B32",
            opcode: 19,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_CALL_B64",
            opcode: 22,
            is_branch: true,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_WAITCNT_VSCNT",
            opcode: 23,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_WAITCNT_VMCNT",
            opcode: 24,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_WAITCNT_EXPCNT",
            opcode: 25,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_WAITCNT_LGKMCNT",
            opcode: 26,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_SUBVECTOR_LOOP_BEGIN",
            opcode: 27,
            is_branch: true,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_SUBVECTOR_LOOP_END",
            opcode: 28,
            is_branch: true,
            is_terminator: false,
        },
    ];

    /// Look up an instruction by opcode.
    pub fn lookup(opcode: u16) -> Option<&'static super::InstrEntry> {
        TABLE.iter().find(|e| e.opcode == opcode)
    }
}

/// ENC_SOPP opcodes (36 instructions).
pub mod sopp {
    /// Do nothing. Delay issue of next instruction by a small, fixed amount.
    pub const S_NOP: u16 = 0;
    /// End of program; terminate wavefront.
    pub const S_ENDPGM: u16 = 1;
    /// Jump to a constant offset relative to the current PC.
    pub const S_BRANCH: u16 = 2;
    /// Allow a wave to 'ping' all the other waves in its threadgroup to force them to wake up early from an S_SLEEP instruct...
    pub const S_WAKEUP: u16 = 3;
    /// If SCC is 0 then jump to a constant offset relative to the current PC.
    pub const S_CBRANCH_SCC0: u16 = 4;
    /// If SCC is 1 then jump to a constant offset relative to the current PC.
    pub const S_CBRANCH_SCC1: u16 = 5;
    /// If VCCZ is 1 then jump to a constant offset relative to the current PC.
    pub const S_CBRANCH_VCCZ: u16 = 6;
    /// If VCCZ is 0 then jump to a constant offset relative to the current PC.
    pub const S_CBRANCH_VCCNZ: u16 = 7;
    /// If EXECZ is 1 then jump to a constant offset relative to the current PC.
    pub const S_CBRANCH_EXECZ: u16 = 8;
    /// If EXECZ is 0 then jump to a constant offset relative to the current PC.
    pub const S_CBRANCH_EXECNZ: u16 = 9;
    /// Synchronize waves within a threadgroup.
    pub const S_BARRIER: u16 = 10;
    /// Kill this wave if the least significant bit of the immediate constant is 1.
    pub const S_SETKILL: u16 = 11;
    /// Wait for the counts of outstanding local data share, vector memory and export instructions to be at or below the spec...
    pub const S_WAITCNT: u16 = 12;
    /// Set or clear the HALT or FATAL_HALT status bits.
    pub const S_SETHALT: u16 = 13;
    /// Cause a wave to sleep for up to ~8000 clocks.
    pub const S_SLEEP: u16 = 14;
    /// Change wave user priority.
    pub const S_SETPRIO: u16 = 15;
    /// Send a message upstream to graphics control hardware.
    pub const S_SENDMSG: u16 = 16;
    /// Send a message to upstream control hardware and then HALT the wavefront; see S_SENDMSG for details.
    pub const S_SENDMSGHALT: u16 = 17;
    /// Enter the trap handler.
    pub const S_TRAP: u16 = 18;
    /// Invalidate entire first level instruction cache.
    pub const S_ICACHE_INV: u16 = 19;
    /// Increment performance counter specified in SIMM16[3:0] by 1.
    pub const S_INCPERFLEVEL: u16 = 20;
    /// Decrement performance counter specified in SIMM16[3:0] by 1.
    pub const S_DECPERFLEVEL: u16 = 21;
    /// Send M0 as user data to the thread trace stream.
    pub const S_TTRACEDATA: u16 = 22;
    /// If the system debug flag is set then jump to a constant offset relative to the current PC.
    pub const S_CBRANCH_CDBGSYS: u16 = 23;
    /// If the user debug flag is set then jump to a constant offset relative to the current PC.
    pub const S_CBRANCH_CDBGUSER: u16 = 24;
    /// If either the system debug flag or the user debug flag is set then jump to a constant offset relative to the current PC.
    pub const S_CBRANCH_CDBGSYS_OR_USER: u16 = 25;
    /// If both the system debug flag and the user debug flag are set then jump to a constant offset relative to the current PC.
    pub const S_CBRANCH_CDBGSYS_AND_USER: u16 = 26;
    /// End of program; signal that a wave has been saved by the context-switch trap handler and terminate wavefront.
    pub const S_ENDPGM_SAVED: u16 = 27;
    /// End of program; signal that a wave has exited its POPS critical section and terminate wavefront.
    pub const S_ENDPGM_ORDERED_PS_DONE: u16 = 30;
    /// Generate an illegal instruction interrupt. This instruction is used to mark the end of a shader buffer for debug tools.
    pub const S_CODE_END: u16 = 31;
    /// Change instruction prefetch mode. This controls how many cachelines ahead of the current PC the shader attempts to pr...
    pub const S_INST_PREFETCH: u16 = 32;
    /// Mark the beginning of a clause.
    pub const S_CLAUSE: u16 = 33;
    /// Wait for one or more ALU-centric counters to fall below specified values. Used in expert scheduling mode.
    pub const S_WAITCNT_DEPCTR: u16 = 35;
    /// Set floating point round mode using an immediate constant.
    pub const S_ROUND_MODE: u16 = 36;
    /// Set floating point denormal mode using an immediate constant.
    pub const S_DENORM_MODE: u16 = 37;
    /// Send SIMM16[7:0] as user data to the thread trace stream.
    pub const S_TTRACEDATA_IMM: u16 = 40;

    /// All ENC_SOPP instructions.
    pub const TABLE: &[super::InstrEntry] = &[
        super::InstrEntry {
            name: "S_NOP",
            opcode: 0,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_ENDPGM",
            opcode: 1,
            is_branch: false,
            is_terminator: true,
        },
        super::InstrEntry {
            name: "S_BRANCH",
            opcode: 2,
            is_branch: true,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_WAKEUP",
            opcode: 3,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_CBRANCH_SCC0",
            opcode: 4,
            is_branch: true,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_CBRANCH_SCC1",
            opcode: 5,
            is_branch: true,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_CBRANCH_VCCZ",
            opcode: 6,
            is_branch: true,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_CBRANCH_VCCNZ",
            opcode: 7,
            is_branch: true,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_CBRANCH_EXECZ",
            opcode: 8,
            is_branch: true,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_CBRANCH_EXECNZ",
            opcode: 9,
            is_branch: true,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_BARRIER",
            opcode: 10,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_SETKILL",
            opcode: 11,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_WAITCNT",
            opcode: 12,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_SETHALT",
            opcode: 13,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_SLEEP",
            opcode: 14,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_SETPRIO",
            opcode: 15,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_SENDMSG",
            opcode: 16,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_SENDMSGHALT",
            opcode: 17,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_TRAP",
            opcode: 18,
            is_branch: true,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_ICACHE_INV",
            opcode: 19,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_INCPERFLEVEL",
            opcode: 20,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_DECPERFLEVEL",
            opcode: 21,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_TTRACEDATA",
            opcode: 22,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_CBRANCH_CDBGSYS",
            opcode: 23,
            is_branch: true,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_CBRANCH_CDBGUSER",
            opcode: 24,
            is_branch: true,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_CBRANCH_CDBGSYS_OR_USER",
            opcode: 25,
            is_branch: true,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_CBRANCH_CDBGSYS_AND_USER",
            opcode: 26,
            is_branch: true,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_ENDPGM_SAVED",
            opcode: 27,
            is_branch: false,
            is_terminator: true,
        },
        super::InstrEntry {
            name: "S_ENDPGM_ORDERED_PS_DONE",
            opcode: 30,
            is_branch: false,
            is_terminator: true,
        },
        super::InstrEntry {
            name: "S_CODE_END",
            opcode: 31,
            is_branch: true,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_INST_PREFETCH",
            opcode: 32,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_CLAUSE",
            opcode: 33,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_WAITCNT_DEPCTR",
            opcode: 35,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_ROUND_MODE",
            opcode: 36,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_DENORM_MODE",
            opcode: 37,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "S_TTRACEDATA_IMM",
            opcode: 40,
            is_branch: false,
            is_terminator: false,
        },
    ];

    /// Look up an instruction by opcode.
    pub fn lookup(opcode: u16) -> Option<&'static super::InstrEntry> {
        TABLE.iter().find(|e| e.opcode == opcode)
    }
}

/// ENC_VOP1 opcodes (81 instructions).
pub mod vop1 {
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
    pub const TABLE: &[super::InstrEntry] = &[
        super::InstrEntry {
            name: "V_NOP",
            opcode: 0,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_MOV_B32",
            opcode: 1,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_READFIRSTLANE_B32",
            opcode: 2,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CVT_I32_F64",
            opcode: 3,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CVT_F64_I32",
            opcode: 4,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CVT_F32_I32",
            opcode: 5,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CVT_F32_U32",
            opcode: 6,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CVT_U32_F32",
            opcode: 7,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CVT_I32_F32",
            opcode: 8,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CVT_F16_F32",
            opcode: 10,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CVT_F32_F16",
            opcode: 11,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CVT_RPI_I32_F32",
            opcode: 12,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CVT_FLR_I32_F32",
            opcode: 13,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CVT_OFF_F32_I4",
            opcode: 14,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CVT_F32_F64",
            opcode: 15,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CVT_F64_F32",
            opcode: 16,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CVT_F32_UBYTE0",
            opcode: 17,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CVT_F32_UBYTE1",
            opcode: 18,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CVT_F32_UBYTE2",
            opcode: 19,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CVT_F32_UBYTE3",
            opcode: 20,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CVT_U32_F64",
            opcode: 21,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CVT_F64_U32",
            opcode: 22,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_TRUNC_F64",
            opcode: 23,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CEIL_F64",
            opcode: 24,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_RNDNE_F64",
            opcode: 25,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_FLOOR_F64",
            opcode: 26,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_PIPEFLUSH",
            opcode: 27,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_FRACT_F32",
            opcode: 32,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_TRUNC_F32",
            opcode: 33,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CEIL_F32",
            opcode: 34,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_RNDNE_F32",
            opcode: 35,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_FLOOR_F32",
            opcode: 36,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_EXP_F32",
            opcode: 37,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_LOG_F32",
            opcode: 39,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_RCP_F32",
            opcode: 42,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_RCP_IFLAG_F32",
            opcode: 43,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_RSQ_F32",
            opcode: 46,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_RCP_F64",
            opcode: 47,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_RSQ_F64",
            opcode: 49,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_SQRT_F32",
            opcode: 51,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_SQRT_F64",
            opcode: 52,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_SIN_F32",
            opcode: 53,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_COS_F32",
            opcode: 54,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_NOT_B32",
            opcode: 55,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_BFREV_B32",
            opcode: 56,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_FFBH_U32",
            opcode: 57,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_FFBL_B32",
            opcode: 58,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_FFBH_I32",
            opcode: 59,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_FREXP_EXP_I32_F64",
            opcode: 60,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_FREXP_MANT_F64",
            opcode: 61,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_FRACT_F64",
            opcode: 62,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_FREXP_EXP_I32_F32",
            opcode: 63,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_FREXP_MANT_F32",
            opcode: 64,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CLREXCP",
            opcode: 65,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_MOVRELD_B32",
            opcode: 66,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_MOVRELS_B32",
            opcode: 67,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_MOVRELSD_B32",
            opcode: 68,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_MOVRELSD_2_B32",
            opcode: 72,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CVT_F16_U16",
            opcode: 80,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CVT_F16_I16",
            opcode: 81,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CVT_U16_F16",
            opcode: 82,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CVT_I16_F16",
            opcode: 83,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_RCP_F16",
            opcode: 84,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_SQRT_F16",
            opcode: 85,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_RSQ_F16",
            opcode: 86,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_LOG_F16",
            opcode: 87,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_EXP_F16",
            opcode: 88,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_FREXP_MANT_F16",
            opcode: 89,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_FREXP_EXP_I16_F16",
            opcode: 90,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_FLOOR_F16",
            opcode: 91,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CEIL_F16",
            opcode: 92,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_TRUNC_F16",
            opcode: 93,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_RNDNE_F16",
            opcode: 94,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_FRACT_F16",
            opcode: 95,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_SIN_F16",
            opcode: 96,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_COS_F16",
            opcode: 97,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_SAT_PK_U8_I16",
            opcode: 98,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CVT_NORM_I16_F16",
            opcode: 99,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CVT_NORM_U16_F16",
            opcode: 100,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_SWAP_B32",
            opcode: 101,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_SWAPREL_B32",
            opcode: 104,
            is_branch: false,
            is_terminator: false,
        },
    ];

    /// Look up an instruction by opcode.
    pub fn lookup(opcode: u16) -> Option<&'static super::InstrEntry> {
        TABLE.iter().find(|e| e.opcode == opcode)
    }
}

/// ENC_VOP2 opcodes (43 instructions).
pub mod vop2 {
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
    pub const TABLE: &[super::InstrEntry] = &[
        super::InstrEntry {
            name: "V_CNDMASK_B32",
            opcode: 1,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_DOT2C_F32_F16",
            opcode: 2,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_ADD_F32",
            opcode: 3,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_SUB_F32",
            opcode: 4,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_SUBREV_F32",
            opcode: 5,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_FMAC_LEGACY_F32",
            opcode: 6,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_MUL_LEGACY_F32",
            opcode: 7,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_MUL_F32",
            opcode: 8,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_MUL_I32_I24",
            opcode: 9,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_MUL_HI_I32_I24",
            opcode: 10,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_MUL_U32_U24",
            opcode: 11,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_MUL_HI_U32_U24",
            opcode: 12,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_DOT4C_I32_I8",
            opcode: 13,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_MIN_F32",
            opcode: 15,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_MAX_F32",
            opcode: 16,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_MIN_I32",
            opcode: 17,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_MAX_I32",
            opcode: 18,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_MIN_U32",
            opcode: 19,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_MAX_U32",
            opcode: 20,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_LSHRREV_B32",
            opcode: 22,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_ASHRREV_I32",
            opcode: 24,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_LSHLREV_B32",
            opcode: 26,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_AND_B32",
            opcode: 27,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_OR_B32",
            opcode: 28,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_XOR_B32",
            opcode: 29,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_XNOR_B32",
            opcode: 30,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_ADD_NC_U32",
            opcode: 37,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_SUB_NC_U32",
            opcode: 38,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_SUBREV_NC_U32",
            opcode: 39,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_ADD_CO_CI_U32",
            opcode: 40,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_SUB_CO_CI_U32",
            opcode: 41,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_SUBREV_CO_CI_U32",
            opcode: 42,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_FMAC_F32",
            opcode: 43,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CVT_PKRTZ_F16_F32",
            opcode: 47,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_ADD_F16",
            opcode: 50,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_SUB_F16",
            opcode: 51,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_SUBREV_F16",
            opcode: 52,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_MUL_F16",
            opcode: 53,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_FMAC_F16",
            opcode: 54,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_MAX_F16",
            opcode: 57,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_MIN_F16",
            opcode: 58,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_LDEXP_F16",
            opcode: 59,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_PK_FMAC_F16",
            opcode: 60,
            is_branch: false,
            is_terminator: false,
        },
    ];

    /// Look up an instruction by opcode.
    pub fn lookup(opcode: u16) -> Option<&'static super::InstrEntry> {
        TABLE.iter().find(|e| e.opcode == opcode)
    }
}

/// ENC_VOP3 opcodes (416 instructions).
pub mod vop3 {
    /// Set the vector condition code to 0. Store the result into VCC or a scalar register.
    pub const V_CMP_F_F32: u16 = 0;
    /// Set the vector condition code to 1 iff the first input is less than the second input. Store the result into VCC or a ...
    pub const V_CMP_LT_F32: u16 = 1;
    /// Set the vector condition code to 1 iff the first input is equal to the second input. Store the result into VCC or a s...
    pub const V_CMP_EQ_F32: u16 = 2;
    /// Set the vector condition code to 1 iff the first input is less than or equal to the second input. Store the result in...
    pub const V_CMP_LE_F32: u16 = 3;
    /// Set the vector condition code to 1 iff the first input is greater than the second input. Store the result into VCC or...
    pub const V_CMP_GT_F32: u16 = 4;
    /// Set the vector condition code to 1 iff the first input is less than or greater than the second input. Store the resul...
    pub const V_CMP_LG_F32: u16 = 5;
    /// Set the vector condition code to 1 iff the first input is greater than or equal to the second input. Store the result...
    pub const V_CMP_GE_F32: u16 = 6;
    /// Set the vector condition code to 1 iff the first input is orderable to the second input. Store the result into VCC or...
    pub const V_CMP_O_F32: u16 = 7;
    /// Set the vector condition code to 1 iff the first input is not orderable to the second input. Store the result into VC...
    pub const V_CMP_U_F32: u16 = 8;
    /// Set the vector condition code to 1 iff the first input is not greater than or equal to the second input. Store the re...
    pub const V_CMP_NGE_F32: u16 = 9;
    /// Set the vector condition code to 1 iff the first input is not less than or greater than the second input. Store the r...
    pub const V_CMP_NLG_F32: u16 = 10;
    /// Set the vector condition code to 1 iff the first input is not greater than the second input. Store the result into VC...
    pub const V_CMP_NGT_F32: u16 = 11;
    /// Set the vector condition code to 1 iff the first input is not less than or equal to the second input. Store the resul...
    pub const V_CMP_NLE_F32: u16 = 12;
    /// Set the vector condition code to 1 iff the first input is not equal to the second input. Store the result into VCC or...
    pub const V_CMP_NEQ_F32: u16 = 13;
    /// Set the vector condition code to 1 iff the first input is not less than the second input. Store the result into VCC o...
    pub const V_CMP_NLT_F32: u16 = 14;
    /// Set the vector condition code to 1. Store the result into VCC or a scalar register.
    pub const V_CMP_TRU_F32: u16 = 15;
    /// Set the vector condition code to 0. Store the result into the EXEC mask.
    pub const V_CMPX_F_F32: u16 = 16;
    /// Set the vector condition code to 1 iff the first input is less than the second input. Store the result into the EXEC ...
    pub const V_CMPX_LT_F32: u16 = 17;
    /// Set the vector condition code to 1 iff the first input is equal to the second input. Store the result into the EXEC m...
    pub const V_CMPX_EQ_F32: u16 = 18;
    /// Set the vector condition code to 1 iff the first input is less than or equal to the second input. Store the result in...
    pub const V_CMPX_LE_F32: u16 = 19;
    /// Set the vector condition code to 1 iff the first input is greater than the second input. Store the result into the EX...
    pub const V_CMPX_GT_F32: u16 = 20;
    /// Set the vector condition code to 1 iff the first input is less than or greater than the second input. Store the resul...
    pub const V_CMPX_LG_F32: u16 = 21;
    /// Set the vector condition code to 1 iff the first input is greater than or equal to the second input. Store the result...
    pub const V_CMPX_GE_F32: u16 = 22;
    /// Set the vector condition code to 1 iff the first input is orderable to the second input. Store the result into the EX...
    pub const V_CMPX_O_F32: u16 = 23;
    /// Set the vector condition code to 1 iff the first input is not orderable to the second input. Store the result into th...
    pub const V_CMPX_U_F32: u16 = 24;
    /// Set the vector condition code to 1 iff the first input is not greater than or equal to the second input. Store the re...
    pub const V_CMPX_NGE_F32: u16 = 25;
    /// Set the vector condition code to 1 iff the first input is not less than or greater than the second input. Store the r...
    pub const V_CMPX_NLG_F32: u16 = 26;
    /// Set the vector condition code to 1 iff the first input is not greater than the second input. Store the result into th...
    pub const V_CMPX_NGT_F32: u16 = 27;
    /// Set the vector condition code to 1 iff the first input is not less than or equal to the second input. Store the resul...
    pub const V_CMPX_NLE_F32: u16 = 28;
    /// Set the vector condition code to 1 iff the first input is not equal to the second input. Store the result into the EX...
    pub const V_CMPX_NEQ_F32: u16 = 29;
    /// Set the vector condition code to 1 iff the first input is not less than the second input. Store the result into the E...
    pub const V_CMPX_NLT_F32: u16 = 30;
    /// Set the vector condition code to 1. Store the result into the EXEC mask.
    pub const V_CMPX_TRU_F32: u16 = 31;
    /// Set the vector condition code to 0. Store the result into VCC or a scalar register.
    pub const V_CMP_F_F64: u16 = 32;
    /// Set the vector condition code to 1 iff the first input is less than the second input. Store the result into VCC or a ...
    pub const V_CMP_LT_F64: u16 = 33;
    /// Set the vector condition code to 1 iff the first input is equal to the second input. Store the result into VCC or a s...
    pub const V_CMP_EQ_F64: u16 = 34;
    /// Set the vector condition code to 1 iff the first input is less than or equal to the second input. Store the result in...
    pub const V_CMP_LE_F64: u16 = 35;
    /// Set the vector condition code to 1 iff the first input is greater than the second input. Store the result into VCC or...
    pub const V_CMP_GT_F64: u16 = 36;
    /// Set the vector condition code to 1 iff the first input is less than or greater than the second input. Store the resul...
    pub const V_CMP_LG_F64: u16 = 37;
    /// Set the vector condition code to 1 iff the first input is greater than or equal to the second input. Store the result...
    pub const V_CMP_GE_F64: u16 = 38;
    /// Set the vector condition code to 1 iff the first input is orderable to the second input. Store the result into VCC or...
    pub const V_CMP_O_F64: u16 = 39;
    /// Set the vector condition code to 1 iff the first input is not orderable to the second input. Store the result into VC...
    pub const V_CMP_U_F64: u16 = 40;
    /// Set the vector condition code to 1 iff the first input is not greater than or equal to the second input. Store the re...
    pub const V_CMP_NGE_F64: u16 = 41;
    /// Set the vector condition code to 1 iff the first input is not less than or greater than the second input. Store the r...
    pub const V_CMP_NLG_F64: u16 = 42;
    /// Set the vector condition code to 1 iff the first input is not greater than the second input. Store the result into VC...
    pub const V_CMP_NGT_F64: u16 = 43;
    /// Set the vector condition code to 1 iff the first input is not less than or equal to the second input. Store the resul...
    pub const V_CMP_NLE_F64: u16 = 44;
    /// Set the vector condition code to 1 iff the first input is not equal to the second input. Store the result into VCC or...
    pub const V_CMP_NEQ_F64: u16 = 45;
    /// Set the vector condition code to 1 iff the first input is not less than the second input. Store the result into VCC o...
    pub const V_CMP_NLT_F64: u16 = 46;
    /// Set the vector condition code to 1. Store the result into VCC or a scalar register.
    pub const V_CMP_TRU_F64: u16 = 47;
    /// Set the vector condition code to 0. Store the result into the EXEC mask.
    pub const V_CMPX_F_F64: u16 = 48;
    /// Set the vector condition code to 1 iff the first input is less than the second input. Store the result into the EXEC ...
    pub const V_CMPX_LT_F64: u16 = 49;
    /// Set the vector condition code to 1 iff the first input is equal to the second input. Store the result into the EXEC m...
    pub const V_CMPX_EQ_F64: u16 = 50;
    /// Set the vector condition code to 1 iff the first input is less than or equal to the second input. Store the result in...
    pub const V_CMPX_LE_F64: u16 = 51;
    /// Set the vector condition code to 1 iff the first input is greater than the second input. Store the result into the EX...
    pub const V_CMPX_GT_F64: u16 = 52;
    /// Set the vector condition code to 1 iff the first input is less than or greater than the second input. Store the resul...
    pub const V_CMPX_LG_F64: u16 = 53;
    /// Set the vector condition code to 1 iff the first input is greater than or equal to the second input. Store the result...
    pub const V_CMPX_GE_F64: u16 = 54;
    /// Set the vector condition code to 1 iff the first input is orderable to the second input. Store the result into the EX...
    pub const V_CMPX_O_F64: u16 = 55;
    /// Set the vector condition code to 1 iff the first input is not orderable to the second input. Store the result into th...
    pub const V_CMPX_U_F64: u16 = 56;
    /// Set the vector condition code to 1 iff the first input is not greater than or equal to the second input. Store the re...
    pub const V_CMPX_NGE_F64: u16 = 57;
    /// Set the vector condition code to 1 iff the first input is not less than or greater than the second input. Store the r...
    pub const V_CMPX_NLG_F64: u16 = 58;
    /// Set the vector condition code to 1 iff the first input is not greater than the second input. Store the result into th...
    pub const V_CMPX_NGT_F64: u16 = 59;
    /// Set the vector condition code to 1 iff the first input is not less than or equal to the second input. Store the resul...
    pub const V_CMPX_NLE_F64: u16 = 60;
    /// Set the vector condition code to 1 iff the first input is not equal to the second input. Store the result into the EX...
    pub const V_CMPX_NEQ_F64: u16 = 61;
    /// Set the vector condition code to 1 iff the first input is not less than the second input. Store the result into the E...
    pub const V_CMPX_NLT_F64: u16 = 62;
    /// Set the vector condition code to 1. Store the result into the EXEC mask.
    pub const V_CMPX_TRU_F64: u16 = 63;
    /// Set the vector condition code to 0. Store the result into VCC or a scalar register.
    pub const V_CMP_F_I32: u16 = 128;
    /// Set the vector condition code to 1 iff the first input is less than the second input. Store the result into VCC or a ...
    pub const V_CMP_LT_I32: u16 = 129;
    /// Set the vector condition code to 1 iff the first input is equal to the second input. Store the result into VCC or a s...
    pub const V_CMP_EQ_I32: u16 = 130;
    /// Set the vector condition code to 1 iff the first input is less than or equal to the second input. Store the result in...
    pub const V_CMP_LE_I32: u16 = 131;
    /// Set the vector condition code to 1 iff the first input is greater than the second input. Store the result into VCC or...
    pub const V_CMP_GT_I32: u16 = 132;
    /// Set the vector condition code to 1 iff the first input is not equal to the second input. Store the result into VCC or...
    pub const V_CMP_NE_I32: u16 = 133;
    /// Set the vector condition code to 1 iff the first input is greater than or equal to the second input. Store the result...
    pub const V_CMP_GE_I32: u16 = 134;
    /// Set the vector condition code to 1. Store the result into VCC or a scalar register.
    pub const V_CMP_T_I32: u16 = 135;
    /// Evaluate the IEEE numeric class function specified as a 10 bit mask in the second input on the first input, a single-...
    pub const V_CMP_CLASS_F32: u16 = 136;
    /// Set the vector condition code to 1 iff the first input is less than the second input. Store the result into VCC or a ...
    pub const V_CMP_LT_I16: u16 = 137;
    /// Set the vector condition code to 1 iff the first input is equal to the second input. Store the result into VCC or a s...
    pub const V_CMP_EQ_I16: u16 = 138;
    /// Set the vector condition code to 1 iff the first input is less than or equal to the second input. Store the result in...
    pub const V_CMP_LE_I16: u16 = 139;
    /// Set the vector condition code to 1 iff the first input is greater than the second input. Store the result into VCC or...
    pub const V_CMP_GT_I16: u16 = 140;
    /// Set the vector condition code to 1 iff the first input is not equal to the second input. Store the result into VCC or...
    pub const V_CMP_NE_I16: u16 = 141;
    /// Set the vector condition code to 1 iff the first input is greater than or equal to the second input. Store the result...
    pub const V_CMP_GE_I16: u16 = 142;
    /// Evaluate the IEEE numeric class function specified as a 10 bit mask in the second input on the first input, a half-pr...
    pub const V_CMP_CLASS_F16: u16 = 143;
    /// Set the vector condition code to 0. Store the result into the EXEC mask.
    pub const V_CMPX_F_I32: u16 = 144;
    /// Set the vector condition code to 1 iff the first input is less than the second input. Store the result into the EXEC ...
    pub const V_CMPX_LT_I32: u16 = 145;
    /// Set the vector condition code to 1 iff the first input is equal to the second input. Store the result into the EXEC m...
    pub const V_CMPX_EQ_I32: u16 = 146;
    /// Set the vector condition code to 1 iff the first input is less than or equal to the second input. Store the result in...
    pub const V_CMPX_LE_I32: u16 = 147;
    /// Set the vector condition code to 1 iff the first input is greater than the second input. Store the result into the EX...
    pub const V_CMPX_GT_I32: u16 = 148;
    /// Set the vector condition code to 1 iff the first input is not equal to the second input. Store the result into the EX...
    pub const V_CMPX_NE_I32: u16 = 149;
    /// Set the vector condition code to 1 iff the first input is greater than or equal to the second input. Store the result...
    pub const V_CMPX_GE_I32: u16 = 150;
    /// Set the vector condition code to 1. Store the result into the EXEC mask.
    pub const V_CMPX_T_I32: u16 = 151;
    /// Evaluate the IEEE numeric class function specified as a 10 bit mask in the second input on the first input, a single-...
    pub const V_CMPX_CLASS_F32: u16 = 152;
    /// Set the vector condition code to 1 iff the first input is less than the second input. Store the result into the EXEC ...
    pub const V_CMPX_LT_I16: u16 = 153;
    /// Set the vector condition code to 1 iff the first input is equal to the second input. Store the result into the EXEC m...
    pub const V_CMPX_EQ_I16: u16 = 154;
    /// Set the vector condition code to 1 iff the first input is less than or equal to the second input. Store the result in...
    pub const V_CMPX_LE_I16: u16 = 155;
    /// Set the vector condition code to 1 iff the first input is greater than the second input. Store the result into the EX...
    pub const V_CMPX_GT_I16: u16 = 156;
    /// Set the vector condition code to 1 iff the first input is not equal to the second input. Store the result into the EX...
    pub const V_CMPX_NE_I16: u16 = 157;
    /// Set the vector condition code to 1 iff the first input is greater than or equal to the second input. Store the result...
    pub const V_CMPX_GE_I16: u16 = 158;
    /// Evaluate the IEEE numeric class function specified as a 10 bit mask in the second input on the first input, a half-pr...
    pub const V_CMPX_CLASS_F16: u16 = 159;
    /// Set the vector condition code to 0. Store the result into VCC or a scalar register.
    pub const V_CMP_F_I64: u16 = 160;
    /// Set the vector condition code to 1 iff the first input is less than the second input. Store the result into VCC or a ...
    pub const V_CMP_LT_I64: u16 = 161;
    /// Set the vector condition code to 1 iff the first input is equal to the second input. Store the result into VCC or a s...
    pub const V_CMP_EQ_I64: u16 = 162;
    /// Set the vector condition code to 1 iff the first input is less than or equal to the second input. Store the result in...
    pub const V_CMP_LE_I64: u16 = 163;
    /// Set the vector condition code to 1 iff the first input is greater than the second input. Store the result into VCC or...
    pub const V_CMP_GT_I64: u16 = 164;
    /// Set the vector condition code to 1 iff the first input is not equal to the second input. Store the result into VCC or...
    pub const V_CMP_NE_I64: u16 = 165;
    /// Set the vector condition code to 1 iff the first input is greater than or equal to the second input. Store the result...
    pub const V_CMP_GE_I64: u16 = 166;
    /// Set the vector condition code to 1. Store the result into VCC or a scalar register.
    pub const V_CMP_T_I64: u16 = 167;
    /// Evaluate the IEEE numeric class function specified as a 10 bit mask in the second input on the first input, a double-...
    pub const V_CMP_CLASS_F64: u16 = 168;
    /// Set the vector condition code to 1 iff the first input is less than the second input. Store the result into VCC or a ...
    pub const V_CMP_LT_U16: u16 = 169;
    /// Set the vector condition code to 1 iff the first input is equal to the second input. Store the result into VCC or a s...
    pub const V_CMP_EQ_U16: u16 = 170;
    /// Set the vector condition code to 1 iff the first input is less than or equal to the second input. Store the result in...
    pub const V_CMP_LE_U16: u16 = 171;
    /// Set the vector condition code to 1 iff the first input is greater than the second input. Store the result into VCC or...
    pub const V_CMP_GT_U16: u16 = 172;
    /// Set the vector condition code to 1 iff the first input is not equal to the second input. Store the result into VCC or...
    pub const V_CMP_NE_U16: u16 = 173;
    /// Set the vector condition code to 1 iff the first input is greater than or equal to the second input. Store the result...
    pub const V_CMP_GE_U16: u16 = 174;
    /// Set the vector condition code to 0. Store the result into the EXEC mask.
    pub const V_CMPX_F_I64: u16 = 176;
    /// Set the vector condition code to 1 iff the first input is less than the second input. Store the result into the EXEC ...
    pub const V_CMPX_LT_I64: u16 = 177;
    /// Set the vector condition code to 1 iff the first input is equal to the second input. Store the result into the EXEC m...
    pub const V_CMPX_EQ_I64: u16 = 178;
    /// Set the vector condition code to 1 iff the first input is less than or equal to the second input. Store the result in...
    pub const V_CMPX_LE_I64: u16 = 179;
    /// Set the vector condition code to 1 iff the first input is greater than the second input. Store the result into the EX...
    pub const V_CMPX_GT_I64: u16 = 180;
    /// Set the vector condition code to 1 iff the first input is not equal to the second input. Store the result into the EX...
    pub const V_CMPX_NE_I64: u16 = 181;
    /// Set the vector condition code to 1 iff the first input is greater than or equal to the second input. Store the result...
    pub const V_CMPX_GE_I64: u16 = 182;
    /// Set the vector condition code to 1. Store the result into the EXEC mask.
    pub const V_CMPX_T_I64: u16 = 183;
    /// Evaluate the IEEE numeric class function specified as a 10 bit mask in the second input on the first input, a double-...
    pub const V_CMPX_CLASS_F64: u16 = 184;
    /// Set the vector condition code to 1 iff the first input is less than the second input. Store the result into the EXEC ...
    pub const V_CMPX_LT_U16: u16 = 185;
    /// Set the vector condition code to 1 iff the first input is equal to the second input. Store the result into the EXEC m...
    pub const V_CMPX_EQ_U16: u16 = 186;
    /// Set the vector condition code to 1 iff the first input is less than or equal to the second input. Store the result in...
    pub const V_CMPX_LE_U16: u16 = 187;
    /// Set the vector condition code to 1 iff the first input is greater than the second input. Store the result into the EX...
    pub const V_CMPX_GT_U16: u16 = 188;
    /// Set the vector condition code to 1 iff the first input is not equal to the second input. Store the result into the EX...
    pub const V_CMPX_NE_U16: u16 = 189;
    /// Set the vector condition code to 1 iff the first input is greater than or equal to the second input. Store the result...
    pub const V_CMPX_GE_U16: u16 = 190;
    /// Set the vector condition code to 0. Store the result into VCC or a scalar register.
    pub const V_CMP_F_U32: u16 = 192;
    /// Set the vector condition code to 1 iff the first input is less than the second input. Store the result into VCC or a ...
    pub const V_CMP_LT_U32: u16 = 193;
    /// Set the vector condition code to 1 iff the first input is equal to the second input. Store the result into VCC or a s...
    pub const V_CMP_EQ_U32: u16 = 194;
    /// Set the vector condition code to 1 iff the first input is less than or equal to the second input. Store the result in...
    pub const V_CMP_LE_U32: u16 = 195;
    /// Set the vector condition code to 1 iff the first input is greater than the second input. Store the result into VCC or...
    pub const V_CMP_GT_U32: u16 = 196;
    /// Set the vector condition code to 1 iff the first input is not equal to the second input. Store the result into VCC or...
    pub const V_CMP_NE_U32: u16 = 197;
    /// Set the vector condition code to 1 iff the first input is greater than or equal to the second input. Store the result...
    pub const V_CMP_GE_U32: u16 = 198;
    /// Set the vector condition code to 1. Store the result into VCC or a scalar register.
    pub const V_CMP_T_U32: u16 = 199;
    /// Set the vector condition code to 0. Store the result into VCC or a scalar register.
    pub const V_CMP_F_F16: u16 = 200;
    /// Set the vector condition code to 1 iff the first input is less than the second input. Store the result into VCC or a ...
    pub const V_CMP_LT_F16: u16 = 201;
    /// Set the vector condition code to 1 iff the first input is equal to the second input. Store the result into VCC or a s...
    pub const V_CMP_EQ_F16: u16 = 202;
    /// Set the vector condition code to 1 iff the first input is less than or equal to the second input. Store the result in...
    pub const V_CMP_LE_F16: u16 = 203;
    /// Set the vector condition code to 1 iff the first input is greater than the second input. Store the result into VCC or...
    pub const V_CMP_GT_F16: u16 = 204;
    /// Set the vector condition code to 1 iff the first input is less than or greater than the second input. Store the resul...
    pub const V_CMP_LG_F16: u16 = 205;
    /// Set the vector condition code to 1 iff the first input is greater than or equal to the second input. Store the result...
    pub const V_CMP_GE_F16: u16 = 206;
    /// Set the vector condition code to 1 iff the first input is orderable to the second input. Store the result into VCC or...
    pub const V_CMP_O_F16: u16 = 207;
    /// Set the vector condition code to 0. Store the result into the EXEC mask.
    pub const V_CMPX_F_U32: u16 = 208;
    /// Set the vector condition code to 1 iff the first input is less than the second input. Store the result into the EXEC ...
    pub const V_CMPX_LT_U32: u16 = 209;
    /// Set the vector condition code to 1 iff the first input is equal to the second input. Store the result into the EXEC m...
    pub const V_CMPX_EQ_U32: u16 = 210;
    /// Set the vector condition code to 1 iff the first input is less than or equal to the second input. Store the result in...
    pub const V_CMPX_LE_U32: u16 = 211;
    /// Set the vector condition code to 1 iff the first input is greater than the second input. Store the result into the EX...
    pub const V_CMPX_GT_U32: u16 = 212;
    /// Set the vector condition code to 1 iff the first input is not equal to the second input. Store the result into the EX...
    pub const V_CMPX_NE_U32: u16 = 213;
    /// Set the vector condition code to 1 iff the first input is greater than or equal to the second input. Store the result...
    pub const V_CMPX_GE_U32: u16 = 214;
    /// Set the vector condition code to 1. Store the result into the EXEC mask.
    pub const V_CMPX_T_U32: u16 = 215;
    /// Set the vector condition code to 0. Store the result into the EXEC mask.
    pub const V_CMPX_F_F16: u16 = 216;
    /// Set the vector condition code to 1 iff the first input is less than the second input. Store the result into the EXEC ...
    pub const V_CMPX_LT_F16: u16 = 217;
    /// Set the vector condition code to 1 iff the first input is equal to the second input. Store the result into the EXEC m...
    pub const V_CMPX_EQ_F16: u16 = 218;
    /// Set the vector condition code to 1 iff the first input is less than or equal to the second input. Store the result in...
    pub const V_CMPX_LE_F16: u16 = 219;
    /// Set the vector condition code to 1 iff the first input is greater than the second input. Store the result into the EX...
    pub const V_CMPX_GT_F16: u16 = 220;
    /// Set the vector condition code to 1 iff the first input is less than or greater than the second input. Store the resul...
    pub const V_CMPX_LG_F16: u16 = 221;
    /// Set the vector condition code to 1 iff the first input is greater than or equal to the second input. Store the result...
    pub const V_CMPX_GE_F16: u16 = 222;
    /// Set the vector condition code to 1 iff the first input is orderable to the second input. Store the result into the EX...
    pub const V_CMPX_O_F16: u16 = 223;
    /// Set the vector condition code to 0. Store the result into VCC or a scalar register.
    pub const V_CMP_F_U64: u16 = 224;
    /// Set the vector condition code to 1 iff the first input is less than the second input. Store the result into VCC or a ...
    pub const V_CMP_LT_U64: u16 = 225;
    /// Set the vector condition code to 1 iff the first input is equal to the second input. Store the result into VCC or a s...
    pub const V_CMP_EQ_U64: u16 = 226;
    /// Set the vector condition code to 1 iff the first input is less than or equal to the second input. Store the result in...
    pub const V_CMP_LE_U64: u16 = 227;
    /// Set the vector condition code to 1 iff the first input is greater than the second input. Store the result into VCC or...
    pub const V_CMP_GT_U64: u16 = 228;
    /// Set the vector condition code to 1 iff the first input is not equal to the second input. Store the result into VCC or...
    pub const V_CMP_NE_U64: u16 = 229;
    /// Set the vector condition code to 1 iff the first input is greater than or equal to the second input. Store the result...
    pub const V_CMP_GE_U64: u16 = 230;
    /// Set the vector condition code to 1. Store the result into VCC or a scalar register.
    pub const V_CMP_T_U64: u16 = 231;
    /// Set the vector condition code to 1 iff the first input is not orderable to the second input. Store the result into VC...
    pub const V_CMP_U_F16: u16 = 232;
    /// Set the vector condition code to 1 iff the first input is not greater than or equal to the second input. Store the re...
    pub const V_CMP_NGE_F16: u16 = 233;
    /// Set the vector condition code to 1 iff the first input is not less than or greater than the second input. Store the r...
    pub const V_CMP_NLG_F16: u16 = 234;
    /// Set the vector condition code to 1 iff the first input is not greater than the second input. Store the result into VC...
    pub const V_CMP_NGT_F16: u16 = 235;
    /// Set the vector condition code to 1 iff the first input is not less than or equal to the second input. Store the resul...
    pub const V_CMP_NLE_F16: u16 = 236;
    /// Set the vector condition code to 1 iff the first input is not equal to the second input. Store the result into VCC or...
    pub const V_CMP_NEQ_F16: u16 = 237;
    /// Set the vector condition code to 1 iff the first input is not less than the second input. Store the result into VCC o...
    pub const V_CMP_NLT_F16: u16 = 238;
    /// Set the vector condition code to 1. Store the result into VCC or a scalar register.
    pub const V_CMP_TRU_F16: u16 = 239;
    /// Set the vector condition code to 0. Store the result into the EXEC mask.
    pub const V_CMPX_F_U64: u16 = 240;
    /// Set the vector condition code to 1 iff the first input is less than the second input. Store the result into the EXEC ...
    pub const V_CMPX_LT_U64: u16 = 241;
    /// Set the vector condition code to 1 iff the first input is equal to the second input. Store the result into the EXEC m...
    pub const V_CMPX_EQ_U64: u16 = 242;
    /// Set the vector condition code to 1 iff the first input is less than or equal to the second input. Store the result in...
    pub const V_CMPX_LE_U64: u16 = 243;
    /// Set the vector condition code to 1 iff the first input is greater than the second input. Store the result into the EX...
    pub const V_CMPX_GT_U64: u16 = 244;
    /// Set the vector condition code to 1 iff the first input is not equal to the second input. Store the result into the EX...
    pub const V_CMPX_NE_U64: u16 = 245;
    /// Set the vector condition code to 1 iff the first input is greater than or equal to the second input. Store the result...
    pub const V_CMPX_GE_U64: u16 = 246;
    /// Set the vector condition code to 1. Store the result into the EXEC mask.
    pub const V_CMPX_T_U64: u16 = 247;
    /// Set the vector condition code to 1 iff the first input is not orderable to the second input. Store the result into th...
    pub const V_CMPX_U_F16: u16 = 248;
    /// Set the vector condition code to 1 iff the first input is not greater than or equal to the second input. Store the re...
    pub const V_CMPX_NGE_F16: u16 = 249;
    /// Set the vector condition code to 1 iff the first input is not less than or greater than the second input. Store the r...
    pub const V_CMPX_NLG_F16: u16 = 250;
    /// Set the vector condition code to 1 iff the first input is not greater than the second input. Store the result into th...
    pub const V_CMPX_NGT_F16: u16 = 251;
    /// Set the vector condition code to 1 iff the first input is not less than or equal to the second input. Store the resul...
    pub const V_CMPX_NLE_F16: u16 = 252;
    /// Set the vector condition code to 1 iff the first input is not equal to the second input. Store the result into the EX...
    pub const V_CMPX_NEQ_F16: u16 = 253;
    /// Set the vector condition code to 1 iff the first input is not less than the second input. Store the result into the E...
    pub const V_CMPX_NLT_F16: u16 = 254;
    /// Set the vector condition code to 1. Store the result into the EXEC mask.
    pub const V_CMPX_TRU_F16: u16 = 255;
    /// Copy data from one of two inputs based on the vector condition code and store the result into a vector register.
    pub const V_CNDMASK_B32: u16 = 257;
    /// Add two floating point inputs and store the result into a vector register.
    pub const V_ADD_F32: u16 = 259;
    /// Subtract the second floating point input from the first input and store the result into a vector register.
    pub const V_SUB_F32: u16 = 260;
    /// Subtract the first floating point input from the second input and store the result into a vector register.
    pub const V_SUBREV_F32: u16 = 261;
    /// Multiply two single-precision values and accumulate the result with the destination. Follows DX9 rules where 0.0 time...
    pub const V_FMAC_LEGACY_F32: u16 = 262;
    /// Multiply two floating point inputs and store the result in a vector register. Follows DX9 rules where 0.0 times anyth...
    pub const V_MUL_LEGACY_F32: u16 = 263;
    /// Multiply two floating point inputs and store the result into a vector register.
    pub const V_MUL_F32: u16 = 264;
    /// Multiply two signed 24-bit integer inputs and store the result as a signed 32-bit integer into a vector register.
    pub const V_MUL_I32_I24: u16 = 265;
    /// Multiply two signed 24-bit integer inputs and store the high 32 bits of the result as a signed 32-bit integer into a ...
    pub const V_MUL_HI_I32_I24: u16 = 266;
    /// Multiply two unsigned 24-bit integer inputs and store the result as an unsigned 32-bit integer into a vector register.
    pub const V_MUL_U32_U24: u16 = 267;
    /// Multiply two unsigned 24-bit integer inputs and store the high 32 bits of the result as an unsigned 32-bit integer in...
    pub const V_MUL_HI_U32_U24: u16 = 268;
    /// Select the minimum of two single-precision float inputs and store the result into a vector register.
    pub const V_MIN_F32: u16 = 271;
    /// Select the maximum of two single-precision float inputs and store the result into a vector register.
    pub const V_MAX_F32: u16 = 272;
    /// Select the minimum of two signed 32-bit integer inputs and store the selected value into a vector register.
    pub const V_MIN_I32: u16 = 273;
    /// Select the maximum of two signed 32-bit integer inputs and store the selected value into a vector register.
    pub const V_MAX_I32: u16 = 274;
    /// Select the minimum of two unsigned 32-bit integer inputs and store the selected value into a vector register.
    pub const V_MIN_U32: u16 = 275;
    /// Select the maximum of two unsigned 32-bit integer inputs and store the selected value into a vector register.
    pub const V_MAX_U32: u16 = 276;
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
    /// Add two unsigned 32-bit integer inputs and store the result into a vector register. No carry-in or carry-out support.
    pub const V_ADD_NC_U32: u16 = 293;
    /// Subtract the second unsigned input from the first input and store the result into a vector register. No carry-in or c...
    pub const V_SUB_NC_U32: u16 = 294;
    /// Subtract the first unsigned input from the second input and store the result into a vector register. No carry-in or c...
    pub const V_SUBREV_NC_U32: u16 = 295;
    /// Multiply two floating point inputs and accumulate the result into the destination register using fused multiply add.
    pub const V_FMAC_F32: u16 = 299;
    /// Convert two single-precision float inputs to a packed half-precision float value using round toward zero semantics (i...
    pub const V_CVT_PKRTZ_F16_F32: u16 = 303;
    /// Add two floating point inputs and store the result into a vector register.
    pub const V_ADD_F16: u16 = 306;
    /// Subtract the second floating point input from the first input and store the result into a vector register.
    pub const V_SUB_F16: u16 = 307;
    /// Subtract the first floating point input from the second input and store the result into a vector register.
    pub const V_SUBREV_F16: u16 = 308;
    /// Multiply two floating point inputs and store the result into a vector register.
    pub const V_MUL_F16: u16 = 309;
    /// Multiply two floating point inputs and accumulate the result into the destination register using fused multiply add.
    pub const V_FMAC_F16: u16 = 310;
    /// Select the maximum of two half-precision float inputs and store the result into a vector register.
    pub const V_MAX_F16: u16 = 313;
    /// Select the minimum of two half-precision float inputs and store the result into a vector register.
    pub const V_MIN_F16: u16 = 314;
    /// Multiply the first input, a floating point value, by an integral power of 2 specified in the second input, a signed i...
    pub const V_LDEXP_F16: u16 = 315;
    /// Multiply and add single-precision values. Follows DX9 rules where 0.0 times anything produces 0.0.
    pub const V_FMA_LEGACY_F32: u16 = 320;
    /// Multiply two signed 24-bit integer inputs in the signed 32-bit integer domain, add a signed 32-bit integer value from...
    pub const V_MAD_I32_I24: u16 = 322;
    /// Multiply two unsigned 24-bit integer inputs in the unsigned 32-bit integer domain, add a unsigned 32-bit integer valu...
    pub const V_MAD_U32_U24: u16 = 323;
    /// Compute the cubemap face ID of a 3D coordinate specified as three single-precision float inputs. Store the result in ...
    pub const V_CUBEID_F32: u16 = 324;
    /// Compute the cubemap S coordinate of a 3D coordinate specified as three single-precision float inputs. Store the resul...
    pub const V_CUBESC_F32: u16 = 325;
    /// Compute the cubemap T coordinate of a 3D coordinate specified as three single-precision float inputs. Store the resul...
    pub const V_CUBETC_F32: u16 = 326;
    /// Compute the cubemap major axis coordinate of a 3D coordinate specified as three single-precision float inputs. Store ...
    pub const V_CUBEMA_F32: u16 = 327;
    /// Extract an unsigned bitfield from the first input using field offset from the second input and size from the third in...
    pub const V_BFE_U32: u16 = 328;
    /// Extract a signed bitfield from the first input using field offset from the second input and size from the third input...
    pub const V_BFE_I32: u16 = 329;
    /// Overwrite a bitfield in the third input with a bitfield from the second input using a mask from the first input, then...
    pub const V_BFI_B32: u16 = 330;
    /// Multiply two single-precision float inputs and add a third input using fused multiply add, and store the result into ...
    pub const V_FMA_F32: u16 = 331;
    /// Multiply two double-precision float inputs and add a third input using fused multiply add, and store the result into ...
    pub const V_FMA_F64: u16 = 332;
    /// Average two 4-D vectors stored as packed bytes in the first two inputs with rounding control provided by the third in...
    pub const V_LERP_U8: u16 = 333;
    /// Align a 64-bit value encoded in the first two inputs to a bit position specified in the third input, then store the r...
    pub const V_ALIGNBIT_B32: u16 = 334;
    /// Align a 64-bit value encoded in the first two inputs to a byte position specified in the third input, then store the ...
    pub const V_ALIGNBYTE_B32: u16 = 335;
    /// Multiply two floating point inputs and store the result into a vector register. Specific rules apply to accommodate l...
    pub const V_MULLIT_F32: u16 = 336;
    /// Select the minimum of three single-precision float inputs and store the selected value into a vector register.
    pub const V_MIN3_F32: u16 = 337;
    /// Select the minimum of three signed 32-bit integer inputs and store the selected value into a vector register.
    pub const V_MIN3_I32: u16 = 338;
    /// Select the minimum of three unsigned 32-bit integer inputs and store the selected value into a vector register.
    pub const V_MIN3_U32: u16 = 339;
    /// Select the maximum of three single-precision float inputs and store the selected value into a vector register.
    pub const V_MAX3_F32: u16 = 340;
    /// Select the maximum of three signed 32-bit integer inputs and store the selected value into a vector register.
    pub const V_MAX3_I32: u16 = 341;
    /// Select the maximum of three unsigned 32-bit integer inputs and store the selected value into a vector register.
    pub const V_MAX3_U32: u16 = 342;
    /// Select the median of three single-precision float values and store the selected value into a vector register.
    pub const V_MED3_F32: u16 = 343;
    /// Select the median of three signed 32-bit integer values and store the selected value into a vector register.
    pub const V_MED3_I32: u16 = 344;
    /// Select the median of three unsigned 32-bit integer values and store the selected value into a vector register.
    pub const V_MED3_U32: u16 = 345;
    /// Calculate the sum of absolute differences of elements in two packed 4-component unsigned 8-bit integer inputs, add an...
    pub const V_SAD_U8: u16 = 346;
    /// Calculate the sum of absolute differences of elements in two packed 4-component unsigned 8-bit integer inputs, shift ...
    pub const V_SAD_HI_U8: u16 = 347;
    /// Calculate the sum of absolute differences of elements in two packed 2-component unsigned 16-bit integer inputs, add a...
    pub const V_SAD_U16: u16 = 348;
    /// Calculate the absolute difference of two unsigned 32-bit integer inputs, add an unsigned 32-bit integer value from th...
    pub const V_SAD_U32: u16 = 349;
    /// Convert a single-precision float value from the first input to an unsigned 8-bit integer value and pack the result in...
    pub const V_CVT_PK_U8_F32: u16 = 350;
    /// Given a single-precision float quotient in the first input, a denominator in the second input and a numerator in the ...
    pub const V_DIV_FIXUP_F32: u16 = 351;
    /// Given a double-precision float quotient in the first input, a denominator in the second input and a numerator in the ...
    pub const V_DIV_FIXUP_F64: u16 = 352;
    /// Add two floating point inputs and store the result into a vector register.
    pub const V_ADD_F64: u16 = 356;
    /// Multiply two floating point inputs and store the result into a vector register.
    pub const V_MUL_F64: u16 = 357;
    /// Select the minimum of two double-precision float inputs and store the result into a vector register.
    pub const V_MIN_F64: u16 = 358;
    /// Select the maximum of two double-precision float inputs and store the result into a vector register.
    pub const V_MAX_F64: u16 = 359;
    /// Multiply the first input, a floating point value, by an integral power of 2 specified in the second input, a signed i...
    pub const V_LDEXP_F64: u16 = 360;
    /// Multiply two unsigned 32-bit integer inputs and store the result into a vector register.
    pub const V_MUL_LO_U32: u16 = 361;
    /// Multiply two unsigned 32-bit integer inputs and store the high 32 bits of the result into a vector register.
    pub const V_MUL_HI_U32: u16 = 362;
    /// Multiply two signed 32-bit integer inputs and store the high 32 bits of the result into a vector register.
    pub const V_MUL_HI_I32: u16 = 364;
    /// Multiply two single-precision float inputs and add a third input using fused multiply add, then scale the exponent of...
    pub const V_DIV_FMAS_F32: u16 = 367;
    /// Multiply two double-precision float inputs and add a third input using fused multiply add, then scale the exponent of...
    pub const V_DIV_FMAS_F64: u16 = 368;
    /// Calculate the sum of absolute differences of elements in two packed 4-component unsigned 8-bit integer inputs, except...
    pub const V_MSAD_U8: u16 = 369;
    /// Perform the V_SAD_U8 operation four times using different slices of the first array, all entries of the second array ...
    pub const V_QSAD_PK_U16_U8: u16 = 370;
    /// Perform the V_MSAD_U8 operation four times using different slices of the first array, all entries of the second array...
    pub const V_MQSAD_PK_U16_U8: u16 = 371;
    /// Look up a 53-bit segment of 2/PI using an integer segment select in the second input. Scale the intermediate result b...
    pub const V_TRIG_PREOP_F64: u16 = 372;
    /// Perform the V_MSAD_U8 operation four times using different slices of the first array, all entries of the second array...
    pub const V_MQSAD_U32_U8: u16 = 373;
    /// Calculate the bitwise XOR of three vector inputs and store the result into a vector register.
    pub const V_XOR3_B32: u16 = 376;
    /// Do nothing.
    pub const V_NOP: u16 = 384;
    /// Move data from a vector input into a vector register.
    pub const V_MOV_B32: u16 = 385;
    /// Read the scalar value in the lowest active lane of the input vector register and store it into a scalar register.
    pub const V_READFIRSTLANE_B32: u16 = 386;
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
    /// Flush the vector ALU pipeline through the destination cache.
    pub const V_PIPEFLUSH: u16 = 411;
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
    /// Clear this wave's exception state in the vector ALU.
    pub const V_CLREXCP: u16 = 449;
    /// Move data from a vector input into a relatively-indexed vector register.
    pub const V_MOVRELD_B32: u16 = 450;
    /// Move data from a relatively-indexed vector register into another vector register.
    pub const V_MOVRELS_B32: u16 = 451;
    /// Move data from a relatively-indexed vector register into another relatively-indexed vector register.
    pub const V_MOVRELSD_B32: u16 = 452;
    /// Move data from a relatively-indexed vector register into another relatively-indexed vector register, using different ...
    pub const V_MOVRELSD_2_B32: u16 = 456;
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
    /// Given two 16-bit unsigned integer inputs, saturate each input over an 8-bit unsigned range, pack the resulting values...
    pub const V_SAT_PK_U8_I16: u16 = 482;
    /// Convert from a half-precision float input to a signed normalized short and store the result into a vector register.
    pub const V_CVT_NORM_I16_F16: u16 = 483;
    /// Convert from a half-precision float input to an unsigned normalized short and store the result into a vector register.
    pub const V_CVT_NORM_U16_F16: u16 = 484;
    /// Given the I coordinate in a vector register and an attribute specifier, load parameter data from the local data share...
    pub const V_INTERP_P1_F32: u16 = 512;
    /// Given the J coordinate in a vector register, an attribute specifier and the result of a prior V_INTERP_P1_F32 in the ...
    pub const V_INTERP_P2_F32: u16 = 513;
    /// Given an attribute specifier and a parameter ID (P0, P10 or P20), load one of the parameter values from the local dat...
    pub const V_INTERP_MOV_F32: u16 = 514;
    /// Given a shift count in the first vector input, calculate the logical shift left of the second vector input and store ...
    pub const V_LSHLREV_B64: u16 = 767;
    /// Given a shift count in the first vector input, calculate the logical shift right of the second vector input and store...
    pub const V_LSHRREV_B64: u16 = 768;
    /// Given a shift count in the first vector input, calculate the arithmetic shift right (preserving sign bit) of the seco...
    pub const V_ASHRREV_I64: u16 = 769;
    /// Add two unsigned 16-bit integer inputs and store the result into a vector register. No carry-in or carry-out support.
    pub const V_ADD_NC_U16: u16 = 771;
    /// Subtract the second unsigned input from the first input and store the result into a vector register. No carry-in or c...
    pub const V_SUB_NC_U16: u16 = 772;
    /// Multiply two unsigned 16-bit integer inputs and store the low bits of the result into a vector register.
    pub const V_MUL_LO_U16: u16 = 773;
    /// Given a shift count in the first vector input, calculate the logical shift right of the second vector input and store...
    pub const V_LSHRREV_B16: u16 = 775;
    /// Given a shift count in the first vector input, calculate the arithmetic shift right (preserving sign bit) of the seco...
    pub const V_ASHRREV_I16: u16 = 776;
    /// Select the maximum of two unsigned 16-bit integer inputs and store the selected value into a vector register.
    pub const V_MAX_U16: u16 = 777;
    /// Select the maximum of two signed 16-bit integer inputs and store the selected value into a vector register.
    pub const V_MAX_I16: u16 = 778;
    /// Select the minimum of two unsigned 16-bit integer inputs and store the selected value into a vector register.
    pub const V_MIN_U16: u16 = 779;
    /// Select the minimum of two signed 16-bit integer inputs and store the selected value into a vector register.
    pub const V_MIN_I16: u16 = 780;
    /// Add two signed 16-bit integer inputs and store the result into a vector register. No carry-in or carry-out support.
    pub const V_ADD_NC_I16: u16 = 781;
    /// Subtract the second signed input from the first input and store the result into a vector register. No carry-in or car...
    pub const V_SUB_NC_I16: u16 = 782;
    /// Pack two half-precision float values into a single 32-bit value and store the result into a vector register.
    pub const V_PACK_B32_F16: u16 = 785;
    /// Convert from two half-precision float inputs to a packed signed normalized short and store the result into a vector r...
    pub const V_CVT_PKNORM_I16_F16: u16 = 786;
    /// Convert from two half-precision float inputs to a packed unsigned normalized short and store the result into a vector...
    pub const V_CVT_PKNORM_U16_F16: u16 = 787;
    /// Given a shift count in the first vector input, calculate the logical shift left of the second vector input and store ...
    pub const V_LSHLREV_B16: u16 = 788;
    /// Multiply two unsigned 16-bit integer inputs, add an unsigned 16-bit integer value from a third input, and store the r...
    pub const V_MAD_U16: u16 = 832;
    /// Given a single-precision float I coordinate in a vector register and an attribute specifier, load two half-precision ...
    pub const V_INTERP_P1LL_F16: u16 = 834;
    /// Given a single-precision float I coordinate in a vector register, a half-precision float P0 value in another vector r...
    pub const V_INTERP_P1LV_F16: u16 = 835;
    /// Permute a 64-bit value constructed from two vector inputs (most significant bits come from the first input) using a p...
    pub const V_PERM_B32: u16 = 836;
    /// Calculate bitwise XOR of the first two vector inputs, then add the third vector input to the intermediate result, the...
    pub const V_XAD_U32: u16 = 837;
    /// Given a shift count in the second input, calculate the logical shift left of the first input, then add the third inpu...
    pub const V_LSHL_ADD_U32: u16 = 838;
    /// Add the first two integer inputs, then given a shift count in the third input, calculate the logical shift left of th...
    pub const V_ADD_LSHL_U32: u16 = 839;
    /// Multiply two half-precision float inputs and add a third input using fused multiply add, and store the result into a ...
    pub const V_FMA_F16: u16 = 843;
    /// Select the minimum of three half-precision float inputs and store the selected value into a vector register.
    pub const V_MIN3_F16: u16 = 849;
    /// Select the minimum of three signed 16-bit integer inputs and store the selected value into a vector register.
    pub const V_MIN3_I16: u16 = 850;
    /// Select the minimum of three unsigned 16-bit integer inputs and store the selected value into a vector register.
    pub const V_MIN3_U16: u16 = 851;
    /// Select the maximum of three half-precision float inputs and store the selected value into a vector register.
    pub const V_MAX3_F16: u16 = 852;
    /// Select the maximum of three signed 16-bit integer inputs and store the selected value into a vector register.
    pub const V_MAX3_I16: u16 = 853;
    /// Select the maximum of three unsigned 16-bit integer inputs and store the selected value into a vector register.
    pub const V_MAX3_U16: u16 = 854;
    /// Select the median of three half-precision float values and store the selected value into a vector register.
    pub const V_MED3_F16: u16 = 855;
    /// Select the median of three signed 16-bit integer values and store the selected value into a vector register.
    pub const V_MED3_I16: u16 = 856;
    /// Select the median of three unsigned 16-bit integer values and store the selected value into a vector register.
    pub const V_MED3_U16: u16 = 857;
    /// Given a single-precision float J coordinate in a vector register, an attribute specifier and the result of a prior V_...
    pub const V_INTERP_P2_F16: u16 = 858;
    /// Multiply two signed 16-bit integer inputs, add a signed 16-bit integer value from a third input, and store the result...
    pub const V_MAD_I16: u16 = 862;
    /// Given a half-precision float quotient in the first input, a denominator in the second input and a numerator in the th...
    pub const V_DIV_FIXUP_F16: u16 = 863;
    /// Read the scalar value in the specified lane of the first input where the lane select is in the second input. Store th...
    pub const V_READLANE_B32: u16 = 864;
    /// Write the scalar value in the first input into the specified lane of a vector register where the lane select is in th...
    pub const V_WRITELANE_B32: u16 = 865;
    /// Multiply the first input, a floating point value, by an integral power of 2 specified in the second input, a signed i...
    pub const V_LDEXP_F32: u16 = 866;
    /// Calculate a bitfield mask given a field offset and size and store the result into a vector register.
    pub const V_BFM_B32: u16 = 867;
    /// Count the number of \"1\" bits in the vector input and store the result into a vector register.
    pub const V_BCNT_U32_B32: u16 = 868;
    /// For each lane 0 <= N < 32, examine the N least significant bits of the first input and count how many of those bits a...
    pub const V_MBCNT_LO_U32_B32: u16 = 869;
    /// For each lane 32 <= N < 64, examine the N least significant bits of the first input and count how many of those bits ...
    pub const V_MBCNT_HI_U32_B32: u16 = 870;
    /// Convert from two single-precision float inputs to a packed signed normalized short and store the result into a vector...
    pub const V_CVT_PKNORM_I16_F32: u16 = 872;
    /// Convert from two single-precision float inputs to a packed unsigned normalized short and store the result into a vect...
    pub const V_CVT_PKNORM_U16_F32: u16 = 873;
    /// Convert from two unsigned 32-bit integer inputs to a packed unsigned 16-bit integer value and store the result into a...
    pub const V_CVT_PK_U16_U32: u16 = 874;
    /// Convert from two signed 32-bit integer inputs to a packed signed 16-bit integer value and store the result into a vec...
    pub const V_CVT_PK_I16_I32: u16 = 875;
    /// Add three unsigned inputs and store the result into a vector register. No carry-in or carry-out support.
    pub const V_ADD3_U32: u16 = 877;
    /// Given a shift count in the second input, calculate the logical shift left of the first input, then calculate the bitw...
    pub const V_LSHL_OR_B32: u16 = 879;
    /// Calculate bitwise AND on the first two vector inputs, then compute the bitwise OR of the intermediate result and the ...
    pub const V_AND_OR_B32: u16 = 881;
    /// Calculate the bitwise OR of three vector inputs and store the result into a vector register.
    pub const V_OR3_B32: u16 = 882;
    /// Multiply two unsigned 16-bit integer inputs in the unsigned 32-bit integer domain, add an unsigned 32-bit integer val...
    pub const V_MAD_U32_U16: u16 = 883;
    /// Multiply two signed 16-bit integer inputs in the signed 32-bit integer domain, add a signed 32-bit integer value from...
    pub const V_MAD_I32_I16: u16 = 885;
    /// Subtract the second signed input from the first input and store the result into a vector register. No carry-in or car...
    pub const V_SUB_NC_I32: u16 = 886;
    /// Perform arbitrary gather-style operation within a row (16 contiguous lanes).
    pub const V_PERMLANE16_B32: u16 = 887;
    /// Perform arbitrary gather-style operation across two rows (each row is 16 contiguous lanes).
    pub const V_PERMLANEX16_B32: u16 = 888;
    /// Add two signed 32-bit integer inputs and store the result into a vector register. No carry-in or carry-out support.
    pub const V_ADD_NC_I32: u16 = 895;

    /// All ENC_VOP3 instructions.
    pub const TABLE: &[super::InstrEntry] = &[
        super::InstrEntry {
            name: "V_CMP_F_F32",
            opcode: 0,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_LT_F32",
            opcode: 1,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_EQ_F32",
            opcode: 2,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_LE_F32",
            opcode: 3,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_GT_F32",
            opcode: 4,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_LG_F32",
            opcode: 5,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_GE_F32",
            opcode: 6,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_O_F32",
            opcode: 7,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_U_F32",
            opcode: 8,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_NGE_F32",
            opcode: 9,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_NLG_F32",
            opcode: 10,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_NGT_F32",
            opcode: 11,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_NLE_F32",
            opcode: 12,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_NEQ_F32",
            opcode: 13,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_NLT_F32",
            opcode: 14,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_TRU_F32",
            opcode: 15,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_F_F32",
            opcode: 16,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_LT_F32",
            opcode: 17,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_EQ_F32",
            opcode: 18,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_LE_F32",
            opcode: 19,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_GT_F32",
            opcode: 20,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_LG_F32",
            opcode: 21,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_GE_F32",
            opcode: 22,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_O_F32",
            opcode: 23,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_U_F32",
            opcode: 24,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_NGE_F32",
            opcode: 25,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_NLG_F32",
            opcode: 26,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_NGT_F32",
            opcode: 27,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_NLE_F32",
            opcode: 28,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_NEQ_F32",
            opcode: 29,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_NLT_F32",
            opcode: 30,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_TRU_F32",
            opcode: 31,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_F_F64",
            opcode: 32,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_LT_F64",
            opcode: 33,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_EQ_F64",
            opcode: 34,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_LE_F64",
            opcode: 35,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_GT_F64",
            opcode: 36,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_LG_F64",
            opcode: 37,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_GE_F64",
            opcode: 38,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_O_F64",
            opcode: 39,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_U_F64",
            opcode: 40,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_NGE_F64",
            opcode: 41,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_NLG_F64",
            opcode: 42,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_NGT_F64",
            opcode: 43,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_NLE_F64",
            opcode: 44,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_NEQ_F64",
            opcode: 45,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_NLT_F64",
            opcode: 46,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_TRU_F64",
            opcode: 47,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_F_F64",
            opcode: 48,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_LT_F64",
            opcode: 49,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_EQ_F64",
            opcode: 50,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_LE_F64",
            opcode: 51,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_GT_F64",
            opcode: 52,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_LG_F64",
            opcode: 53,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_GE_F64",
            opcode: 54,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_O_F64",
            opcode: 55,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_U_F64",
            opcode: 56,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_NGE_F64",
            opcode: 57,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_NLG_F64",
            opcode: 58,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_NGT_F64",
            opcode: 59,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_NLE_F64",
            opcode: 60,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_NEQ_F64",
            opcode: 61,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_NLT_F64",
            opcode: 62,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_TRU_F64",
            opcode: 63,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_F_I32",
            opcode: 128,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_LT_I32",
            opcode: 129,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_EQ_I32",
            opcode: 130,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_LE_I32",
            opcode: 131,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_GT_I32",
            opcode: 132,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_NE_I32",
            opcode: 133,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_GE_I32",
            opcode: 134,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_T_I32",
            opcode: 135,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_CLASS_F32",
            opcode: 136,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_LT_I16",
            opcode: 137,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_EQ_I16",
            opcode: 138,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_LE_I16",
            opcode: 139,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_GT_I16",
            opcode: 140,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_NE_I16",
            opcode: 141,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_GE_I16",
            opcode: 142,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_CLASS_F16",
            opcode: 143,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_F_I32",
            opcode: 144,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_LT_I32",
            opcode: 145,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_EQ_I32",
            opcode: 146,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_LE_I32",
            opcode: 147,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_GT_I32",
            opcode: 148,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_NE_I32",
            opcode: 149,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_GE_I32",
            opcode: 150,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_T_I32",
            opcode: 151,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_CLASS_F32",
            opcode: 152,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_LT_I16",
            opcode: 153,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_EQ_I16",
            opcode: 154,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_LE_I16",
            opcode: 155,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_GT_I16",
            opcode: 156,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_NE_I16",
            opcode: 157,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_GE_I16",
            opcode: 158,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_CLASS_F16",
            opcode: 159,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_F_I64",
            opcode: 160,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_LT_I64",
            opcode: 161,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_EQ_I64",
            opcode: 162,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_LE_I64",
            opcode: 163,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_GT_I64",
            opcode: 164,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_NE_I64",
            opcode: 165,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_GE_I64",
            opcode: 166,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_T_I64",
            opcode: 167,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_CLASS_F64",
            opcode: 168,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_LT_U16",
            opcode: 169,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_EQ_U16",
            opcode: 170,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_LE_U16",
            opcode: 171,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_GT_U16",
            opcode: 172,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_NE_U16",
            opcode: 173,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_GE_U16",
            opcode: 174,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_F_I64",
            opcode: 176,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_LT_I64",
            opcode: 177,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_EQ_I64",
            opcode: 178,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_LE_I64",
            opcode: 179,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_GT_I64",
            opcode: 180,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_NE_I64",
            opcode: 181,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_GE_I64",
            opcode: 182,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_T_I64",
            opcode: 183,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_CLASS_F64",
            opcode: 184,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_LT_U16",
            opcode: 185,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_EQ_U16",
            opcode: 186,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_LE_U16",
            opcode: 187,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_GT_U16",
            opcode: 188,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_NE_U16",
            opcode: 189,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_GE_U16",
            opcode: 190,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_F_U32",
            opcode: 192,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_LT_U32",
            opcode: 193,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_EQ_U32",
            opcode: 194,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_LE_U32",
            opcode: 195,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_GT_U32",
            opcode: 196,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_NE_U32",
            opcode: 197,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_GE_U32",
            opcode: 198,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_T_U32",
            opcode: 199,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_F_F16",
            opcode: 200,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_LT_F16",
            opcode: 201,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_EQ_F16",
            opcode: 202,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_LE_F16",
            opcode: 203,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_GT_F16",
            opcode: 204,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_LG_F16",
            opcode: 205,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_GE_F16",
            opcode: 206,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_O_F16",
            opcode: 207,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_F_U32",
            opcode: 208,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_LT_U32",
            opcode: 209,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_EQ_U32",
            opcode: 210,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_LE_U32",
            opcode: 211,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_GT_U32",
            opcode: 212,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_NE_U32",
            opcode: 213,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_GE_U32",
            opcode: 214,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_T_U32",
            opcode: 215,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_F_F16",
            opcode: 216,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_LT_F16",
            opcode: 217,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_EQ_F16",
            opcode: 218,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_LE_F16",
            opcode: 219,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_GT_F16",
            opcode: 220,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_LG_F16",
            opcode: 221,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_GE_F16",
            opcode: 222,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_O_F16",
            opcode: 223,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_F_U64",
            opcode: 224,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_LT_U64",
            opcode: 225,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_EQ_U64",
            opcode: 226,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_LE_U64",
            opcode: 227,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_GT_U64",
            opcode: 228,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_NE_U64",
            opcode: 229,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_GE_U64",
            opcode: 230,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_T_U64",
            opcode: 231,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_U_F16",
            opcode: 232,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_NGE_F16",
            opcode: 233,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_NLG_F16",
            opcode: 234,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_NGT_F16",
            opcode: 235,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_NLE_F16",
            opcode: 236,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_NEQ_F16",
            opcode: 237,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_NLT_F16",
            opcode: 238,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_TRU_F16",
            opcode: 239,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_F_U64",
            opcode: 240,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_LT_U64",
            opcode: 241,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_EQ_U64",
            opcode: 242,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_LE_U64",
            opcode: 243,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_GT_U64",
            opcode: 244,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_NE_U64",
            opcode: 245,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_GE_U64",
            opcode: 246,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_T_U64",
            opcode: 247,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_U_F16",
            opcode: 248,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_NGE_F16",
            opcode: 249,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_NLG_F16",
            opcode: 250,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_NGT_F16",
            opcode: 251,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_NLE_F16",
            opcode: 252,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_NEQ_F16",
            opcode: 253,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_NLT_F16",
            opcode: 254,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_TRU_F16",
            opcode: 255,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CNDMASK_B32",
            opcode: 257,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_ADD_F32",
            opcode: 259,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_SUB_F32",
            opcode: 260,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_SUBREV_F32",
            opcode: 261,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_FMAC_LEGACY_F32",
            opcode: 262,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_MUL_LEGACY_F32",
            opcode: 263,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_MUL_F32",
            opcode: 264,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_MUL_I32_I24",
            opcode: 265,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_MUL_HI_I32_I24",
            opcode: 266,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_MUL_U32_U24",
            opcode: 267,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_MUL_HI_U32_U24",
            opcode: 268,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_MIN_F32",
            opcode: 271,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_MAX_F32",
            opcode: 272,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_MIN_I32",
            opcode: 273,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_MAX_I32",
            opcode: 274,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_MIN_U32",
            opcode: 275,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_MAX_U32",
            opcode: 276,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_LSHRREV_B32",
            opcode: 278,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_ASHRREV_I32",
            opcode: 280,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_LSHLREV_B32",
            opcode: 282,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_AND_B32",
            opcode: 283,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_OR_B32",
            opcode: 284,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_XOR_B32",
            opcode: 285,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_XNOR_B32",
            opcode: 286,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_ADD_NC_U32",
            opcode: 293,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_SUB_NC_U32",
            opcode: 294,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_SUBREV_NC_U32",
            opcode: 295,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_FMAC_F32",
            opcode: 299,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CVT_PKRTZ_F16_F32",
            opcode: 303,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_ADD_F16",
            opcode: 306,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_SUB_F16",
            opcode: 307,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_SUBREV_F16",
            opcode: 308,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_MUL_F16",
            opcode: 309,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_FMAC_F16",
            opcode: 310,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_MAX_F16",
            opcode: 313,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_MIN_F16",
            opcode: 314,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_LDEXP_F16",
            opcode: 315,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_FMA_LEGACY_F32",
            opcode: 320,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_MAD_I32_I24",
            opcode: 322,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_MAD_U32_U24",
            opcode: 323,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CUBEID_F32",
            opcode: 324,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CUBESC_F32",
            opcode: 325,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CUBETC_F32",
            opcode: 326,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CUBEMA_F32",
            opcode: 327,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_BFE_U32",
            opcode: 328,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_BFE_I32",
            opcode: 329,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_BFI_B32",
            opcode: 330,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_FMA_F32",
            opcode: 331,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_FMA_F64",
            opcode: 332,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_LERP_U8",
            opcode: 333,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_ALIGNBIT_B32",
            opcode: 334,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_ALIGNBYTE_B32",
            opcode: 335,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_MULLIT_F32",
            opcode: 336,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_MIN3_F32",
            opcode: 337,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_MIN3_I32",
            opcode: 338,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_MIN3_U32",
            opcode: 339,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_MAX3_F32",
            opcode: 340,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_MAX3_I32",
            opcode: 341,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_MAX3_U32",
            opcode: 342,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_MED3_F32",
            opcode: 343,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_MED3_I32",
            opcode: 344,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_MED3_U32",
            opcode: 345,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_SAD_U8",
            opcode: 346,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_SAD_HI_U8",
            opcode: 347,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_SAD_U16",
            opcode: 348,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_SAD_U32",
            opcode: 349,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CVT_PK_U8_F32",
            opcode: 350,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_DIV_FIXUP_F32",
            opcode: 351,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_DIV_FIXUP_F64",
            opcode: 352,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_ADD_F64",
            opcode: 356,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_MUL_F64",
            opcode: 357,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_MIN_F64",
            opcode: 358,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_MAX_F64",
            opcode: 359,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_LDEXP_F64",
            opcode: 360,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_MUL_LO_U32",
            opcode: 361,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_MUL_HI_U32",
            opcode: 362,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_MUL_HI_I32",
            opcode: 364,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_DIV_FMAS_F32",
            opcode: 367,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_DIV_FMAS_F64",
            opcode: 368,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_MSAD_U8",
            opcode: 369,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_QSAD_PK_U16_U8",
            opcode: 370,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_MQSAD_PK_U16_U8",
            opcode: 371,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_TRIG_PREOP_F64",
            opcode: 372,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_MQSAD_U32_U8",
            opcode: 373,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_XOR3_B32",
            opcode: 376,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_NOP",
            opcode: 384,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_MOV_B32",
            opcode: 385,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_READFIRSTLANE_B32",
            opcode: 386,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CVT_I32_F64",
            opcode: 387,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CVT_F64_I32",
            opcode: 388,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CVT_F32_I32",
            opcode: 389,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CVT_F32_U32",
            opcode: 390,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CVT_U32_F32",
            opcode: 391,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CVT_I32_F32",
            opcode: 392,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CVT_F16_F32",
            opcode: 394,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CVT_F32_F16",
            opcode: 395,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CVT_RPI_I32_F32",
            opcode: 396,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CVT_FLR_I32_F32",
            opcode: 397,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CVT_OFF_F32_I4",
            opcode: 398,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CVT_F32_F64",
            opcode: 399,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CVT_F64_F32",
            opcode: 400,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CVT_F32_UBYTE0",
            opcode: 401,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CVT_F32_UBYTE1",
            opcode: 402,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CVT_F32_UBYTE2",
            opcode: 403,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CVT_F32_UBYTE3",
            opcode: 404,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CVT_U32_F64",
            opcode: 405,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CVT_F64_U32",
            opcode: 406,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_TRUNC_F64",
            opcode: 407,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CEIL_F64",
            opcode: 408,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_RNDNE_F64",
            opcode: 409,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_FLOOR_F64",
            opcode: 410,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_PIPEFLUSH",
            opcode: 411,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_FRACT_F32",
            opcode: 416,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_TRUNC_F32",
            opcode: 417,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CEIL_F32",
            opcode: 418,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_RNDNE_F32",
            opcode: 419,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_FLOOR_F32",
            opcode: 420,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_EXP_F32",
            opcode: 421,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_LOG_F32",
            opcode: 423,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_RCP_F32",
            opcode: 426,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_RCP_IFLAG_F32",
            opcode: 427,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_RSQ_F32",
            opcode: 430,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_RCP_F64",
            opcode: 431,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_RSQ_F64",
            opcode: 433,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_SQRT_F32",
            opcode: 435,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_SQRT_F64",
            opcode: 436,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_SIN_F32",
            opcode: 437,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_COS_F32",
            opcode: 438,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_NOT_B32",
            opcode: 439,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_BFREV_B32",
            opcode: 440,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_FFBH_U32",
            opcode: 441,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_FFBL_B32",
            opcode: 442,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_FFBH_I32",
            opcode: 443,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_FREXP_EXP_I32_F64",
            opcode: 444,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_FREXP_MANT_F64",
            opcode: 445,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_FRACT_F64",
            opcode: 446,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_FREXP_EXP_I32_F32",
            opcode: 447,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_FREXP_MANT_F32",
            opcode: 448,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CLREXCP",
            opcode: 449,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_MOVRELD_B32",
            opcode: 450,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_MOVRELS_B32",
            opcode: 451,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_MOVRELSD_B32",
            opcode: 452,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_MOVRELSD_2_B32",
            opcode: 456,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CVT_F16_U16",
            opcode: 464,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CVT_F16_I16",
            opcode: 465,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CVT_U16_F16",
            opcode: 466,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CVT_I16_F16",
            opcode: 467,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_RCP_F16",
            opcode: 468,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_SQRT_F16",
            opcode: 469,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_RSQ_F16",
            opcode: 470,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_LOG_F16",
            opcode: 471,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_EXP_F16",
            opcode: 472,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_FREXP_MANT_F16",
            opcode: 473,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_FREXP_EXP_I16_F16",
            opcode: 474,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_FLOOR_F16",
            opcode: 475,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CEIL_F16",
            opcode: 476,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_TRUNC_F16",
            opcode: 477,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_RNDNE_F16",
            opcode: 478,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_FRACT_F16",
            opcode: 479,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_SIN_F16",
            opcode: 480,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_COS_F16",
            opcode: 481,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_SAT_PK_U8_I16",
            opcode: 482,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CVT_NORM_I16_F16",
            opcode: 483,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CVT_NORM_U16_F16",
            opcode: 484,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_INTERP_P1_F32",
            opcode: 512,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_INTERP_P2_F32",
            opcode: 513,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_INTERP_MOV_F32",
            opcode: 514,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_LSHLREV_B64",
            opcode: 767,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_LSHRREV_B64",
            opcode: 768,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_ASHRREV_I64",
            opcode: 769,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_ADD_NC_U16",
            opcode: 771,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_SUB_NC_U16",
            opcode: 772,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_MUL_LO_U16",
            opcode: 773,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_LSHRREV_B16",
            opcode: 775,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_ASHRREV_I16",
            opcode: 776,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_MAX_U16",
            opcode: 777,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_MAX_I16",
            opcode: 778,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_MIN_U16",
            opcode: 779,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_MIN_I16",
            opcode: 780,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_ADD_NC_I16",
            opcode: 781,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_SUB_NC_I16",
            opcode: 782,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_PACK_B32_F16",
            opcode: 785,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CVT_PKNORM_I16_F16",
            opcode: 786,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CVT_PKNORM_U16_F16",
            opcode: 787,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_LSHLREV_B16",
            opcode: 788,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_MAD_U16",
            opcode: 832,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_INTERP_P1LL_F16",
            opcode: 834,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_INTERP_P1LV_F16",
            opcode: 835,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_PERM_B32",
            opcode: 836,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_XAD_U32",
            opcode: 837,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_LSHL_ADD_U32",
            opcode: 838,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_ADD_LSHL_U32",
            opcode: 839,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_FMA_F16",
            opcode: 843,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_MIN3_F16",
            opcode: 849,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_MIN3_I16",
            opcode: 850,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_MIN3_U16",
            opcode: 851,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_MAX3_F16",
            opcode: 852,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_MAX3_I16",
            opcode: 853,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_MAX3_U16",
            opcode: 854,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_MED3_F16",
            opcode: 855,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_MED3_I16",
            opcode: 856,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_MED3_U16",
            opcode: 857,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_INTERP_P2_F16",
            opcode: 858,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_MAD_I16",
            opcode: 862,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_DIV_FIXUP_F16",
            opcode: 863,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_READLANE_B32",
            opcode: 864,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_WRITELANE_B32",
            opcode: 865,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_LDEXP_F32",
            opcode: 866,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_BFM_B32",
            opcode: 867,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_BCNT_U32_B32",
            opcode: 868,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_MBCNT_LO_U32_B32",
            opcode: 869,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_MBCNT_HI_U32_B32",
            opcode: 870,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CVT_PKNORM_I16_F32",
            opcode: 872,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CVT_PKNORM_U16_F32",
            opcode: 873,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CVT_PK_U16_U32",
            opcode: 874,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CVT_PK_I16_I32",
            opcode: 875,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_ADD3_U32",
            opcode: 877,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_LSHL_OR_B32",
            opcode: 879,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_AND_OR_B32",
            opcode: 881,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_OR3_B32",
            opcode: 882,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_MAD_U32_U16",
            opcode: 883,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_MAD_I32_I16",
            opcode: 885,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_SUB_NC_I32",
            opcode: 886,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_PERMLANE16_B32",
            opcode: 887,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_PERMLANEX16_B32",
            opcode: 888,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_ADD_NC_I32",
            opcode: 895,
            is_branch: false,
            is_terminator: false,
        },
    ];

    /// Look up an instruction by opcode.
    pub fn lookup(opcode: u16) -> Option<&'static super::InstrEntry> {
        TABLE.iter().find(|e| e.opcode == opcode)
    }
}

/// ENC_VOP3P opcodes (29 instructions).
pub mod vop3p {
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
    pub const TABLE: &[super::InstrEntry] = &[
        super::InstrEntry {
            name: "V_PK_MAD_I16",
            opcode: 0,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_PK_MUL_LO_U16",
            opcode: 1,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_PK_ADD_I16",
            opcode: 2,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_PK_SUB_I16",
            opcode: 3,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_PK_LSHLREV_B16",
            opcode: 4,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_PK_LSHRREV_B16",
            opcode: 5,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_PK_ASHRREV_I16",
            opcode: 6,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_PK_MAX_I16",
            opcode: 7,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_PK_MIN_I16",
            opcode: 8,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_PK_MAD_U16",
            opcode: 9,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_PK_ADD_U16",
            opcode: 10,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_PK_SUB_U16",
            opcode: 11,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_PK_MAX_U16",
            opcode: 12,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_PK_MIN_U16",
            opcode: 13,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_PK_FMA_F16",
            opcode: 14,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_PK_ADD_F16",
            opcode: 15,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_PK_MUL_F16",
            opcode: 16,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_PK_MIN_F16",
            opcode: 17,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_PK_MAX_F16",
            opcode: 18,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_DOT2_F32_F16",
            opcode: 19,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_DOT2_I32_I16",
            opcode: 20,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_DOT2_U32_U16",
            opcode: 21,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_DOT4_I32_I8",
            opcode: 22,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_DOT4_U32_U8",
            opcode: 23,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_DOT8_I32_I4",
            opcode: 24,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_DOT8_U32_U4",
            opcode: 25,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_FMA_MIX_F32",
            opcode: 32,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_FMA_MIXLO_F16",
            opcode: 33,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_FMA_MIXHI_F16",
            opcode: 34,
            is_branch: false,
            is_terminator: false,
        },
    ];

    /// Look up an instruction by opcode.
    pub fn lookup(opcode: u16) -> Option<&'static super::InstrEntry> {
        TABLE.iter().find(|e| e.opcode == opcode)
    }
}

/// ENC_VOPC opcodes (190 instructions).
pub mod vopc {
    /// Set the vector condition code to 0. Store the result into VCC or a scalar register.
    pub const V_CMP_F_F32: u16 = 0;
    /// Set the vector condition code to 1 iff the first input is less than the second input. Store the result into VCC or a ...
    pub const V_CMP_LT_F32: u16 = 1;
    /// Set the vector condition code to 1 iff the first input is equal to the second input. Store the result into VCC or a s...
    pub const V_CMP_EQ_F32: u16 = 2;
    /// Set the vector condition code to 1 iff the first input is less than or equal to the second input. Store the result in...
    pub const V_CMP_LE_F32: u16 = 3;
    /// Set the vector condition code to 1 iff the first input is greater than the second input. Store the result into VCC or...
    pub const V_CMP_GT_F32: u16 = 4;
    /// Set the vector condition code to 1 iff the first input is less than or greater than the second input. Store the resul...
    pub const V_CMP_LG_F32: u16 = 5;
    /// Set the vector condition code to 1 iff the first input is greater than or equal to the second input. Store the result...
    pub const V_CMP_GE_F32: u16 = 6;
    /// Set the vector condition code to 1 iff the first input is orderable to the second input. Store the result into VCC or...
    pub const V_CMP_O_F32: u16 = 7;
    /// Set the vector condition code to 1 iff the first input is not orderable to the second input. Store the result into VC...
    pub const V_CMP_U_F32: u16 = 8;
    /// Set the vector condition code to 1 iff the first input is not greater than or equal to the second input. Store the re...
    pub const V_CMP_NGE_F32: u16 = 9;
    /// Set the vector condition code to 1 iff the first input is not less than or greater than the second input. Store the r...
    pub const V_CMP_NLG_F32: u16 = 10;
    /// Set the vector condition code to 1 iff the first input is not greater than the second input. Store the result into VC...
    pub const V_CMP_NGT_F32: u16 = 11;
    /// Set the vector condition code to 1 iff the first input is not less than or equal to the second input. Store the resul...
    pub const V_CMP_NLE_F32: u16 = 12;
    /// Set the vector condition code to 1 iff the first input is not equal to the second input. Store the result into VCC or...
    pub const V_CMP_NEQ_F32: u16 = 13;
    /// Set the vector condition code to 1 iff the first input is not less than the second input. Store the result into VCC o...
    pub const V_CMP_NLT_F32: u16 = 14;
    /// Set the vector condition code to 1. Store the result into VCC or a scalar register.
    pub const V_CMP_TRU_F32: u16 = 15;
    /// Set the vector condition code to 0. Store the result into the EXEC mask.
    pub const V_CMPX_F_F32: u16 = 16;
    /// Set the vector condition code to 1 iff the first input is less than the second input. Store the result into the EXEC ...
    pub const V_CMPX_LT_F32: u16 = 17;
    /// Set the vector condition code to 1 iff the first input is equal to the second input. Store the result into the EXEC m...
    pub const V_CMPX_EQ_F32: u16 = 18;
    /// Set the vector condition code to 1 iff the first input is less than or equal to the second input. Store the result in...
    pub const V_CMPX_LE_F32: u16 = 19;
    /// Set the vector condition code to 1 iff the first input is greater than the second input. Store the result into the EX...
    pub const V_CMPX_GT_F32: u16 = 20;
    /// Set the vector condition code to 1 iff the first input is less than or greater than the second input. Store the resul...
    pub const V_CMPX_LG_F32: u16 = 21;
    /// Set the vector condition code to 1 iff the first input is greater than or equal to the second input. Store the result...
    pub const V_CMPX_GE_F32: u16 = 22;
    /// Set the vector condition code to 1 iff the first input is orderable to the second input. Store the result into the EX...
    pub const V_CMPX_O_F32: u16 = 23;
    /// Set the vector condition code to 1 iff the first input is not orderable to the second input. Store the result into th...
    pub const V_CMPX_U_F32: u16 = 24;
    /// Set the vector condition code to 1 iff the first input is not greater than or equal to the second input. Store the re...
    pub const V_CMPX_NGE_F32: u16 = 25;
    /// Set the vector condition code to 1 iff the first input is not less than or greater than the second input. Store the r...
    pub const V_CMPX_NLG_F32: u16 = 26;
    /// Set the vector condition code to 1 iff the first input is not greater than the second input. Store the result into th...
    pub const V_CMPX_NGT_F32: u16 = 27;
    /// Set the vector condition code to 1 iff the first input is not less than or equal to the second input. Store the resul...
    pub const V_CMPX_NLE_F32: u16 = 28;
    /// Set the vector condition code to 1 iff the first input is not equal to the second input. Store the result into the EX...
    pub const V_CMPX_NEQ_F32: u16 = 29;
    /// Set the vector condition code to 1 iff the first input is not less than the second input. Store the result into the E...
    pub const V_CMPX_NLT_F32: u16 = 30;
    /// Set the vector condition code to 1. Store the result into the EXEC mask.
    pub const V_CMPX_TRU_F32: u16 = 31;
    /// Set the vector condition code to 0. Store the result into VCC or a scalar register.
    pub const V_CMP_F_F64: u16 = 32;
    /// Set the vector condition code to 1 iff the first input is less than the second input. Store the result into VCC or a ...
    pub const V_CMP_LT_F64: u16 = 33;
    /// Set the vector condition code to 1 iff the first input is equal to the second input. Store the result into VCC or a s...
    pub const V_CMP_EQ_F64: u16 = 34;
    /// Set the vector condition code to 1 iff the first input is less than or equal to the second input. Store the result in...
    pub const V_CMP_LE_F64: u16 = 35;
    /// Set the vector condition code to 1 iff the first input is greater than the second input. Store the result into VCC or...
    pub const V_CMP_GT_F64: u16 = 36;
    /// Set the vector condition code to 1 iff the first input is less than or greater than the second input. Store the resul...
    pub const V_CMP_LG_F64: u16 = 37;
    /// Set the vector condition code to 1 iff the first input is greater than or equal to the second input. Store the result...
    pub const V_CMP_GE_F64: u16 = 38;
    /// Set the vector condition code to 1 iff the first input is orderable to the second input. Store the result into VCC or...
    pub const V_CMP_O_F64: u16 = 39;
    /// Set the vector condition code to 1 iff the first input is not orderable to the second input. Store the result into VC...
    pub const V_CMP_U_F64: u16 = 40;
    /// Set the vector condition code to 1 iff the first input is not greater than or equal to the second input. Store the re...
    pub const V_CMP_NGE_F64: u16 = 41;
    /// Set the vector condition code to 1 iff the first input is not less than or greater than the second input. Store the r...
    pub const V_CMP_NLG_F64: u16 = 42;
    /// Set the vector condition code to 1 iff the first input is not greater than the second input. Store the result into VC...
    pub const V_CMP_NGT_F64: u16 = 43;
    /// Set the vector condition code to 1 iff the first input is not less than or equal to the second input. Store the resul...
    pub const V_CMP_NLE_F64: u16 = 44;
    /// Set the vector condition code to 1 iff the first input is not equal to the second input. Store the result into VCC or...
    pub const V_CMP_NEQ_F64: u16 = 45;
    /// Set the vector condition code to 1 iff the first input is not less than the second input. Store the result into VCC o...
    pub const V_CMP_NLT_F64: u16 = 46;
    /// Set the vector condition code to 1. Store the result into VCC or a scalar register.
    pub const V_CMP_TRU_F64: u16 = 47;
    /// Set the vector condition code to 0. Store the result into the EXEC mask.
    pub const V_CMPX_F_F64: u16 = 48;
    /// Set the vector condition code to 1 iff the first input is less than the second input. Store the result into the EXEC ...
    pub const V_CMPX_LT_F64: u16 = 49;
    /// Set the vector condition code to 1 iff the first input is equal to the second input. Store the result into the EXEC m...
    pub const V_CMPX_EQ_F64: u16 = 50;
    /// Set the vector condition code to 1 iff the first input is less than or equal to the second input. Store the result in...
    pub const V_CMPX_LE_F64: u16 = 51;
    /// Set the vector condition code to 1 iff the first input is greater than the second input. Store the result into the EX...
    pub const V_CMPX_GT_F64: u16 = 52;
    /// Set the vector condition code to 1 iff the first input is less than or greater than the second input. Store the resul...
    pub const V_CMPX_LG_F64: u16 = 53;
    /// Set the vector condition code to 1 iff the first input is greater than or equal to the second input. Store the result...
    pub const V_CMPX_GE_F64: u16 = 54;
    /// Set the vector condition code to 1 iff the first input is orderable to the second input. Store the result into the EX...
    pub const V_CMPX_O_F64: u16 = 55;
    /// Set the vector condition code to 1 iff the first input is not orderable to the second input. Store the result into th...
    pub const V_CMPX_U_F64: u16 = 56;
    /// Set the vector condition code to 1 iff the first input is not greater than or equal to the second input. Store the re...
    pub const V_CMPX_NGE_F64: u16 = 57;
    /// Set the vector condition code to 1 iff the first input is not less than or greater than the second input. Store the r...
    pub const V_CMPX_NLG_F64: u16 = 58;
    /// Set the vector condition code to 1 iff the first input is not greater than the second input. Store the result into th...
    pub const V_CMPX_NGT_F64: u16 = 59;
    /// Set the vector condition code to 1 iff the first input is not less than or equal to the second input. Store the resul...
    pub const V_CMPX_NLE_F64: u16 = 60;
    /// Set the vector condition code to 1 iff the first input is not equal to the second input. Store the result into the EX...
    pub const V_CMPX_NEQ_F64: u16 = 61;
    /// Set the vector condition code to 1 iff the first input is not less than the second input. Store the result into the E...
    pub const V_CMPX_NLT_F64: u16 = 62;
    /// Set the vector condition code to 1. Store the result into the EXEC mask.
    pub const V_CMPX_TRU_F64: u16 = 63;
    /// Set the vector condition code to 0. Store the result into VCC or a scalar register.
    pub const V_CMP_F_I32: u16 = 128;
    /// Set the vector condition code to 1 iff the first input is less than the second input. Store the result into VCC or a ...
    pub const V_CMP_LT_I32: u16 = 129;
    /// Set the vector condition code to 1 iff the first input is equal to the second input. Store the result into VCC or a s...
    pub const V_CMP_EQ_I32: u16 = 130;
    /// Set the vector condition code to 1 iff the first input is less than or equal to the second input. Store the result in...
    pub const V_CMP_LE_I32: u16 = 131;
    /// Set the vector condition code to 1 iff the first input is greater than the second input. Store the result into VCC or...
    pub const V_CMP_GT_I32: u16 = 132;
    /// Set the vector condition code to 1 iff the first input is not equal to the second input. Store the result into VCC or...
    pub const V_CMP_NE_I32: u16 = 133;
    /// Set the vector condition code to 1 iff the first input is greater than or equal to the second input. Store the result...
    pub const V_CMP_GE_I32: u16 = 134;
    /// Set the vector condition code to 1. Store the result into VCC or a scalar register.
    pub const V_CMP_T_I32: u16 = 135;
    /// Evaluate the IEEE numeric class function specified as a 10 bit mask in the second input on the first input, a single-...
    pub const V_CMP_CLASS_F32: u16 = 136;
    /// Set the vector condition code to 1 iff the first input is less than the second input. Store the result into VCC or a ...
    pub const V_CMP_LT_I16: u16 = 137;
    /// Set the vector condition code to 1 iff the first input is equal to the second input. Store the result into VCC or a s...
    pub const V_CMP_EQ_I16: u16 = 138;
    /// Set the vector condition code to 1 iff the first input is less than or equal to the second input. Store the result in...
    pub const V_CMP_LE_I16: u16 = 139;
    /// Set the vector condition code to 1 iff the first input is greater than the second input. Store the result into VCC or...
    pub const V_CMP_GT_I16: u16 = 140;
    /// Set the vector condition code to 1 iff the first input is not equal to the second input. Store the result into VCC or...
    pub const V_CMP_NE_I16: u16 = 141;
    /// Set the vector condition code to 1 iff the first input is greater than or equal to the second input. Store the result...
    pub const V_CMP_GE_I16: u16 = 142;
    /// Evaluate the IEEE numeric class function specified as a 10 bit mask in the second input on the first input, a half-pr...
    pub const V_CMP_CLASS_F16: u16 = 143;
    /// Set the vector condition code to 0. Store the result into the EXEC mask.
    pub const V_CMPX_F_I32: u16 = 144;
    /// Set the vector condition code to 1 iff the first input is less than the second input. Store the result into the EXEC ...
    pub const V_CMPX_LT_I32: u16 = 145;
    /// Set the vector condition code to 1 iff the first input is equal to the second input. Store the result into the EXEC m...
    pub const V_CMPX_EQ_I32: u16 = 146;
    /// Set the vector condition code to 1 iff the first input is less than or equal to the second input. Store the result in...
    pub const V_CMPX_LE_I32: u16 = 147;
    /// Set the vector condition code to 1 iff the first input is greater than the second input. Store the result into the EX...
    pub const V_CMPX_GT_I32: u16 = 148;
    /// Set the vector condition code to 1 iff the first input is not equal to the second input. Store the result into the EX...
    pub const V_CMPX_NE_I32: u16 = 149;
    /// Set the vector condition code to 1 iff the first input is greater than or equal to the second input. Store the result...
    pub const V_CMPX_GE_I32: u16 = 150;
    /// Set the vector condition code to 1. Store the result into the EXEC mask.
    pub const V_CMPX_T_I32: u16 = 151;
    /// Evaluate the IEEE numeric class function specified as a 10 bit mask in the second input on the first input, a single-...
    pub const V_CMPX_CLASS_F32: u16 = 152;
    /// Set the vector condition code to 1 iff the first input is less than the second input. Store the result into the EXEC ...
    pub const V_CMPX_LT_I16: u16 = 153;
    /// Set the vector condition code to 1 iff the first input is equal to the second input. Store the result into the EXEC m...
    pub const V_CMPX_EQ_I16: u16 = 154;
    /// Set the vector condition code to 1 iff the first input is less than or equal to the second input. Store the result in...
    pub const V_CMPX_LE_I16: u16 = 155;
    /// Set the vector condition code to 1 iff the first input is greater than the second input. Store the result into the EX...
    pub const V_CMPX_GT_I16: u16 = 156;
    /// Set the vector condition code to 1 iff the first input is not equal to the second input. Store the result into the EX...
    pub const V_CMPX_NE_I16: u16 = 157;
    /// Set the vector condition code to 1 iff the first input is greater than or equal to the second input. Store the result...
    pub const V_CMPX_GE_I16: u16 = 158;
    /// Evaluate the IEEE numeric class function specified as a 10 bit mask in the second input on the first input, a half-pr...
    pub const V_CMPX_CLASS_F16: u16 = 159;
    /// Set the vector condition code to 0. Store the result into VCC or a scalar register.
    pub const V_CMP_F_I64: u16 = 160;
    /// Set the vector condition code to 1 iff the first input is less than the second input. Store the result into VCC or a ...
    pub const V_CMP_LT_I64: u16 = 161;
    /// Set the vector condition code to 1 iff the first input is equal to the second input. Store the result into VCC or a s...
    pub const V_CMP_EQ_I64: u16 = 162;
    /// Set the vector condition code to 1 iff the first input is less than or equal to the second input. Store the result in...
    pub const V_CMP_LE_I64: u16 = 163;
    /// Set the vector condition code to 1 iff the first input is greater than the second input. Store the result into VCC or...
    pub const V_CMP_GT_I64: u16 = 164;
    /// Set the vector condition code to 1 iff the first input is not equal to the second input. Store the result into VCC or...
    pub const V_CMP_NE_I64: u16 = 165;
    /// Set the vector condition code to 1 iff the first input is greater than or equal to the second input. Store the result...
    pub const V_CMP_GE_I64: u16 = 166;
    /// Set the vector condition code to 1. Store the result into VCC or a scalar register.
    pub const V_CMP_T_I64: u16 = 167;
    /// Evaluate the IEEE numeric class function specified as a 10 bit mask in the second input on the first input, a double-...
    pub const V_CMP_CLASS_F64: u16 = 168;
    /// Set the vector condition code to 1 iff the first input is less than the second input. Store the result into VCC or a ...
    pub const V_CMP_LT_U16: u16 = 169;
    /// Set the vector condition code to 1 iff the first input is equal to the second input. Store the result into VCC or a s...
    pub const V_CMP_EQ_U16: u16 = 170;
    /// Set the vector condition code to 1 iff the first input is less than or equal to the second input. Store the result in...
    pub const V_CMP_LE_U16: u16 = 171;
    /// Set the vector condition code to 1 iff the first input is greater than the second input. Store the result into VCC or...
    pub const V_CMP_GT_U16: u16 = 172;
    /// Set the vector condition code to 1 iff the first input is not equal to the second input. Store the result into VCC or...
    pub const V_CMP_NE_U16: u16 = 173;
    /// Set the vector condition code to 1 iff the first input is greater than or equal to the second input. Store the result...
    pub const V_CMP_GE_U16: u16 = 174;
    /// Set the vector condition code to 0. Store the result into the EXEC mask.
    pub const V_CMPX_F_I64: u16 = 176;
    /// Set the vector condition code to 1 iff the first input is less than the second input. Store the result into the EXEC ...
    pub const V_CMPX_LT_I64: u16 = 177;
    /// Set the vector condition code to 1 iff the first input is equal to the second input. Store the result into the EXEC m...
    pub const V_CMPX_EQ_I64: u16 = 178;
    /// Set the vector condition code to 1 iff the first input is less than or equal to the second input. Store the result in...
    pub const V_CMPX_LE_I64: u16 = 179;
    /// Set the vector condition code to 1 iff the first input is greater than the second input. Store the result into the EX...
    pub const V_CMPX_GT_I64: u16 = 180;
    /// Set the vector condition code to 1 iff the first input is not equal to the second input. Store the result into the EX...
    pub const V_CMPX_NE_I64: u16 = 181;
    /// Set the vector condition code to 1 iff the first input is greater than or equal to the second input. Store the result...
    pub const V_CMPX_GE_I64: u16 = 182;
    /// Set the vector condition code to 1. Store the result into the EXEC mask.
    pub const V_CMPX_T_I64: u16 = 183;
    /// Evaluate the IEEE numeric class function specified as a 10 bit mask in the second input on the first input, a double-...
    pub const V_CMPX_CLASS_F64: u16 = 184;
    /// Set the vector condition code to 1 iff the first input is less than the second input. Store the result into the EXEC ...
    pub const V_CMPX_LT_U16: u16 = 185;
    /// Set the vector condition code to 1 iff the first input is equal to the second input. Store the result into the EXEC m...
    pub const V_CMPX_EQ_U16: u16 = 186;
    /// Set the vector condition code to 1 iff the first input is less than or equal to the second input. Store the result in...
    pub const V_CMPX_LE_U16: u16 = 187;
    /// Set the vector condition code to 1 iff the first input is greater than the second input. Store the result into the EX...
    pub const V_CMPX_GT_U16: u16 = 188;
    /// Set the vector condition code to 1 iff the first input is not equal to the second input. Store the result into the EX...
    pub const V_CMPX_NE_U16: u16 = 189;
    /// Set the vector condition code to 1 iff the first input is greater than or equal to the second input. Store the result...
    pub const V_CMPX_GE_U16: u16 = 190;
    /// Set the vector condition code to 0. Store the result into VCC or a scalar register.
    pub const V_CMP_F_U32: u16 = 192;
    /// Set the vector condition code to 1 iff the first input is less than the second input. Store the result into VCC or a ...
    pub const V_CMP_LT_U32: u16 = 193;
    /// Set the vector condition code to 1 iff the first input is equal to the second input. Store the result into VCC or a s...
    pub const V_CMP_EQ_U32: u16 = 194;
    /// Set the vector condition code to 1 iff the first input is less than or equal to the second input. Store the result in...
    pub const V_CMP_LE_U32: u16 = 195;
    /// Set the vector condition code to 1 iff the first input is greater than the second input. Store the result into VCC or...
    pub const V_CMP_GT_U32: u16 = 196;
    /// Set the vector condition code to 1 iff the first input is not equal to the second input. Store the result into VCC or...
    pub const V_CMP_NE_U32: u16 = 197;
    /// Set the vector condition code to 1 iff the first input is greater than or equal to the second input. Store the result...
    pub const V_CMP_GE_U32: u16 = 198;
    /// Set the vector condition code to 1. Store the result into VCC or a scalar register.
    pub const V_CMP_T_U32: u16 = 199;
    /// Set the vector condition code to 0. Store the result into VCC or a scalar register.
    pub const V_CMP_F_F16: u16 = 200;
    /// Set the vector condition code to 1 iff the first input is less than the second input. Store the result into VCC or a ...
    pub const V_CMP_LT_F16: u16 = 201;
    /// Set the vector condition code to 1 iff the first input is equal to the second input. Store the result into VCC or a s...
    pub const V_CMP_EQ_F16: u16 = 202;
    /// Set the vector condition code to 1 iff the first input is less than or equal to the second input. Store the result in...
    pub const V_CMP_LE_F16: u16 = 203;
    /// Set the vector condition code to 1 iff the first input is greater than the second input. Store the result into VCC or...
    pub const V_CMP_GT_F16: u16 = 204;
    /// Set the vector condition code to 1 iff the first input is less than or greater than the second input. Store the resul...
    pub const V_CMP_LG_F16: u16 = 205;
    /// Set the vector condition code to 1 iff the first input is greater than or equal to the second input. Store the result...
    pub const V_CMP_GE_F16: u16 = 206;
    /// Set the vector condition code to 1 iff the first input is orderable to the second input. Store the result into VCC or...
    pub const V_CMP_O_F16: u16 = 207;
    /// Set the vector condition code to 0. Store the result into the EXEC mask.
    pub const V_CMPX_F_U32: u16 = 208;
    /// Set the vector condition code to 1 iff the first input is less than the second input. Store the result into the EXEC ...
    pub const V_CMPX_LT_U32: u16 = 209;
    /// Set the vector condition code to 1 iff the first input is equal to the second input. Store the result into the EXEC m...
    pub const V_CMPX_EQ_U32: u16 = 210;
    /// Set the vector condition code to 1 iff the first input is less than or equal to the second input. Store the result in...
    pub const V_CMPX_LE_U32: u16 = 211;
    /// Set the vector condition code to 1 iff the first input is greater than the second input. Store the result into the EX...
    pub const V_CMPX_GT_U32: u16 = 212;
    /// Set the vector condition code to 1 iff the first input is not equal to the second input. Store the result into the EX...
    pub const V_CMPX_NE_U32: u16 = 213;
    /// Set the vector condition code to 1 iff the first input is greater than or equal to the second input. Store the result...
    pub const V_CMPX_GE_U32: u16 = 214;
    /// Set the vector condition code to 1. Store the result into the EXEC mask.
    pub const V_CMPX_T_U32: u16 = 215;
    /// Set the vector condition code to 0. Store the result into the EXEC mask.
    pub const V_CMPX_F_F16: u16 = 216;
    /// Set the vector condition code to 1 iff the first input is less than the second input. Store the result into the EXEC ...
    pub const V_CMPX_LT_F16: u16 = 217;
    /// Set the vector condition code to 1 iff the first input is equal to the second input. Store the result into the EXEC m...
    pub const V_CMPX_EQ_F16: u16 = 218;
    /// Set the vector condition code to 1 iff the first input is less than or equal to the second input. Store the result in...
    pub const V_CMPX_LE_F16: u16 = 219;
    /// Set the vector condition code to 1 iff the first input is greater than the second input. Store the result into the EX...
    pub const V_CMPX_GT_F16: u16 = 220;
    /// Set the vector condition code to 1 iff the first input is less than or greater than the second input. Store the resul...
    pub const V_CMPX_LG_F16: u16 = 221;
    /// Set the vector condition code to 1 iff the first input is greater than or equal to the second input. Store the result...
    pub const V_CMPX_GE_F16: u16 = 222;
    /// Set the vector condition code to 1 iff the first input is orderable to the second input. Store the result into the EX...
    pub const V_CMPX_O_F16: u16 = 223;
    /// Set the vector condition code to 0. Store the result into VCC or a scalar register.
    pub const V_CMP_F_U64: u16 = 224;
    /// Set the vector condition code to 1 iff the first input is less than the second input. Store the result into VCC or a ...
    pub const V_CMP_LT_U64: u16 = 225;
    /// Set the vector condition code to 1 iff the first input is equal to the second input. Store the result into VCC or a s...
    pub const V_CMP_EQ_U64: u16 = 226;
    /// Set the vector condition code to 1 iff the first input is less than or equal to the second input. Store the result in...
    pub const V_CMP_LE_U64: u16 = 227;
    /// Set the vector condition code to 1 iff the first input is greater than the second input. Store the result into VCC or...
    pub const V_CMP_GT_U64: u16 = 228;
    /// Set the vector condition code to 1 iff the first input is not equal to the second input. Store the result into VCC or...
    pub const V_CMP_NE_U64: u16 = 229;
    /// Set the vector condition code to 1 iff the first input is greater than or equal to the second input. Store the result...
    pub const V_CMP_GE_U64: u16 = 230;
    /// Set the vector condition code to 1. Store the result into VCC or a scalar register.
    pub const V_CMP_T_U64: u16 = 231;
    /// Set the vector condition code to 1 iff the first input is not orderable to the second input. Store the result into VC...
    pub const V_CMP_U_F16: u16 = 232;
    /// Set the vector condition code to 1 iff the first input is not greater than or equal to the second input. Store the re...
    pub const V_CMP_NGE_F16: u16 = 233;
    /// Set the vector condition code to 1 iff the first input is not less than or greater than the second input. Store the r...
    pub const V_CMP_NLG_F16: u16 = 234;
    /// Set the vector condition code to 1 iff the first input is not greater than the second input. Store the result into VC...
    pub const V_CMP_NGT_F16: u16 = 235;
    /// Set the vector condition code to 1 iff the first input is not less than or equal to the second input. Store the resul...
    pub const V_CMP_NLE_F16: u16 = 236;
    /// Set the vector condition code to 1 iff the first input is not equal to the second input. Store the result into VCC or...
    pub const V_CMP_NEQ_F16: u16 = 237;
    /// Set the vector condition code to 1 iff the first input is not less than the second input. Store the result into VCC o...
    pub const V_CMP_NLT_F16: u16 = 238;
    /// Set the vector condition code to 1. Store the result into VCC or a scalar register.
    pub const V_CMP_TRU_F16: u16 = 239;
    /// Set the vector condition code to 0. Store the result into the EXEC mask.
    pub const V_CMPX_F_U64: u16 = 240;
    /// Set the vector condition code to 1 iff the first input is less than the second input. Store the result into the EXEC ...
    pub const V_CMPX_LT_U64: u16 = 241;
    /// Set the vector condition code to 1 iff the first input is equal to the second input. Store the result into the EXEC m...
    pub const V_CMPX_EQ_U64: u16 = 242;
    /// Set the vector condition code to 1 iff the first input is less than or equal to the second input. Store the result in...
    pub const V_CMPX_LE_U64: u16 = 243;
    /// Set the vector condition code to 1 iff the first input is greater than the second input. Store the result into the EX...
    pub const V_CMPX_GT_U64: u16 = 244;
    /// Set the vector condition code to 1 iff the first input is not equal to the second input. Store the result into the EX...
    pub const V_CMPX_NE_U64: u16 = 245;
    /// Set the vector condition code to 1 iff the first input is greater than or equal to the second input. Store the result...
    pub const V_CMPX_GE_U64: u16 = 246;
    /// Set the vector condition code to 1. Store the result into the EXEC mask.
    pub const V_CMPX_T_U64: u16 = 247;
    /// Set the vector condition code to 1 iff the first input is not orderable to the second input. Store the result into th...
    pub const V_CMPX_U_F16: u16 = 248;
    /// Set the vector condition code to 1 iff the first input is not greater than or equal to the second input. Store the re...
    pub const V_CMPX_NGE_F16: u16 = 249;
    /// Set the vector condition code to 1 iff the first input is not less than or greater than the second input. Store the r...
    pub const V_CMPX_NLG_F16: u16 = 250;
    /// Set the vector condition code to 1 iff the first input is not greater than the second input. Store the result into th...
    pub const V_CMPX_NGT_F16: u16 = 251;
    /// Set the vector condition code to 1 iff the first input is not less than or equal to the second input. Store the resul...
    pub const V_CMPX_NLE_F16: u16 = 252;
    /// Set the vector condition code to 1 iff the first input is not equal to the second input. Store the result into the EX...
    pub const V_CMPX_NEQ_F16: u16 = 253;
    /// Set the vector condition code to 1 iff the first input is not less than the second input. Store the result into the E...
    pub const V_CMPX_NLT_F16: u16 = 254;
    /// Set the vector condition code to 1. Store the result into the EXEC mask.
    pub const V_CMPX_TRU_F16: u16 = 255;

    /// All ENC_VOPC instructions.
    pub const TABLE: &[super::InstrEntry] = &[
        super::InstrEntry {
            name: "V_CMP_F_F32",
            opcode: 0,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_LT_F32",
            opcode: 1,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_EQ_F32",
            opcode: 2,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_LE_F32",
            opcode: 3,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_GT_F32",
            opcode: 4,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_LG_F32",
            opcode: 5,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_GE_F32",
            opcode: 6,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_O_F32",
            opcode: 7,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_U_F32",
            opcode: 8,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_NGE_F32",
            opcode: 9,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_NLG_F32",
            opcode: 10,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_NGT_F32",
            opcode: 11,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_NLE_F32",
            opcode: 12,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_NEQ_F32",
            opcode: 13,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_NLT_F32",
            opcode: 14,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_TRU_F32",
            opcode: 15,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_F_F32",
            opcode: 16,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_LT_F32",
            opcode: 17,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_EQ_F32",
            opcode: 18,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_LE_F32",
            opcode: 19,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_GT_F32",
            opcode: 20,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_LG_F32",
            opcode: 21,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_GE_F32",
            opcode: 22,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_O_F32",
            opcode: 23,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_U_F32",
            opcode: 24,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_NGE_F32",
            opcode: 25,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_NLG_F32",
            opcode: 26,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_NGT_F32",
            opcode: 27,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_NLE_F32",
            opcode: 28,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_NEQ_F32",
            opcode: 29,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_NLT_F32",
            opcode: 30,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_TRU_F32",
            opcode: 31,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_F_F64",
            opcode: 32,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_LT_F64",
            opcode: 33,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_EQ_F64",
            opcode: 34,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_LE_F64",
            opcode: 35,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_GT_F64",
            opcode: 36,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_LG_F64",
            opcode: 37,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_GE_F64",
            opcode: 38,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_O_F64",
            opcode: 39,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_U_F64",
            opcode: 40,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_NGE_F64",
            opcode: 41,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_NLG_F64",
            opcode: 42,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_NGT_F64",
            opcode: 43,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_NLE_F64",
            opcode: 44,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_NEQ_F64",
            opcode: 45,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_NLT_F64",
            opcode: 46,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_TRU_F64",
            opcode: 47,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_F_F64",
            opcode: 48,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_LT_F64",
            opcode: 49,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_EQ_F64",
            opcode: 50,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_LE_F64",
            opcode: 51,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_GT_F64",
            opcode: 52,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_LG_F64",
            opcode: 53,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_GE_F64",
            opcode: 54,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_O_F64",
            opcode: 55,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_U_F64",
            opcode: 56,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_NGE_F64",
            opcode: 57,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_NLG_F64",
            opcode: 58,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_NGT_F64",
            opcode: 59,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_NLE_F64",
            opcode: 60,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_NEQ_F64",
            opcode: 61,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_NLT_F64",
            opcode: 62,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_TRU_F64",
            opcode: 63,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_F_I32",
            opcode: 128,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_LT_I32",
            opcode: 129,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_EQ_I32",
            opcode: 130,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_LE_I32",
            opcode: 131,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_GT_I32",
            opcode: 132,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_NE_I32",
            opcode: 133,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_GE_I32",
            opcode: 134,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_T_I32",
            opcode: 135,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_CLASS_F32",
            opcode: 136,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_LT_I16",
            opcode: 137,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_EQ_I16",
            opcode: 138,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_LE_I16",
            opcode: 139,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_GT_I16",
            opcode: 140,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_NE_I16",
            opcode: 141,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_GE_I16",
            opcode: 142,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_CLASS_F16",
            opcode: 143,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_F_I32",
            opcode: 144,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_LT_I32",
            opcode: 145,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_EQ_I32",
            opcode: 146,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_LE_I32",
            opcode: 147,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_GT_I32",
            opcode: 148,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_NE_I32",
            opcode: 149,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_GE_I32",
            opcode: 150,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_T_I32",
            opcode: 151,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_CLASS_F32",
            opcode: 152,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_LT_I16",
            opcode: 153,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_EQ_I16",
            opcode: 154,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_LE_I16",
            opcode: 155,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_GT_I16",
            opcode: 156,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_NE_I16",
            opcode: 157,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_GE_I16",
            opcode: 158,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_CLASS_F16",
            opcode: 159,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_F_I64",
            opcode: 160,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_LT_I64",
            opcode: 161,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_EQ_I64",
            opcode: 162,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_LE_I64",
            opcode: 163,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_GT_I64",
            opcode: 164,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_NE_I64",
            opcode: 165,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_GE_I64",
            opcode: 166,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_T_I64",
            opcode: 167,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_CLASS_F64",
            opcode: 168,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_LT_U16",
            opcode: 169,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_EQ_U16",
            opcode: 170,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_LE_U16",
            opcode: 171,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_GT_U16",
            opcode: 172,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_NE_U16",
            opcode: 173,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_GE_U16",
            opcode: 174,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_F_I64",
            opcode: 176,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_LT_I64",
            opcode: 177,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_EQ_I64",
            opcode: 178,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_LE_I64",
            opcode: 179,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_GT_I64",
            opcode: 180,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_NE_I64",
            opcode: 181,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_GE_I64",
            opcode: 182,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_T_I64",
            opcode: 183,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_CLASS_F64",
            opcode: 184,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_LT_U16",
            opcode: 185,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_EQ_U16",
            opcode: 186,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_LE_U16",
            opcode: 187,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_GT_U16",
            opcode: 188,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_NE_U16",
            opcode: 189,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_GE_U16",
            opcode: 190,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_F_U32",
            opcode: 192,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_LT_U32",
            opcode: 193,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_EQ_U32",
            opcode: 194,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_LE_U32",
            opcode: 195,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_GT_U32",
            opcode: 196,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_NE_U32",
            opcode: 197,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_GE_U32",
            opcode: 198,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_T_U32",
            opcode: 199,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_F_F16",
            opcode: 200,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_LT_F16",
            opcode: 201,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_EQ_F16",
            opcode: 202,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_LE_F16",
            opcode: 203,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_GT_F16",
            opcode: 204,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_LG_F16",
            opcode: 205,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_GE_F16",
            opcode: 206,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_O_F16",
            opcode: 207,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_F_U32",
            opcode: 208,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_LT_U32",
            opcode: 209,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_EQ_U32",
            opcode: 210,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_LE_U32",
            opcode: 211,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_GT_U32",
            opcode: 212,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_NE_U32",
            opcode: 213,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_GE_U32",
            opcode: 214,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_T_U32",
            opcode: 215,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_F_F16",
            opcode: 216,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_LT_F16",
            opcode: 217,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_EQ_F16",
            opcode: 218,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_LE_F16",
            opcode: 219,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_GT_F16",
            opcode: 220,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_LG_F16",
            opcode: 221,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_GE_F16",
            opcode: 222,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_O_F16",
            opcode: 223,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_F_U64",
            opcode: 224,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_LT_U64",
            opcode: 225,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_EQ_U64",
            opcode: 226,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_LE_U64",
            opcode: 227,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_GT_U64",
            opcode: 228,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_NE_U64",
            opcode: 229,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_GE_U64",
            opcode: 230,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_T_U64",
            opcode: 231,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_U_F16",
            opcode: 232,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_NGE_F16",
            opcode: 233,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_NLG_F16",
            opcode: 234,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_NGT_F16",
            opcode: 235,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_NLE_F16",
            opcode: 236,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_NEQ_F16",
            opcode: 237,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_NLT_F16",
            opcode: 238,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMP_TRU_F16",
            opcode: 239,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_F_U64",
            opcode: 240,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_LT_U64",
            opcode: 241,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_EQ_U64",
            opcode: 242,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_LE_U64",
            opcode: 243,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_GT_U64",
            opcode: 244,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_NE_U64",
            opcode: 245,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_GE_U64",
            opcode: 246,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_T_U64",
            opcode: 247,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_U_F16",
            opcode: 248,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_NGE_F16",
            opcode: 249,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_NLG_F16",
            opcode: 250,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_NGT_F16",
            opcode: 251,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_NLE_F16",
            opcode: 252,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_NEQ_F16",
            opcode: 253,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_NLT_F16",
            opcode: 254,
            is_branch: false,
            is_terminator: false,
        },
        super::InstrEntry {
            name: "V_CMPX_TRU_F16",
            opcode: 255,
            is_branch: false,
            is_terminator: false,
        },
    ];

    /// Look up an instruction by opcode.
    pub fn lookup(opcode: u16) -> Option<&'static super::InstrEntry> {
        TABLE.iter().find(|e| e.opcode == opcode)
    }
}

/// Total instruction count across all compute-relevant encodings: 1446
pub const TOTAL_INSTRUCTIONS: usize = 1446;

/// Look up encoding field info by name.
pub fn encoding_bits(name: &str) -> Option<u32> {
    match name {
        "ENC_DS" => Some(64),
        "ENC_FLAT" => Some(64),
        "ENC_FLAT_GLBL" => Some(64),
        "ENC_FLAT_SCRATCH" => Some(64),
        "ENC_MIMG" => Some(64),
        "ENC_MTBUF" => Some(64),
        "ENC_MUBUF" => Some(64),
        "ENC_SMEM" => Some(64),
        "ENC_SOP1" => Some(32),
        "ENC_SOP2" => Some(32),
        "ENC_SOPC" => Some(32),
        "ENC_SOPK" => Some(32),
        "ENC_SOPP" => Some(32),
        "ENC_VOP1" => Some(32),
        "ENC_VOP2" => Some(32),
        "ENC_VOP3" => Some(64),
        "ENC_VOP3P" => Some(64),
        "ENC_VOPC" => Some(32),
        _ => None,
    }
}
