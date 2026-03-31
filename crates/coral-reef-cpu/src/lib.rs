// SPDX-License-Identifier: AGPL-3.0-only
#![forbid(unsafe_code)]
#![warn(missing_docs)]
//! CPU compilation backend and shader validation for coralReef.
//!
//! Provides two execution paths for WGSL compute shaders on the CPU:
//!
//! - **Naga interpreter** ([`interpret`]) — walks a `naga::Module` directly.
//! - **`CoralIR` reference executor** ([`coral_ir_exec`]) — walks optimized `CoralIR`
//!   ops, mirroring the exact semantics the GPU and JIT backends execute.
//!
//! Both paths share the tolerance-based validation engine ([`validate`]) for
//! comparing outputs within numerical tolerance.
//!
//! Wire types in [`types`] are shared with `coralreef-core` for IPC.

pub mod coral_ir_exec;
pub mod interpret;
pub mod types;
pub mod validate;

pub use coral_ir_exec::execute_coral_ir;
pub use interpret::execute_cpu;
pub use types::{
    BindingData, CompileCpuRequest, DualPathResult, ExecuteCpuRequest, ExecuteCpuResponse,
    ExpectedBinding, Mismatch, Tolerance, ValidateRequest, ValidateResponse,
};
pub use validate::validate;

/// Extract workgroup size from a compiled `CoralIR` shader.
///
/// Shared utility used by both the `CoralIR` interpreter and the JIT backend
/// to read the declared `@workgroup_size` from the shader metadata.
#[must_use]
pub fn extract_workgroup_size(shader: &coral_reef::codegen::ir::Shader<'_>) -> [u32; 3] {
    if let coral_reef::codegen::ir::ShaderStageInfo::Compute(cs) = &shader.info.stage {
        [
            u32::from(cs.local_size[0]),
            u32::from(cs.local_size[1]),
            u32::from(cs.local_size[2]),
        ]
    } else {
        [1, 1, 1]
    }
}
