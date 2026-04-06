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

/// ENC_MTBUF encoding fields (64 bits).
pub mod fields {
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
pub const TABLE: &[InstrEntry] = &[
    InstrEntry {
        name: "TBUFFER_LOAD_FORMAT_X",
        opcode: 0,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "TBUFFER_LOAD_FORMAT_XY",
        opcode: 1,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "TBUFFER_LOAD_FORMAT_XYZ",
        opcode: 2,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "TBUFFER_LOAD_FORMAT_XYZW",
        opcode: 3,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "TBUFFER_STORE_FORMAT_X",
        opcode: 4,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "TBUFFER_STORE_FORMAT_XY",
        opcode: 5,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "TBUFFER_STORE_FORMAT_XYZ",
        opcode: 6,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "TBUFFER_STORE_FORMAT_XYZW",
        opcode: 7,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "TBUFFER_LOAD_FORMAT_D16_X",
        opcode: 8,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "TBUFFER_LOAD_FORMAT_D16_XY",
        opcode: 9,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "TBUFFER_LOAD_FORMAT_D16_XYZ",
        opcode: 10,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "TBUFFER_LOAD_FORMAT_D16_XYZW",
        opcode: 11,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "TBUFFER_STORE_FORMAT_D16_X",
        opcode: 12,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "TBUFFER_STORE_FORMAT_D16_XY",
        opcode: 13,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "TBUFFER_STORE_FORMAT_D16_XYZ",
        opcode: 14,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "TBUFFER_STORE_FORMAT_D16_XYZW",
        opcode: 15,
        is_branch: false,
        is_terminator: false,
    },
];

/// Look up an instruction by opcode.
#[must_use]
pub fn lookup(opcode: u16) -> Option<&'static InstrEntry> {
    TABLE.iter().find(|e| e.opcode == opcode)
}
