// SPDX-License-Identifier: AGPL-3.0-only
//! Stub for `nak_bindings` — NAK-specific C binding types.
//!
//! **Legacy**: `from_nir`, `qmd`, `hw_runner` are disabled; coralNak is evolving
//! to SPIR-V via naga. These types are dead code until removed.
//!
//! These are bindgen-generated types from NAK C headers.  They define
//! the compiler context, shader binary, and various configuration structs.
//!
//! ## Key types to implement
//!
//! - `nak_compiler` — compiler instance
//! - `nak_shader_bin` — compiled shader binary
//! - `nak_shader_info` — shader info for the Nouveau driver
//! - `nak_fs_key` — fragment shader variant key
//! - `nak_range` — register range descriptor
//! - Various enums for memory types, interpolation modes, etc.

#![allow(non_camel_case_types, dead_code)]

/// NAK compiler instance.
#[derive(Debug)]
pub struct nak_compiler {
    /// Shader model version.
    pub sm: u32,
}

/// Compiled shader binary.
#[derive(Debug)]
pub struct nak_shader_bin {
    /// Binary code.
    pub code: Vec<u8>,
    /// Code size in bytes.
    pub code_size: usize,
}

/// Shader info for driver.
#[derive(Debug, Default)]
pub struct nak_shader_info {
    /// Shader stage.
    pub stage: u32,
    /// Number of GPRs used.
    pub num_gprs: u32,
    /// Number of instructions.
    pub num_instrs: u32,
    /// Number of barriers.
    pub num_barriers: u8,
    /// Shared memory size in bytes.
    pub slm_size: u32,
    /// CRS (call/return stack) size.
    pub crs_size: u32,
    /// Whether the shader writes global memory.
    pub writes_global_mem: bool,
    /// Whether the shader uses fp64.
    pub uses_fp64: bool,
    /// SPH header.
    pub hdr: [u32; 32],
}

/// Fragment shader variant key.
#[derive(Debug, Default)]
pub struct nak_fs_key {
    /// Placeholder.
    pub placeholder: u32,
}

/// Register range.
#[derive(Debug, Clone, Copy)]
pub struct nak_range {
    /// Start register.
    pub start: u32,
    /// Number of registers.
    pub count: u32,
}
