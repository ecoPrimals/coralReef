// SPDX-License-Identifier: AGPL-3.0-or-later

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

