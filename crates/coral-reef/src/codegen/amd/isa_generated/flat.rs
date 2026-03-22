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

/// ENC_FLAT encoding fields (64 bits).
pub mod fields {
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

// GFX9 (Vega) hardware encoding for FLAT loads.  The auto-generated RDNA2
// values (8-15) are wrong for GFX9 where loads were renumbered to 16-23.
// Verified against `llvm-mc -mcpu=gfx906 -show-encoding`.
/// Load 8 bits of unsigned data from the flat aperture, zero extend to 32 bits and store the result into a vector register.
pub const FLAT_LOAD_UBYTE: u16 = 16;
/// Load 8 bits of signed data from the flat aperture, sign extend to 32 bits and store the result into a vector register.
pub const FLAT_LOAD_SBYTE: u16 = 17;
/// Load 16 bits of unsigned data from the flat aperture, zero extend to 32 bits and store the result into a vector regis...
pub const FLAT_LOAD_USHORT: u16 = 18;
/// Load 16 bits of signed data from the flat aperture, sign extend to 32 bits and store the result into a vector register.
pub const FLAT_LOAD_SSHORT: u16 = 19;
/// Load 32 bits of data from the flat aperture into a vector register.
pub const FLAT_LOAD_DWORD: u16 = 20;
/// Load 64 bits of data from the flat aperture into a vector register.
pub const FLAT_LOAD_DWORDX2: u16 = 21;
/// Load 96 bits of data from the flat aperture into a vector register.
pub const FLAT_LOAD_DWORDX3: u16 = 22;
/// Load 128 bits of data from the flat aperture into a vector register.
pub const FLAT_LOAD_DWORDX4: u16 = 23;
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
// GFX9: DWORDX3/X4 order is swapped vs RDNA2 (verified via llvm-mc).
/// Store 96 bits of data from vector input registers into the flat aperture.
pub const FLAT_STORE_DWORDX3: u16 = 30;
/// Store 128 bits of data from vector input registers into the flat aperture.
pub const FLAT_STORE_DWORDX4: u16 = 31;
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
pub const TABLE: &[InstrEntry] = &[
    InstrEntry {
        name: "FLAT_LOAD_UBYTE",
        opcode: 16,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "FLAT_LOAD_SBYTE",
        opcode: 17,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "FLAT_LOAD_USHORT",
        opcode: 18,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "FLAT_LOAD_SSHORT",
        opcode: 19,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "FLAT_LOAD_DWORD",
        opcode: 20,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "FLAT_LOAD_DWORDX2",
        opcode: 21,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "FLAT_LOAD_DWORDX3",
        opcode: 22,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "FLAT_LOAD_DWORDX4",
        opcode: 23,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "FLAT_STORE_BYTE",
        opcode: 24,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "FLAT_STORE_BYTE_D16_HI",
        opcode: 25,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "FLAT_STORE_SHORT",
        opcode: 26,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "FLAT_STORE_SHORT_D16_HI",
        opcode: 27,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "FLAT_STORE_DWORD",
        opcode: 28,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "FLAT_STORE_DWORDX2",
        opcode: 29,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "FLAT_STORE_DWORDX3",
        opcode: 30,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "FLAT_STORE_DWORDX4",
        opcode: 31,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "FLAT_LOAD_UBYTE_D16",
        opcode: 32,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "FLAT_LOAD_UBYTE_D16_HI",
        opcode: 33,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "FLAT_LOAD_SBYTE_D16",
        opcode: 34,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "FLAT_LOAD_SBYTE_D16_HI",
        opcode: 35,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "FLAT_LOAD_SHORT_D16",
        opcode: 36,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "FLAT_LOAD_SHORT_D16_HI",
        opcode: 37,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "FLAT_ATOMIC_SWAP",
        opcode: 48,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "FLAT_ATOMIC_CMPSWAP",
        opcode: 49,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "FLAT_ATOMIC_ADD",
        opcode: 50,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "FLAT_ATOMIC_SUB",
        opcode: 51,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "FLAT_ATOMIC_SMIN",
        opcode: 53,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "FLAT_ATOMIC_UMIN",
        opcode: 54,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "FLAT_ATOMIC_SMAX",
        opcode: 55,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "FLAT_ATOMIC_UMAX",
        opcode: 56,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "FLAT_ATOMIC_AND",
        opcode: 57,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "FLAT_ATOMIC_OR",
        opcode: 58,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "FLAT_ATOMIC_XOR",
        opcode: 59,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "FLAT_ATOMIC_INC",
        opcode: 60,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "FLAT_ATOMIC_DEC",
        opcode: 61,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "FLAT_ATOMIC_FCMPSWAP",
        opcode: 62,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "FLAT_ATOMIC_FMIN",
        opcode: 63,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "FLAT_ATOMIC_FMAX",
        opcode: 64,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "FLAT_ATOMIC_SWAP_X2",
        opcode: 80,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "FLAT_ATOMIC_CMPSWAP_X2",
        opcode: 81,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "FLAT_ATOMIC_ADD_X2",
        opcode: 82,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "FLAT_ATOMIC_SUB_X2",
        opcode: 83,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "FLAT_ATOMIC_SMIN_X2",
        opcode: 85,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "FLAT_ATOMIC_UMIN_X2",
        opcode: 86,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "FLAT_ATOMIC_SMAX_X2",
        opcode: 87,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "FLAT_ATOMIC_UMAX_X2",
        opcode: 88,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "FLAT_ATOMIC_AND_X2",
        opcode: 89,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "FLAT_ATOMIC_OR_X2",
        opcode: 90,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "FLAT_ATOMIC_XOR_X2",
        opcode: 91,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "FLAT_ATOMIC_INC_X2",
        opcode: 92,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "FLAT_ATOMIC_DEC_X2",
        opcode: 93,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "FLAT_ATOMIC_FCMPSWAP_X2",
        opcode: 94,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "FLAT_ATOMIC_FMIN_X2",
        opcode: 95,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "FLAT_ATOMIC_FMAX_X2",
        opcode: 96,
        is_branch: false,
        is_terminator: false,
    },
];

/// Look up an instruction by opcode.
#[must_use]
pub fn lookup(opcode: u16) -> Option<&'static InstrEntry> {
    TABLE.iter().find(|e| e.opcode == opcode)
}
