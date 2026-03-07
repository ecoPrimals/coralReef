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

/// ENC_FLAT_SCRATCH encoding fields (64 bits).
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
pub const TABLE: &[InstrEntry] = &[
    InstrEntry {
        name: "SCRATCH_LOAD_UBYTE",
        opcode: 8,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "SCRATCH_LOAD_SBYTE",
        opcode: 9,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "SCRATCH_LOAD_USHORT",
        opcode: 10,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "SCRATCH_LOAD_SSHORT",
        opcode: 11,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "SCRATCH_LOAD_DWORD",
        opcode: 12,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "SCRATCH_LOAD_DWORDX2",
        opcode: 13,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "SCRATCH_LOAD_DWORDX4",
        opcode: 14,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "SCRATCH_LOAD_DWORDX3",
        opcode: 15,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "SCRATCH_STORE_BYTE",
        opcode: 24,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "SCRATCH_STORE_BYTE_D16_HI",
        opcode: 25,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "SCRATCH_STORE_SHORT",
        opcode: 26,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "SCRATCH_STORE_SHORT_D16_HI",
        opcode: 27,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "SCRATCH_STORE_DWORD",
        opcode: 28,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "SCRATCH_STORE_DWORDX2",
        opcode: 29,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "SCRATCH_STORE_DWORDX4",
        opcode: 30,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "SCRATCH_STORE_DWORDX3",
        opcode: 31,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "SCRATCH_LOAD_UBYTE_D16",
        opcode: 32,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "SCRATCH_LOAD_UBYTE_D16_HI",
        opcode: 33,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "SCRATCH_LOAD_SBYTE_D16",
        opcode: 34,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "SCRATCH_LOAD_SBYTE_D16_HI",
        opcode: 35,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "SCRATCH_LOAD_SHORT_D16",
        opcode: 36,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "SCRATCH_LOAD_SHORT_D16_HI",
        opcode: 37,
        is_branch: false,
        is_terminator: false,
    },
];

/// Look up an instruction by opcode.
#[must_use]
pub fn lookup(opcode: u16) -> Option<&'static InstrEntry> {
    TABLE.iter().find(|e| e.opcode == opcode)
}
