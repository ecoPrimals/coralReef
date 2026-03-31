// SPDX-License-Identifier: AGPL-3.0-only
#![deny(unsafe_code)]
#![warn(missing_docs)]
//! # `coral-reef-jit` — Cranelift JIT Backend for `CoralIR`
//!
//! Compiles optimized `CoralIR` (from `coral-reef`) to native x86-64/aarch64 code
//! via Cranelift, enabling CPU-side execution of GPU compute shaders.
//!
//! ## Purpose
//!
//! This crate is the "Path B" in the dual-path validation chain:
//!
//! - **Path A** (Naga interpreter): `coral-reef-cpu` — reference oracle
//! - **Path B** (Cranelift JIT): WGSL → naga → `CoralIR` → optimize → Cranelift → execute
//!
//! Comparing results from both paths proves that the optimization pipeline
//! preserves shader semantics.
//!
//! ## Usage
//!
//! ```rust,ignore
//! use coral_reef_jit::execute_jit;
//! use coral_reef_cpu::types::ExecuteCpuRequest;
//!
//! let request = ExecuteCpuRequest {
//!     wgsl_source: "@compute @workgroup_size(1) fn main() {}".into(),
//!     entry_point: None,
//!     workgroups: [1, 1, 1],
//!     bindings: vec![],
//!     uniforms: vec![],
//! };
//! let response = execute_jit(&request).unwrap();
//! ```

pub mod builtins;
pub mod cache;
pub mod cmp_codes;
pub mod error;
pub mod memory;
pub mod runtime;
pub mod sovereign;
pub mod translate;

use coral_reef::CompileOptions;
use coral_reef::gpu_arch::{GpuTarget, NvArch};
use coral_reef_cpu::types::{ExecuteCpuRequest, ExecuteCpuResponse};
use tracing::instrument;

use error::JitError;
use memory::BindingBuffers;
use sovereign::translate_and_compile_sovereign;
use translate::{CompiledKernel, KernelFn};

/// Compile a WGSL shader to a JIT kernel without executing it.
///
/// Separating compilation from execution enables caching in the progressive
/// trust model — compile once, execute many times.
///
/// # Errors
///
/// Returns [`JitError`] if WGSL parsing, `CoralIR` compilation, or Cranelift
/// code generation fails.
pub fn compile_to_kernel(request: &ExecuteCpuRequest) -> Result<CompiledKernel, JitError> {
    let options = CompileOptions {
        target: GpuTarget::Nvidia(NvArch::Sm86),
        ..Default::default()
    };
    let sm = coral_reef::shader_model_for(options.target)
        .map_err(|e| JitError::Compilation(e.to_string()))?;

    let shader = coral_reef::compile_wgsl_to_ir(&request.wgsl_source, &options, sm.as_ref())
        .map_err(|e| JitError::Compilation(e.to_string()))?;

    translate_and_compile_sovereign(&shader)
}

/// Execute a pre-compiled kernel against the given request's bindings.
///
/// # Errors
///
/// Returns [`JitError`] if WGSL source cannot be compiled to `CoralIR` (for
/// workgroup size extraction).
pub fn execute_kernel(
    compiled: &CompiledKernel,
    request: &ExecuteCpuRequest,
) -> Result<ExecuteCpuResponse, JitError> {
    let start = std::time::Instant::now();

    let options = CompileOptions {
        target: GpuTarget::Nvidia(NvArch::Sm86),
        ..Default::default()
    };
    let sm = coral_reef::shader_model_for(options.target)
        .map_err(|e| JitError::Compilation(e.to_string()))?;
    let shader = coral_reef::compile_wgsl_to_ir(&request.wgsl_source, &options, sm.as_ref())
        .map_err(|e| JitError::Compilation(e.to_string()))?;

    let mut buffers = BindingBuffers::from_bindings(&request.bindings);
    let mut ptrs = buffers.as_mut_ptrs();

    let workgroup_size = coral_reef_cpu::extract_workgroup_size(&shader);
    let [wg_x, wg_y, wg_z] = request.workgroups;

    dispatch_workgroups(compiled, &mut ptrs, [wg_x, wg_y, wg_z], workgroup_size);

    #[expect(
        clippy::cast_possible_truncation,
        reason = "elapsed nanoseconds will not exceed u64 in practice"
    )]
    let elapsed_ns = start.elapsed().as_nanos() as u64;

    let output_bindings = buffers.into_binding_data(&request.bindings);

    Ok(ExecuteCpuResponse {
        bindings: output_bindings,
        execution_time_ns: elapsed_ns,
        strategy_used: None,
        cache_hit: false,
        revalidated: false,
    })
}

