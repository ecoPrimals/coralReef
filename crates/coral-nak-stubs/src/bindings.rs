// SPDX-License-Identifier: AGPL-3.0-only
//! Stub for `compiler::bindings::*` — Mesa C FFI struct replacements.
//!
//! **Legacy**: `from_nir`, `qmd`, `hw_runner` are disabled; coralNak is evolving
//! to SPIR-V via naga. These types are dead code until removed.
//!
//! The original NAK imports types like `nir_shader`, `shader_info`, `nv_device_info`,
//! and Mesa shader stage enums from bindgen-generated C headers.
//!
//! ## Types to implement
//!
//! - `nv_device_info` — GPU device information
//! - `shader_info` — Shader metadata (inputs, outputs, etc.)
//! - `MESA_SHADER_*` — Shader stage constants
//! - `nir_shader_compiler_options` — NIR lowering configuration

#![allow(non_camel_case_types, non_upper_case_globals, dead_code)]

/// Shader stage identifiers (replaces `MESA_SHADER_*`).
pub const MESA_SHADER_VERTEX: u32 = 0;
/// Fragment shader stage.
pub const MESA_SHADER_FRAGMENT: u32 = 4;
/// Compute shader stage.
pub const MESA_SHADER_COMPUTE: u32 = 5;
/// Tessellation control shader stage.
pub const MESA_SHADER_TESS_CTRL: u32 = 1;
/// Tessellation evaluation shader stage.
pub const MESA_SHADER_TESS_EVAL: u32 = 2;
/// Geometry shader stage.
pub const MESA_SHADER_GEOMETRY: u32 = 3;

/// GPU device information (replaces `nv_device_info`).
#[derive(Debug, Clone)]
pub struct nv_device_info {
    /// GPU chipset (e.g. 0x170 for GA100).
    pub chipset: u32,
    /// Shader Model version (e.g. 70 for Volta).
    pub sm: u32,
    /// Maximum warps per SM.
    pub max_warps_per_mp: u32,
    /// Number of GPRs per SM.
    pub gpr_alloc_gran: u32,
    /// Compute class (e.g. the device class for compute dispatch).
    pub cls_compute: u32,
    /// Warp size (typically 32).
    pub warp_size: u32,
    /// Shared memory per SM.
    pub shared_memory_per_mp: u32,
}

/// Shader info — metadata about a compiled shader.
#[derive(Debug, Clone, Default)]
pub struct shader_info {
    /// Shader stage.
    pub stage: u32,
    /// Shader name (for debugging).
    pub name: String,
}

/// Tessellation shader info sub-struct.
#[derive(Debug, Clone, Default)]
pub struct shader_info__bindgen_ty_1__bindgen_ty_5 {
    /// Stub placeholder.
    pub placeholder: u32,
}
