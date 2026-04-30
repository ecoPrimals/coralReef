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
mod expr_misc;
mod math;
mod pointers;
mod statements;
mod types;

use std::collections::HashMap;

use crate::backend::{BinaryFormat, CompilationInfo, CompiledBinary};
use crate::error::CompileError;

use types::{BufferBinding, PtxVal, SharedVar};

/// Compile WGSL source directly to PTX for SM100+ targets.
///
/// Parses WGSL → naga Module, then emits PTX text. Returns a
/// `CompiledBinary` with `format: BinaryFormat::Ptx`.
pub fn emit_compute_ptx(wgsl_source: &str, sm: u8) -> Result<CompiledBinary, CompileError> {
    let module = crate::codegen::naga_translate::parse_wgsl(wgsl_source)?;

    let ep_index = module
        .entry_points
        .iter()
        .position(|ep| ep.stage == naga::ShaderStage::Compute)
        .ok_or_else(|| CompileError::InvalidInput("no compute entry point".into()))?;

    let ep = &module.entry_points[ep_index];

    let mut emitter = PtxEmitter::new(&module, ep, sm);
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ptx_write_42() {
        let wgsl = r"
@group(0) @binding(0)
var<storage, read_write> out: array<u32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    out[gid.x] = 42u;
}
";
        let result = emit_compute_ptx(wgsl, 120).expect("compile");
        let ptx = std::str::from_utf8(&result.binary).expect("PTX output is valid UTF-8");
        assert!(ptx.contains(".version 8.7"));
        assert!(ptx.contains(".target sm_120"));
        assert!(ptx.contains("main_kernel"));
        assert!(ptx.contains("_buf0_ptr"));
        assert!(ptx.contains("st.global"));
        assert!(ptx.contains("42"));
        assert_eq!(result.format, BinaryFormat::Ptx);
        assert_eq!(result.info.local_size, [64, 1, 1]);
    }

    #[test]
    fn ptx_copy_ab() {
        let wgsl = r"
@group(0) @binding(0)
var<storage, read> src: array<u32>;

@group(0) @binding(1)
var<storage, read_write> dst: array<u32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    dst[gid.x] = src[gid.x];
}
";
        let result = emit_compute_ptx(wgsl, 120).expect("compile");
        let ptx = std::str::from_utf8(&result.binary).expect("PTX output is valid UTF-8");
        assert!(ptx.contains("_buf0_ptr"));
        assert!(ptx.contains("_buf1_ptr"));
        assert!(ptx.contains("ld.global"));
        assert!(ptx.contains("st.global"));
    }

    #[test]
    fn ptx_array_length() {
        let wgsl = r"
@group(0) @binding(0)
var<storage, read_write> buf: array<u32>;

@compute @workgroup_size(1)
fn main() {
    let len = arrayLength(&buf);
    buf[0] = len;
}
";
        let result = emit_compute_ptx(wgsl, 120).expect("compile");
        let ptx = std::str::from_utf8(&result.binary).expect("PTX output is valid UTF-8");
        assert!(ptx.contains("_buf0_size"));
        assert!(ptx.contains("shr.u64") || ptx.contains("div.u64"));
    }

    #[test]
    fn ptx_num_workgroups() {
        let wgsl = r"
@group(0) @binding(0)
var<storage, read_write> out: array<u32>;

@compute @workgroup_size(1)
fn main(@builtin(num_workgroups) nwg: vec3<u32>) {
    out[0] = nwg.x;
    out[1] = nwg.y;
    out[2] = nwg.z;
}
";
        let result = emit_compute_ptx(wgsl, 120).expect("compile");
        let ptx = std::str::from_utf8(&result.binary).expect("PTX output is valid UTF-8");
        assert!(ptx.contains("%nctaid.x"));
        assert!(ptx.contains("%nctaid.y"));
        assert!(ptx.contains("%nctaid.z"));
    }
}
