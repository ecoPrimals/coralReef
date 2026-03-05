// SPDX-License-Identifier: AGPL-3.0-only
//! Stub for `compiler::nir` — NIR intermediate representation types.
//!
//! **Legacy**: `from_nir` is disabled; coralNak is evolving to SPIR-V via naga.
//! These types are dead code until removed.
//!
//! The original NAK's `from_nir.rs` module (~3700 lines) translates NIR to
//! NAK IR.  In coralNak, this will be replaced entirely by a naga-based
//! SPIR-V/WGSL frontend, making these NIR types unnecessary long-term.
//!
//! ## Evolution Plan
//!
//! 1. **Phase 1 (current)**: Empty stubs so `from_nir.rs` can parse
//! 2. **Phase 2**: Replace `from_nir.rs` with `from_spirv.rs` (naga → coral-nak IR)
//! 3. **Phase 3**: Delete NIR stubs entirely

#![allow(non_camel_case_types, dead_code)]

/// NIR shader (top-level module).
pub struct nir_shader;

/// NIR SSA definition.
pub struct nir_def;

/// NIR basic block.
pub struct nir_block;

/// NIR source operand.
pub struct nir_src;

/// NIR phi instruction.
pub struct nir_phi_instr;

/// NIR ALU instruction.
pub struct nir_alu_instr;

/// NIR intrinsic instruction.
pub struct nir_intrinsic_instr;

/// NIR texture instruction.
pub struct nir_tex_instr;

/// NIR load-const instruction.
pub struct nir_load_const_instr;

/// NIR jump instruction.
pub struct nir_jump_instr;

/// NIR if-then-else.
pub struct nir_if;
