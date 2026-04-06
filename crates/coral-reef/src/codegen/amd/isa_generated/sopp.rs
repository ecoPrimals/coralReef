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

/// ENC_SOPP encoding fields (32 bits).
pub mod fields {
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
pub const TABLE: &[InstrEntry] = &[
    InstrEntry {
        name: "S_NOP",
        opcode: 0,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_ENDPGM",
        opcode: 1,
        is_branch: false,
        is_terminator: true,
    },
    InstrEntry {
        name: "S_BRANCH",
        opcode: 2,
        is_branch: true,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_WAKEUP",
        opcode: 3,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_CBRANCH_SCC0",
        opcode: 4,
        is_branch: true,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_CBRANCH_SCC1",
        opcode: 5,
        is_branch: true,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_CBRANCH_VCCZ",
        opcode: 6,
        is_branch: true,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_CBRANCH_VCCNZ",
        opcode: 7,
        is_branch: true,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_CBRANCH_EXECZ",
        opcode: 8,
        is_branch: true,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_CBRANCH_EXECNZ",
        opcode: 9,
        is_branch: true,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_BARRIER",
        opcode: 10,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_SETKILL",
        opcode: 11,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_WAITCNT",
        opcode: 12,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_SETHALT",
        opcode: 13,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_SLEEP",
        opcode: 14,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_SETPRIO",
        opcode: 15,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_SENDMSG",
        opcode: 16,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_SENDMSGHALT",
        opcode: 17,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_TRAP",
        opcode: 18,
        is_branch: true,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_ICACHE_INV",
        opcode: 19,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_INCPERFLEVEL",
        opcode: 20,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_DECPERFLEVEL",
        opcode: 21,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_TTRACEDATA",
        opcode: 22,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_CBRANCH_CDBGSYS",
        opcode: 23,
        is_branch: true,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_CBRANCH_CDBGUSER",
        opcode: 24,
        is_branch: true,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_CBRANCH_CDBGSYS_OR_USER",
        opcode: 25,
        is_branch: true,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_CBRANCH_CDBGSYS_AND_USER",
        opcode: 26,
        is_branch: true,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_ENDPGM_SAVED",
        opcode: 27,
        is_branch: false,
        is_terminator: true,
    },
    InstrEntry {
        name: "S_ENDPGM_ORDERED_PS_DONE",
        opcode: 30,
        is_branch: false,
        is_terminator: true,
    },
    InstrEntry {
        name: "S_CODE_END",
        opcode: 31,
        is_branch: true,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_INST_PREFETCH",
        opcode: 32,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_CLAUSE",
        opcode: 33,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_WAITCNT_DEPCTR",
        opcode: 35,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_ROUND_MODE",
        opcode: 36,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_DENORM_MODE",
        opcode: 37,
        is_branch: false,
        is_terminator: false,
    },
    InstrEntry {
        name: "S_TTRACEDATA_IMM",
        opcode: 40,
        is_branch: false,
        is_terminator: false,
    },
];

/// Look up an instruction by opcode.
#[must_use]
pub fn lookup(opcode: u16) -> Option<&'static InstrEntry> {
    TABLE.iter().find(|e| e.opcode == opcode)
}