/// Execute a WGSL compute shader on the CPU via the Cranelift JIT backend.
///
/// This is the "Path B" execution entry point:
/// 1. Parse WGSL → naga → `CoralIR`
/// 2. Run all architecture-independent optimization passes
/// 3. Translate optimized `CoralIR` → Cranelift CLIF instructions
/// 4. JIT compile to native code
/// 5. Dispatch workgroups, invoking the JIT'd kernel for each invocation
/// 6. Return modified buffer bindings
///
/// # Errors
///
/// Returns [`JitError`] if compilation or execution fails.
#[instrument(skip_all, fields(bindings = request.bindings.len(), workgroups = ?request.workgroups))]
pub fn execute_jit(request: &ExecuteCpuRequest) -> Result<ExecuteCpuResponse, JitError> {
    let compiled = compile_to_kernel(request)?;
    execute_kernel(&compiled, request)
}

/// Dispatch all workgroups, invoking the JIT'd kernel for each invocation.
///
/// The kernel function pointer is resolved once and reused across all invocations
/// to avoid repeated transmute overhead.
fn dispatch_workgroups(
    compiled: &CompiledKernel,
    ptrs: &mut [*mut u8],
    num_workgroups: [u32; 3],
    workgroup_size: [u32; 3],
) {
    // SAFETY: The JIT'd function was compiled by Cranelift from verified CoralIR.
    // The binding pointers are valid for the duration of this call (owned by
    // BindingBuffers). The function signature matches KernelFn exactly. We hoist
    // the transmute outside the loop so it happens once.
    #[expect(unsafe_code, reason = "JIT function pointer invocation")]
    let kernel: KernelFn = unsafe { compiled.as_fn() };

    let bindings_ptr = ptrs.as_mut_ptr();

    for wg_z in 0..num_workgroups[2] {
        for wg_y in 0..num_workgroups[1] {
            for wg_x in 0..num_workgroups[0] {
                for tid_z in 0..workgroup_size[2] {
                    for tid_y in 0..workgroup_size[1] {
                        for tid_x in 0..workgroup_size[0] {
                            let gid_x = wg_x * workgroup_size[0] + tid_x;
                            let gid_y = wg_y * workgroup_size[1] + tid_y;
                            let gid_z = wg_z * workgroup_size[2] + tid_z;

                            #[expect(unsafe_code, reason = "JIT function pointer call")]
                            unsafe {
                                kernel(
                                    bindings_ptr,
                                    gid_x,
                                    gid_y,
                                    gid_z,
                                    wg_x,
                                    wg_y,
                                    wg_z,
                                    tid_x,
                                    tid_y,
                                    tid_z,
                                    num_workgroups[0],
                                    num_workgroups[1],
                                    num_workgroups[2],
                                    workgroup_size[0],
                                    workgroup_size[1],
                                    workgroup_size[2],
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trivial_empty_shader_executes() {
        let request = ExecuteCpuRequest {
            wgsl_source: "@compute @workgroup_size(1) fn main() {}".into(),
            entry_point: None,
            workgroups: [1, 1, 1],
            bindings: vec![],
            uniforms: vec![],
            strategy: coral_reef_cpu::types::ExecutionStrategy::Jit,
        };
        let result = execute_jit(&request);
        assert!(result.is_ok(), "trivial shader should execute: {result:?}");
    }

    #[test]
    fn workgroup_size_extraction_defaults_to_one() {
        let options = CompileOptions {
            target: GpuTarget::Nvidia(NvArch::Sm86),
            ..Default::default()
        };
        let sm = coral_reef::shader_model_for(options.target).unwrap();
        let shader = coral_reef::compile_wgsl_to_ir(
            "@compute @workgroup_size(4, 2, 1) fn main() {}",
            &options,
            sm.as_ref(),
        )
        .unwrap();
        let size = coral_reef_cpu::extract_workgroup_size(&shader);
        assert_eq!(size, [4, 2, 1]);
    }
}
