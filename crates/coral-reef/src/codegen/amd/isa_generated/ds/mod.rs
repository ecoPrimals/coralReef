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
mod table;
pub use table::{TABLE, lookup};

/// ENC_DS encoding fields (64 bits).
pub mod fields {
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
/// LDS
pub const DS_CONSUME: u16 = 61;
/// LDS
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
