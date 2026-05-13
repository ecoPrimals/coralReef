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

    #[test]
    fn ptx_switch_statement() {
        let wgsl = r"
@group(0) @binding(0)
var<storage, read_write> out: array<u32>;

@compute @workgroup_size(1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    switch gid.x {
        case 0u: { out[0] = 10u; }
        case 1u: { out[0] = 20u; }
        default: { out[0] = 99u; }
    }
}
";
        let result = emit_compute_ptx(wgsl, 120).expect("compile");
        let ptx = std::str::from_utf8(&result.binary).expect("PTX output is valid UTF-8");
        assert!(ptx.contains("setp.eq.s32"));
        assert!(ptx.contains("10"));
        assert!(ptx.contains("20"));
        assert!(ptx.contains("99"));
    }

    #[test]
    fn ptx_math_sqrt_exp2_log2() {
        let wgsl = r"
@group(0) @binding(0)
var<storage, read_write> buf: array<f32>;

@compute @workgroup_size(1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    buf[0] = sqrt(buf[0]);
    buf[1] = exp2(buf[1]);
    buf[2] = log2(buf[2]);
}
";
        let result = emit_compute_ptx(wgsl, 120).expect("compile");
        let ptx = std::str::from_utf8(&result.binary).expect("PTX output is valid UTF-8");
        assert!(ptx.contains("sqrt.rn"));
        assert!(ptx.contains("ex2.approx"));
        assert!(ptx.contains("lg2.approx"));
    }

    #[test]
    fn ptx_math_pow_exp_log() {
        let wgsl = r"
@group(0) @binding(0)
var<storage, read_write> buf: array<f32>;

@compute @workgroup_size(1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    buf[0] = pow(buf[0], buf[1]);
    buf[2] = exp(buf[2]);
    buf[3] = log(buf[3]);
}
";
        let result = emit_compute_ptx(wgsl, 120).expect("compile");
        let ptx = std::str::from_utf8(&result.binary).expect("PTX output is valid UTF-8");
        assert!(ptx.contains("lg2.approx"));
        assert!(ptx.contains("ex2.approx"));
    }

    #[test]
    fn ptx_math_fma_clamp_abs() {
        let wgsl = r"
@group(0) @binding(0)
var<storage, read_write> buf: array<f32>;

@compute @workgroup_size(1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    buf[0] = fma(buf[0], buf[1], buf[2]);
    buf[3] = clamp(buf[3], 0.0, 1.0);
    buf[4] = abs(buf[4]);
}
";
        let result = emit_compute_ptx(wgsl, 120).expect("compile");
        let ptx = std::str::from_utf8(&result.binary).expect("PTX output is valid UTF-8");
        assert!(ptx.contains("fma.rn"));
        assert!(ptx.contains("abs."));
    }

    #[test]
    fn ptx_math_fract() {
        let wgsl = r"
@group(0) @binding(0)
var<storage, read_write> buf: array<f32>;

@compute @workgroup_size(1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    buf[0] = fract(buf[0]);
}
";
        let result = emit_compute_ptx(wgsl, 120).expect("compile");
        let ptx = std::str::from_utf8(&result.binary).expect("PTX output is valid UTF-8");
        assert!(ptx.contains("cvt.rmi"));
        assert!(ptx.contains("sub."));
    }

    #[test]
    fn ptx_if_else() {
        let wgsl = r"
@group(0) @binding(0)
var<storage, read_write> buf: array<u32>;

@compute @workgroup_size(1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    if gid.x == 0u {
        buf[0] = 1u;
    } else {
        buf[0] = 2u;
    }
}
";
        let result = emit_compute_ptx(wgsl, 120).expect("compile");
        let ptx = std::str::from_utf8(&result.binary).expect("PTX output is valid UTF-8");
        assert!(ptx.contains("setp"));
        assert!(ptx.contains("bra"));
    }

    #[test]
    fn ptx_loop_basic() {
        let wgsl = r"
@group(0) @binding(0)
var<storage, read_write> buf: array<u32>;

@compute @workgroup_size(1)
fn main() {
    var i: u32 = 0u;
    loop {
        if i >= 10u { break; }
        buf[i] = i;
        continuing { i = i + 1u; }
    }
}
";
        let result = emit_compute_ptx(wgsl, 120).expect("compile");
        let ptx = std::str::from_utf8(&result.binary).expect("PTX output is valid UTF-8");
        assert!(ptx.contains("$L"));
        assert!(ptx.contains("bra"));
    }

    #[test]
    fn ptx_atomic_add_global() {
        let wgsl = r"
@group(0) @binding(0)
var<storage, read_write> buf: array<atomic<u32>>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    atomicAdd(&buf[0], 1u);
}
";
        let result = emit_compute_ptx(wgsl, 120).expect("compile");
        let ptx = std::str::from_utf8(&result.binary).expect("PTX output is valid UTF-8");
        assert!(ptx.contains("atom.global.add.u32"), "Expected atom.global.add.u32 in:\n{ptx}");
    }

    #[test]
    fn ptx_atomic_max_shared() {
        let wgsl = r"
var<workgroup> shared_max: atomic<u32>;

@group(0) @binding(0)
var<storage, read_write> buf: array<u32>;

@compute @workgroup_size(64)
fn main(@builtin(local_invocation_id) lid: vec3<u32>) {
    atomicMax(&shared_max, lid.x);
}
";
        let result = emit_compute_ptx(wgsl, 120).expect("compile");
        let ptx = std::str::from_utf8(&result.binary).expect("PTX output is valid UTF-8");
        assert!(ptx.contains("atom.shared.max.u32"), "Expected atom.shared.max.u32 in:\n{ptx}");
    }

    #[test]
    fn ptx_atomic_cas() {
        let wgsl = r"
@group(0) @binding(0)
var<storage, read_write> buf: array<atomic<u32>>;

@compute @workgroup_size(1)
fn main() {
    atomicCompareExchangeWeak(&buf[0], 0u, 42u);
}
";
        let result = emit_compute_ptx(wgsl, 120).expect("compile");
        let ptx = std::str::from_utf8(&result.binary).expect("PTX output is valid UTF-8");
        assert!(ptx.contains("atom.global.cas.u32"), "Expected atom.global.cas.u32 in:\n{ptx}");
    }

    #[test]
    fn ptx_memory_barrier() {
        let wgsl = r"
var<workgroup> wg_buf: array<u32, 64>;

@group(0) @binding(0)
var<storage, read_write> buf: array<u32>;

@compute @workgroup_size(64)
fn main(@builtin(local_invocation_id) lid: vec3<u32>) {
    wg_buf[lid.x] = lid.x;
    workgroupBarrier();
    buf[lid.x] = wg_buf[63u - lid.x];
}
";
        let result = emit_compute_ptx(wgsl, 120).expect("compile");
        let ptx = std::str::from_utf8(&result.binary).expect("PTX output is valid UTF-8");
        assert!(ptx.contains("bar.sync"), "Expected bar.sync in:\n{ptx}");
    }

    fn build_scan_module(
        collective_op: naga::CollectiveOperation,
    ) -> (naga::Module, usize) {
        use naga::*;

        let mut module = Module::default();
        let u32_ty = module.types.insert(
            Type { name: None, inner: TypeInner::Scalar(Scalar::U32) },
            Span::UNDEFINED,
        );
        let buf_ty = module.types.insert(
            Type {
                name: None,
                inner: TypeInner::Array {
                    base: u32_ty,
                    size: ArraySize::Dynamic,
                    stride: 4,
                },
            },
            Span::UNDEFINED,
        );
        let result_ty = module.types.insert(
            Type { name: None, inner: TypeInner::Scalar(Scalar::U32) },
            Span::UNDEFINED,
        );
        let arg_ty = module.types.insert(
            Type { name: None, inner: TypeInner::Scalar(Scalar::U32) },
            Span::UNDEFINED,
        );
        let gv = module.global_variables.append(
            GlobalVariable {
                name: Some("buf".into()),
                space: AddressSpace::Storage {
                    access: StorageAccess::LOAD | StorageAccess::STORE,
                },
                binding: Some(ResourceBinding { group: 0, binding: 0 }),
                ty: buf_ty,
                init: None,
            },
            Span::UNDEFINED,
        );

        let mut func = Function {
            name: Some("main".into()),
            ..Default::default()
        };

        let lane_expr = func.expressions.append(
            Expression::FunctionArgument(0),
            Span::UNDEFINED,
        );
        let scan_result = func.expressions.append(
            Expression::SubgroupOperationResult { ty: result_ty },
            Span::UNDEFINED,
        );
        let gv_expr = func.expressions.append(
            Expression::GlobalVariable(gv),
            Span::UNDEFINED,
        );
        let access = func.expressions.append(
            Expression::Access { base: gv_expr, index: lane_expr },
            Span::UNDEFINED,
        );

        func.body.push(
            Statement::Emit(func.expressions.range_from(func.expressions.len() - 4)),
            Span::UNDEFINED,
        );
        func.body.push(
            Statement::SubgroupCollectiveOperation {
                op: SubgroupOperation::Add,
                collective_op,
                argument: lane_expr,
                result: scan_result,
            },
            Span::UNDEFINED,
        );
        func.body.push(
            Statement::Store { pointer: access, value: scan_result },
            Span::UNDEFINED,
        );

        func.arguments.push(FunctionArgument {
            name: Some("lane".into()),
            ty: arg_ty,
            binding: Some(Binding::BuiltIn(BuiltIn::SubgroupInvocationId)),
        });

        module.entry_points.push(EntryPoint {
            name: "main".into(),
            stage: ShaderStage::Compute,
            early_depth_test: None,
            workgroup_size: [32, 1, 1],
            workgroup_size_overrides: None,
            function: func,
            mesh_info: None,
            task_payload: None,
        });
        (module, 0)
    }

    #[test]
    fn ptx_subgroup_inclusive_scan_add() {
        let (module, ep_idx) =
            build_scan_module(naga::CollectiveOperation::InclusiveScan);
        let ep_ref = &module.entry_points[ep_idx];
        let mut emitter = PtxEmitter::new(&module, ep_ref, 120);
        let ptx = emitter.emit().expect("emit scan");
        assert!(
            ptx.contains("shfl.sync.up.b32"),
            "Expected shfl.sync.up.b32 for inclusive scan in:\n{ptx}"
        );
    }

    #[test]
    fn ptx_subgroup_exclusive_scan_add() {
        let (module, ep_idx) =
            build_scan_module(naga::CollectiveOperation::ExclusiveScan);
        let ep_ref = &module.entry_points[ep_idx];
        let mut emitter = PtxEmitter::new(&module, ep_ref, 120);
        let ptx = emitter.emit().expect("emit exclusive scan");
        assert!(
            ptx.contains("shfl.sync.up.b32"),
            "Expected shfl.sync.up.b32 for exclusive scan in:\n{ptx}"
        );
        assert!(
            ptx.contains("selp."),
            "Expected selp for identity element in exclusive scan in:\n{ptx}"
        );
    }

    #[test]
    fn ptx_unhandled_statement_errors() {
        let wgsl = r"
@group(0) @binding(0)
var output_tex: texture_storage_2d<rgba8unorm, write>;

@compute @workgroup_size(1)
fn main() {
    textureStore(output_tex, vec2<u32>(0u, 0u), vec4<f32>(1.0, 0.0, 0.0, 1.0));
}
";
        let result = emit_compute_ptx(wgsl, 120);
        assert!(result.is_err(), "Expected error for unhandled image store");
    }
}
