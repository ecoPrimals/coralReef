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
pub mod gemm;
mod math;
mod pointers;
mod statements;
mod types;

use std::collections::HashMap;

use crate::backend::{BinaryFormat, CompilationInfo, CompiledBinary};
use crate::error::CompileError;

use types::{BufferBinding, PtxVal, SharedVar, SurfaceBinding, TextureBinding};

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
        assert!(
            ptx.contains("atom.global.add.u32"),
            "Expected atom.global.add.u32 in:\n{ptx}"
        );
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
        assert!(
            ptx.contains("atom.shared.max.u32"),
            "Expected atom.shared.max.u32 in:\n{ptx}"
        );
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
        assert!(
            ptx.contains("atom.global.cas.u32"),
            "Expected atom.global.cas.u32 in:\n{ptx}"
        );
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

    fn build_scan_module(collective_op: naga::CollectiveOperation) -> (naga::Module, usize) {
        use naga::*;

        let mut module = Module::default();
        let u32_ty = module.types.insert(
            Type {
                name: None,
                inner: TypeInner::Scalar(Scalar::U32),
            },
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
            Type {
                name: None,
                inner: TypeInner::Scalar(Scalar::U32),
            },
            Span::UNDEFINED,
        );
        let arg_ty = module.types.insert(
            Type {
                name: None,
                inner: TypeInner::Scalar(Scalar::U32),
            },
            Span::UNDEFINED,
        );
        let gv = module.global_variables.append(
            GlobalVariable {
                name: Some("buf".into()),
                space: AddressSpace::Storage {
                    access: StorageAccess::LOAD | StorageAccess::STORE,
                },
                binding: Some(ResourceBinding {
                    group: 0,
                    binding: 0,
                }),
                ty: buf_ty,
                init: None,
            },
            Span::UNDEFINED,
        );

        let mut func = Function {
            name: Some("main".into()),
            ..Default::default()
        };

        let lane_expr = func
            .expressions
            .append(Expression::FunctionArgument(0), Span::UNDEFINED);
        let scan_result = func.expressions.append(
            Expression::SubgroupOperationResult { ty: result_ty },
            Span::UNDEFINED,
        );
        let gv_expr = func
            .expressions
            .append(Expression::GlobalVariable(gv), Span::UNDEFINED);
        let access = func.expressions.append(
            Expression::Access {
                base: gv_expr,
                index: lane_expr,
            },
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
            Statement::Store {
                pointer: access,
                value: scan_result,
            },
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
        let (module, ep_idx) = build_scan_module(naga::CollectiveOperation::InclusiveScan);
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
        let (module, ep_idx) = build_scan_module(naga::CollectiveOperation::ExclusiveScan);
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
    fn ptx_image_store_2d_rgba8() {
        let wgsl = r"
@group(0) @binding(0)
var output_tex: texture_storage_2d<rgba8unorm, write>;

@compute @workgroup_size(1)
fn main() {
    textureStore(output_tex, vec2<u32>(0u, 0u), vec4<f32>(1.0, 0.0, 0.0, 1.0));
}
";
        let result = emit_compute_ptx(wgsl, 120);
        assert!(result.is_ok(), "ImageStore should compile: {result:?}");
        let compiled = result.unwrap();
        let ptx = String::from_utf8_lossy(&compiled.binary);
        assert!(
            ptx.contains(".surfref"),
            "should declare surface: {ptx:.200}"
        );
        assert!(
            ptx.contains("sust.b.2d"),
            "should emit sust.b.2d: {ptx:.400}"
        );
    }

    #[test]
    fn ptx_image_load_2d_rgba32() {
        let wgsl = r"
@group(0) @binding(0)
var input_tex: texture_storage_2d<rgba32float, read>;
@group(0) @binding(1)
var<storage, read_write> out: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let pixel = textureLoad(input_tex, vec2<i32>(i32(gid.x), 0i));
    out[gid.x] = pixel.x;
}
";
        let result = emit_compute_ptx(wgsl, 120);
        assert!(result.is_ok(), "ImageLoad should compile: {result:?}");
        let compiled = result.unwrap();
        let ptx = String::from_utf8_lossy(&compiled.binary);
        assert!(
            ptx.contains("suld.b.2d"),
            "should emit suld.b.2d: {ptx:.400}"
        );
    }

    #[test]
    fn ptx_image_store_rg32float() {
        let wgsl = r"
@group(0) @binding(0)
var tex: texture_storage_2d<rg32float, write>;

@compute @workgroup_size(1)
fn main() {
    textureStore(tex, vec2<u32>(0u, 0u), vec4<f32>(1.0, 2.0, 0.0, 0.0));
}
";
        let result = emit_compute_ptx(wgsl, 120);
        assert!(result.is_ok(), "rg32float store: {result:?}");
        let compiled = result.unwrap();
        let ptx = String::from_utf8_lossy(&compiled.binary);
        assert!(
            ptx.contains("sust.b.2d.v2.b32"),
            "should emit v2.b32 for rg32float: {ptx:.400}"
        );
    }

    #[test]
    fn ptx_image_store_r32uint() {
        let wgsl = r"
@group(0) @binding(0)
var tex: texture_storage_2d<r32uint, write>;

@compute @workgroup_size(1)
fn main() {
    textureStore(tex, vec2<u32>(0u, 0u), vec4<u32>(42u, 0u, 0u, 0u));
}
";
        let result = emit_compute_ptx(wgsl, 120);
        assert!(result.is_ok(), "r32uint store: {result:?}");
        let compiled = result.unwrap();
        let ptx = String::from_utf8_lossy(&compiled.binary);
        assert!(
            ptx.contains("sust.b.2d.b32"),
            "should emit b32 for r32uint: {ptx:.400}"
        );
    }

    #[test]
    fn ptx_image_store_rgba16float() {
        let wgsl = r"
@group(0) @binding(0)
var tex: texture_storage_2d<rgba16float, write>;

@compute @workgroup_size(1)
fn main() {
    textureStore(tex, vec2<u32>(0u, 0u), vec4<f32>(1.0, 0.5, 0.25, 0.125));
}
";
        let result = emit_compute_ptx(wgsl, 120);
        assert!(result.is_ok(), "rgba16float store: {result:?}");
        let compiled = result.unwrap();
        let ptx = String::from_utf8_lossy(&compiled.binary);
        assert!(
            ptx.contains("sust.b.2d.v4.b16"),
            "should emit v4.b16 for rgba16float: {ptx:.400}"
        );
    }

    #[test]
    fn ptx_image_load_r32float() {
        let wgsl = r"
@group(0) @binding(0)
var tex: texture_storage_2d<r32float, read>;
@group(0) @binding(1)
var<storage, read_write> out: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let v = textureLoad(tex, vec2<i32>(i32(gid.x), 0i));
    out[gid.x] = v.x;
}
";
        let result = emit_compute_ptx(wgsl, 120);
        assert!(result.is_ok(), "r32float load: {result:?}");
        let compiled = result.unwrap();
        let ptx = String::from_utf8_lossy(&compiled.binary);
        assert!(
            ptx.contains("suld.b.2d.b32"),
            "should emit b32 for r32float: {ptx:.400}"
        );
    }

    #[test]
    fn ptx_image_store_bgra8unorm() {
        let wgsl = r"
@group(0) @binding(0)
var tex: texture_storage_2d<bgra8unorm, write>;

@compute @workgroup_size(1)
fn main() {
    textureStore(tex, vec2<u32>(0u, 0u), vec4<f32>(0.0, 1.0, 0.0, 1.0));
}
";
        let result = emit_compute_ptx(wgsl, 120);
        assert!(result.is_ok(), "bgra8unorm store: {result:?}");
        let compiled = result.unwrap();
        let ptx = String::from_utf8_lossy(&compiled.binary);
        assert!(
            ptx.contains("sust.b.2d.v4.b8"),
            "should emit v4.b8 for bgra8unorm: {ptx:.400}"
        );
    }

    #[test]
    fn ptx_image_query_size_2d() {
        let wgsl = r"
@group(0) @binding(0)
var tex: texture_storage_2d<rgba8unorm, read>;

@group(0) @binding(1)
var<storage, read_write> out: array<u32>;

@compute @workgroup_size(1)
fn main() {
    let dims = textureDimensions(tex);
    out[0] = dims.x;
    out[1] = dims.y;
}
";
        let result = emit_compute_ptx(wgsl, 120);
        assert!(result.is_ok(), "ImageQuery size 2d: {result:?}");
        let compiled = result.unwrap();
        let ptx = String::from_utf8_lossy(&compiled.binary);
        assert!(
            ptx.contains("suq.width.b32"),
            "should emit suq.width: {ptx:.400}"
        );
        assert!(
            ptx.contains("suq.height.b32"),
            "should emit suq.height for 2d: {ptx:.400}"
        );
    }

    #[test]
    fn ptx_image_query_size_1d() {
        let wgsl = r"
@group(0) @binding(0)
var tex: texture_storage_1d<r32uint, read>;

@group(0) @binding(1)
var<storage, read_write> out: array<u32>;

@compute @workgroup_size(1)
fn main() {
    let w = textureDimensions(tex);
    out[0] = w;
}
";
        let result = emit_compute_ptx(wgsl, 120);
        assert!(result.is_ok(), "ImageQuery size 1d: {result:?}");
        let compiled = result.unwrap();
        let ptx = String::from_utf8_lossy(&compiled.binary);
        assert!(
            ptx.contains("suq.width.b32"),
            "should emit suq.width for 1d: {ptx:.400}"
        );
    }

    #[test]
    fn ptx_image_sample_2d_level_zero() {
        let wgsl = r"
@group(0) @binding(0)
var my_tex: texture_2d<f32>;
@group(0) @binding(1)
var my_sampler: sampler;
@group(0) @binding(2)
var<storage, read_write> out: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let uv = vec2<f32>(f32(gid.x) / 64.0, 0.5);
    let color = textureSampleLevel(my_tex, my_sampler, uv, 0.0);
    out[gid.x] = color.r;
}
";
        let result = emit_compute_ptx(wgsl, 120);
        assert!(result.is_ok(), "ImageSample 2d level: {result:?}");
        let compiled = result.unwrap();
        let ptx = String::from_utf8_lossy(&compiled.binary);
        assert!(
            ptx.contains(".texref"),
            "should declare .texref: {ptx:.200}"
        );
        assert!(
            ptx.contains("tex.level.2d.v4.f32.f32"),
            "should emit tex.level.2d: {ptx:.600}"
        );
    }

    #[test]
    fn ptx_image_sample_1d_explicit_lod() {
        let wgsl = r"
@group(0) @binding(0)
var my_tex: texture_1d<f32>;
@group(0) @binding(1)
var my_sampler: sampler;
@group(0) @binding(2)
var<storage, read_write> out: array<f32>;

@compute @workgroup_size(32)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let u = f32(gid.x) / 32.0;
    let val = textureSampleLevel(my_tex, my_sampler, u, 2.0);
    out[gid.x] = val.r;
}
";
        let result = emit_compute_ptx(wgsl, 120);
        assert!(result.is_ok(), "ImageSample 1d lod: {result:?}");
        let compiled = result.unwrap();
        let ptx = String::from_utf8_lossy(&compiled.binary);
        assert!(
            ptx.contains("tex.level.1d.v4.f32.f32"),
            "should emit tex.level.1d: {ptx:.600}"
        );
    }

    #[test]
    fn ptx_image_sample_3d_level() {
        let wgsl = r"
@group(0) @binding(0)
var vol_tex: texture_3d<f32>;
@group(0) @binding(1)
var vol_sampler: sampler;
@group(0) @binding(2)
var<storage, read_write> out: array<f32>;

@compute @workgroup_size(8)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let uvw = vec3<f32>(f32(gid.x) / 8.0, 0.5, 0.5);
    let val = textureSampleLevel(vol_tex, vol_sampler, uvw, 0.0);
    out[gid.x] = val.r + val.g;
}
";
        let result = emit_compute_ptx(wgsl, 120);
        assert!(result.is_ok(), "ImageSample 3d: {result:?}");
        let compiled = result.unwrap();
        let ptx = String::from_utf8_lossy(&compiled.binary);
        assert!(
            ptx.contains("tex.level.3d.v4.f32.f32"),
            "should emit tex.level.3d: {ptx:.600}"
        );
    }

    #[test]
    fn ptx_image_sample_2d_gradient() {
        let wgsl = r"
@group(0) @binding(0)
var grad_tex: texture_2d<f32>;
@group(0) @binding(1)
var grad_sampler: sampler;
@group(0) @binding(2)
var<storage, read_write> out: array<f32>;

@compute @workgroup_size(16)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let uv = vec2<f32>(f32(gid.x) / 16.0, 0.5);
    let ddx = vec2<f32>(1.0 / 16.0, 0.0);
    let ddy = vec2<f32>(0.0, 1.0 / 16.0);
    let val = textureSampleGrad(grad_tex, grad_sampler, uv, ddx, ddy);
    out[gid.x] = val.r;
}
";
        let result = emit_compute_ptx(wgsl, 120);
        assert!(result.is_ok(), "ImageSample 2d gradient: {result:?}");
        let compiled = result.unwrap();
        let ptx = String::from_utf8_lossy(&compiled.binary);
        assert!(
            ptx.contains("tex.grad.2d.v4.f32.f32"),
            "should emit tex.grad.2d: {ptx:.600}"
        );
    }

    #[test]
    fn ptx_image_sample_uint_texture() {
        let wgsl = r"
@group(0) @binding(0)
var uint_tex: texture_2d<u32>;
@group(0) @binding(1)
var uint_sampler: sampler;
@group(0) @binding(2)
var<storage, read_write> out: array<u32>;

@compute @workgroup_size(32)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let uv = vec2<f32>(f32(gid.x) / 32.0, 0.5);
    let val = textureSampleLevel(uint_tex, uint_sampler, uv, 0.0);
    out[gid.x] = val.r;
}
";
        let result = emit_compute_ptx(wgsl, 120);
        assert!(result.is_ok(), "ImageSample u32: {result:?}");
        let compiled = result.unwrap();
        let ptx = String::from_utf8_lossy(&compiled.binary);
        assert!(
            ptx.contains(".texref"),
            "should declare .texref for u32 texture: {ptx:.200}"
        );
        assert!(
            ptx.contains("tex.level.2d.v4.u32.u32"),
            "should emit tex with u32 channel type: {ptx:.600}"
        );
    }

    #[test]
    fn ptx_texture_gather_2d() {
        let wgsl = r"
@group(0) @binding(0)
var gather_tex: texture_2d<f32>;
@group(0) @binding(1)
var gather_sampler: sampler;
@group(0) @binding(2)
var<storage, read_write> out: array<f32>;

@compute @workgroup_size(16)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let uv = vec2<f32>(f32(gid.x) / 16.0, 0.5);
    let gathered = textureGather(0, gather_tex, gather_sampler, uv);
    out[gid.x] = gathered.x + gathered.y + gathered.z + gathered.w;
}
";
        let result = emit_compute_ptx(wgsl, 120);
        assert!(result.is_ok(), "textureGather: {result:?}");
        let compiled = result.unwrap();
        let ptx = String::from_utf8_lossy(&compiled.binary);
        assert!(
            ptx.contains("tld4.r.2d.v4.f32.f32"),
            "should emit tld4.r.2d for component 0: {ptx:.600}"
        );
    }

    #[test]
    fn ptx_function_call_inline_simple() {
        let wgsl = r"
fn double(x: u32) -> u32 {
    return x * 2u;
}

@group(0) @binding(0)
var<storage, read_write> out: array<u32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    out[gid.x] = double(gid.x);
}
";
        let result = emit_compute_ptx(wgsl, 120);
        assert!(result.is_ok(), "Function call inlining: {result:?}");
        let compiled = result.unwrap();
        let ptx = String::from_utf8_lossy(&compiled.binary);
        assert!(
            ptx.contains("mul.lo.u32"),
            "should emit multiply from inlined double(): {ptx:.600}"
        );
    }

    #[test]
    fn ptx_function_call_inline_multi_arg() {
        let wgsl = r"
fn add_scaled(a: f32, b: f32, scale: f32) -> f32 {
    return (a + b) * scale;
}

@group(0) @binding(0)
var<storage, read_write> out: array<f32>;

@compute @workgroup_size(32)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let x = f32(gid.x);
    out[gid.x] = add_scaled(x, 1.0, 2.0);
}
";
        let result = emit_compute_ptx(wgsl, 120);
        assert!(result.is_ok(), "Multi-arg inline call: {result:?}");
        let compiled = result.unwrap();
        let ptx = String::from_utf8_lossy(&compiled.binary);
        assert!(
            ptx.contains("add.f32"),
            "should emit add from inlined function: {ptx:.600}"
        );
        assert!(
            ptx.contains("mul.f32"),
            "should emit mul from inlined function: {ptx:.600}"
        );
    }

    #[test]
    fn ptx_function_call_inline_void() {
        let wgsl = r"
@group(0) @binding(0)
var<storage, read_write> out: array<u32>;

fn write_at(idx: u32, val: u32) {
    out[idx] = val;
}

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    write_at(gid.x, 99u);
}
";
        let result = emit_compute_ptx(wgsl, 120);
        assert!(result.is_ok(), "Void function inline: {result:?}");
        let compiled = result.unwrap();
        let ptx = String::from_utf8_lossy(&compiled.binary);
        assert!(
            ptx.contains("st.global"),
            "should emit store from inlined void function: {ptx:.600}"
        );
    }

    #[test]
    fn ptx_function_call_inline_nested() {
        let wgsl = r"
fn square(x: u32) -> u32 {
    return x * x;
}

fn sum_of_squares(a: u32, b: u32) -> u32 {
    return square(a) + square(b);
}

@group(0) @binding(0)
var<storage, read_write> out: array<u32>;

@compute @workgroup_size(32)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    out[gid.x] = sum_of_squares(gid.x, gid.x + 1u);
}
";
        let result = emit_compute_ptx(wgsl, 120);
        assert!(result.is_ok(), "Nested call inlining: {result:?}");
        let compiled = result.unwrap();
        let ptx = String::from_utf8_lossy(&compiled.binary);
        assert!(
            ptx.contains("mul.lo.u32"),
            "should emit multiplies from nested inlined calls: {ptx:.600}"
        );
        assert!(
            ptx.contains("add.u32"),
            "should emit add from outer inlined function: {ptx:.600}"
        );
    }

    #[test]
    fn ptx_workgroup_uniform_load() {
        let wgsl = r"
var<workgroup> shared_val: u32;

@group(0) @binding(0)
var<storage, read_write> out: array<u32>;

@compute @workgroup_size(64)
fn main(@builtin(local_invocation_id) lid: vec3<u32>) {
    if lid.x == 0u {
        shared_val = 42u;
    }
    let uniform_val = workgroupUniformLoad(&shared_val);
    out[lid.x] = uniform_val;
}
";
        let result = emit_compute_ptx(wgsl, 120);
        assert!(
            result.is_ok(),
            "WorkGroupUniformLoad should compile: {result:?}"
        );
        let compiled = result.unwrap();
        let ptx = String::from_utf8_lossy(&compiled.binary);
        assert!(
            ptx.contains("bar.sync"),
            "should emit barrier for workgroupUniformLoad: {ptx:.600}"
        );
        assert!(
            ptx.contains("ld.shared"),
            "should emit shared memory load: {ptx:.600}"
        );
    }

    #[test]
    fn ptx_image_atomic_add_2d() {
        let wgsl = r"
@group(0) @binding(0)
var atomic_tex: texture_storage_2d<r32uint, read_write>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    textureStore(atomic_tex, vec2<u32>(gid.x, 0u), vec4<u32>(1u, 0u, 0u, 0u));
}
";
        let result = emit_compute_ptx(wgsl, 120);
        assert!(
            result.is_ok(),
            "Storage texture write should compile: {result:?}"
        );
        let compiled = result.unwrap();
        let ptx = String::from_utf8_lossy(&compiled.binary);
        assert!(
            ptx.contains("sust.b.2d"),
            "should emit surface store: {ptx:.600}"
        );
    }
}
