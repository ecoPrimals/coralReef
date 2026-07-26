// SPDX-License-Identifier: AGPL-3.0-or-later
//! PTX code emitter from naga Module for SM100+ (Blackwell).
//!
//! Emits NVIDIA PTX (Parallel Thread Execution) text from a naga compute
//! shader `Module`. The CUDA driver JIT-compiles PTX to native SASS,
//! bypassing the cubin ELF format that SM120 currently rejects.
//!
//! ## Parameter convention
//!
//! Per storage buffer binding (ordered by `(group, binding)`):
//!   - `.param .u64 _bufN_ptr`  — device pointer
//!   - `.param .u64 _bufN_size` — byte length (for `arrayLength`)
//!
//! ## Builtin mapping
//!
//! | WGSL builtin            | PTX special register           |
//! |-------------------------|-------------------------------|
//! | `global_invocation_id`  | `%tid + %ctaid * %ntid`       |
//! | `local_invocation_id`   | `%tid`                        |
//! | `workgroup_id`          | `%ctaid`                      |
//! | `num_workgroups`        | `%nctaid`                     |
//! | `local_invocation_index`| `tid.x + tid.y*ntid.x + ...` |

mod builtins;
mod emitter;
mod expr_arith;
mod expr_cast;
mod expr_eval;
mod expr_image;
mod expr_misc;
pub mod gemm;
mod math;
mod math_ext;
mod math_ext_trig;
mod math_matrix;
mod math_pack;
mod pointers;
mod ray_query;
mod statements;
mod subgroup;
mod types;

use std::collections::HashMap;

use crate::backend::{BinaryFormat, CompilationInfo, CompiledBinary};
use crate::error::CompileError;

use types::{BufferBinding, PtxVal, SharedVar, SurfaceBinding, TextureBinding};

/// Extract a required math function argument from an `Option`.
///
/// Returns `CompileError::NotImplemented` when the argument is missing,
/// with a message like `"min without arg1"`.
fn require_math_arg<'a>(
    arg: Option<&'a PtxVal>,
    func: &str,
    index: u8,
) -> Result<&'a PtxVal, CompileError> {
    arg.ok_or_else(|| CompileError::NotImplemented(format!("{func} without arg{index}").into()))
}

/// Compile WGSL source directly to PTX for SM100+ targets.
///
/// Parses WGSL → naga Module, then emits PTX text. Returns a
/// `CompiledBinary` with `format: BinaryFormat::Ptx`.
pub fn emit_compute_ptx(wgsl_source: &str, sm: u8) -> Result<CompiledBinary, CompileError> {
    let module = crate::codegen::naga_translate::parse_wgsl(wgsl_source)?;
    emit_compute_ptx_module(&module, sm, None)
}

/// Emit PTX from a pre-parsed `naga::Module` for SM100+ targets.
///
/// Accepts a module directly, skipping the WGSL parse step.
/// Used by `compile_module_full` for the Blackwell PTX path.
///
/// If `entry_point_name` is `Some`, compiles that specific entry point.
/// If `None`, uses the first compute-stage entry point in the module.
pub fn emit_compute_ptx_module(
    module: &naga::Module,
    sm: u8,
    entry_point_name: Option<&str>,
) -> Result<CompiledBinary, CompileError> {
    let ep_index = match entry_point_name {
        Some(name) => module
            .entry_points
            .iter()
            .position(|ep| ep.name == name)
            .ok_or_else(|| {
                CompileError::InvalidInput(
                    format!("entry point '{name}' not found in module").into(),
                )
            })?,
        None => module
            .entry_points
            .iter()
            .position(|ep| ep.stage == naga::ShaderStage::Compute)
            .ok_or_else(|| CompileError::InvalidInput("no compute entry point".into()))?,
    };

    let ep = &module.entry_points[ep_index];

    let mut emitter = PtxEmitter::new(module, ep, sm);
    let ptx = emitter.emit()?;

    let ws = ep.workgroup_size;
    Ok(CompiledBinary {
        binary: ptx.into_bytes(),
        info: CompilationInfo {
            gpr_count: emitter.r32_next.max(emitter.rd64_next),
            instr_count: 0,
            shared_mem_bytes: emitter.shared_mem_bytes,
            barrier_count: emitter.barrier_count,
            local_size: [ws[0], ws[1], ws[2]],
            local_mem_bytes: 0,
        },
        format: BinaryFormat::Ptx,
    })
}

pub struct PtxEmitter<'a> {
    pub(crate) module: &'a naga::Module,
    pub(crate) func: &'a naga::Function,
    pub(crate) sm: u8,
    pub(crate) workgroup_size: [u32; 3],

    pub(crate) bindings: Vec<BufferBinding>,
    pub(crate) surfaces: Vec<SurfaceBinding>,
    pub(crate) textures: Vec<TextureBinding>,
    pub(crate) shared_vars: Vec<SharedVar>,

    pub(crate) r32_next: u32,
    pub(crate) rd64_next: u32,
    pub(crate) pred_next: u32,
    pub(crate) label_next: u32,

    pub(crate) values: HashMap<naga::Handle<naga::Expression>, PtxVal>,
    pub(crate) locals: HashMap<naga::Handle<naga::LocalVariable>, PtxVal>,
    pub(crate) gv_ptr_regs: HashMap<naga::Handle<naga::GlobalVariable>, (PtxVal, usize)>,

    pub(crate) body: String,
    pub(crate) shared_mem_bytes: u32,
    pub(crate) barrier_count: u32,

    /// When > 0, we're inside an inlined function call. `Return` should
    /// store its value rather than emitting `ret;`.
    pub(crate) inline_depth: u32,
    /// Holds the return value of the most recently completed inlined call.
    pub(crate) inline_return_val: Option<PtxVal>,

    /// Per ray-query opaque state, keyed by the expression handle that holds
    /// the `TypeInner::RayQuery` local/variable.
    pub(crate) ray_queries: HashMap<naga::Handle<naga::Expression>, types::RayQueryState>,

    /// Label to branch to on `break` (top = innermost loop exit).
    pub(crate) loop_break_label: Vec<String>,
    /// Label to branch to on `continue` (top = innermost loop header).
    pub(crate) loop_continue_label: Vec<String>,
}

#[cfg(test)]
mod tests_core;
#[cfg(test)]
mod tests_image;
#[cfg(test)]
mod tests_math_ext;
#[cfg(test)]
mod tests_math_pack;
