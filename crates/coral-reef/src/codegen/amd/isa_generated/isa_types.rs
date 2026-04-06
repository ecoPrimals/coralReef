// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright © 2026 ecoPrimals
//! AUTO-GENERATED from AMD RDNA2 ISA XML specification.
//!
//! Source: specs/amd/amdgpu_isa_rdna2.xml (MIT license, AMD GPUOpen)
//! Generator: tools/amd-isa-gen (pure Rust, sovereign toolchain)
//!
//! DO NOT EDIT BY HAND. Regenerate with:
//!   cargo run -p amd-isa-gen

/// Bit field within an encoding format.
#[derive(Debug, Clone, Copy)]
pub struct BitField {
    /// Bit offset within the instruction word(s).
    pub offset: u32,
    /// Number of bits.
    pub width: u32,
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
