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

/// ENC_SMEM encoding fields (64 bits).
pub mod fields {
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
pub const TABLE: &[InstrEntry] = &[
    InstrEntry {
        name: "S_LOAD_DWORD",
        opcode: 0,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_LOAD_DWORDX2",
        opcode: 1,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_LOAD_DWORDX4",
        opcode: 2,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_LOAD_DWORDX8",
        opcode: 3,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_LOAD_DWORDX16",
        opcode: 4,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_BUFFER_LOAD_DWORD",
        opcode: 8,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_BUFFER_LOAD_DWORDX2",
        opcode: 9,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_BUFFER_LOAD_DWORDX4",
        opcode: 10,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_BUFFER_LOAD_DWORDX8",
        opcode: 11,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_BUFFER_LOAD_DWORDX16",
        opcode: 12,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_GL1_INV",
        opcode: 31,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_DCACHE_INV",
        opcode: 32,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_MEMTIME",
        opcode: 36,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_MEMREALTIME",
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
