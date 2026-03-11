// SPDX-License-Identifier: AGPL-3.0-only
use super::super::ir::{ComputeShaderInfo, Op, ShaderModelInfo, ShaderStageInfo};
use super::{parse_glsl, parse_spirv, parse_wgsl, translate};
use crate::error::CompileError;

fn sm70() -> ShaderModelInfo {
    ShaderModelInfo::new(70, 64)
}

// ---------------------------------------------------------------------------
// Parsing tests
// ---------------------------------------------------------------------------

#[test]
fn test_parse_wgsl_valid_minimal_compute() {
    let wgsl = r"
        @compute @workgroup_size(64)
        fn main() {}
    ";
    let result = parse_wgsl(wgsl);
    assert!(
        result.is_ok(),
        "valid minimal compute shader should parse: {result:?}"
    );
    let module = result.unwrap();
    assert_eq!(module.entry_points.len(), 1);
    assert_eq!(module.entry_points[0].name, "main");
    assert_eq!(module.entry_points[0].workgroup_size, [64, 1, 1]);
}

#[test]
fn test_parse_wgsl_invalid_returns_error() {
    let invalid_wgsl = "fn main() { let x = ; }";
    let result = parse_wgsl(invalid_wgsl);
    let err = result.expect_err("invalid WGSL should return error");
    assert!(matches!(err, CompileError::InvalidInput(_)));
}

#[test]
fn test_parse_spirv_empty_returns_error() {
    let empty: &[u32] = &[];
    let result = parse_spirv(empty);
    let err = result.expect_err("empty SPIR-V should return error");
    assert!(matches!(err, CompileError::InvalidInput(_)));
}

#[test]
fn test_parse_spirv_invalid_magic_returns_error() {
    let wrong_magic = [0xDEAD_BEEFu32, 0x0001_0000, 0, 0, 0];
    let result = parse_spirv(&wrong_magic);
    let err = result.expect_err("invalid SPIR-V magic should return error");
    assert!(matches!(err, CompileError::InvalidInput(_)));
}

#[test]
fn test_parse_glsl_valid_minimal_compute() {
    let glsl = r"#version 450
        layout(local_size_x = 64) in;
        void main() {}
    ";
    let result = parse_glsl(glsl);
    assert!(
        result.is_ok(),
        "valid GLSL compute shader should parse: {result:?}"
    );
    let module = result.unwrap();
    assert_eq!(module.entry_points.len(), 1);
    assert_eq!(module.entry_points[0].name, "main");
    assert_eq!(module.entry_points[0].workgroup_size, [64, 1, 1]);
}

#[test]
fn test_parse_glsl_invalid_returns_error() {
    let invalid_glsl = "#version 450\nvoid main() { int x = ; }";
    let result = parse_glsl(invalid_glsl);
    let err = result.expect_err("invalid GLSL should return error");
    assert!(matches!(err, CompileError::InvalidInput(_)));
}

#[test]
fn test_translate_glsl_compute_with_buffer() {
    let glsl = r"#version 450
        layout(local_size_x = 64) in;
        layout(std430, binding = 0) buffer Data { float data[]; };
        void main() {
            uint gid = gl_GlobalInvocationID.x;
            data[gid] = data[gid] + 1.0;
        }
    ";
    let module = parse_glsl(glsl).unwrap();
    let sm = sm70();
    let result = translate(&module, &sm, "main");
    assert!(
        result.is_ok(),
        "GLSL compute with buffer should translate: {:?}",
        result.err()
    );
    let shader = result.unwrap();
    if let ShaderStageInfo::Compute(ComputeShaderInfo { local_size, .. }) = shader.info.stage {
        assert_eq!(local_size, [64, 1, 1]);
    } else {
        panic!("expected Compute stage info");
    }
}

// ---------------------------------------------------------------------------
// Translation tests
// ---------------------------------------------------------------------------

#[test]
fn test_translate_valid_compute_produces_shader_with_workgroup_size() {
    let wgsl = r"
        @compute @workgroup_size(8, 4, 2)
        fn main() {}
    ";
    let module = parse_wgsl(wgsl).unwrap();
    let sm = sm70();
    let result = translate(&module, &sm, "main");
    assert!(result.is_ok(), "valid compute module should translate");
    let shader = result.unwrap();
    if let ShaderStageInfo::Compute(ComputeShaderInfo { local_size, .. }) = shader.info.stage {
        assert_eq!(local_size, [8, 4, 2]);
    } else {
        panic!("expected Compute stage info");
    }
}

#[test]
fn test_translate_nonexistent_entry_point_returns_error() {
    let wgsl = r"
        @compute @workgroup_size(1)
        fn main() {}
    ";
    let module = parse_wgsl(wgsl).unwrap();
    let sm = sm70();
    let result = translate(&module, &sm, "nonexistent");
    match result {
        Ok(_) => panic!("expected error for nonexistent entry point"),
        Err(e) => assert!(matches!(e, CompileError::InvalidInput(_))),
    }
}

#[test]
fn test_translate_non_compute_entry_point_returns_error() {
    let wgsl = r"
        @vertex
        fn vs_main(@builtin(vertex_index) idx: u32) -> @builtin(position) vec4<f32> {
            return vec4<f32>(0.0, 0.0, 0.0, 1.0);
        }
    ";
    let module = parse_wgsl(wgsl).unwrap();
    let sm = sm70();
    let result = translate(&module, &sm, "vs_main");
    match result {
        Ok(_) => panic!("vertex entry point should fail"),
        Err(e) => assert!(matches!(e, CompileError::InvalidInput(_))),
    }
}

// ---------------------------------------------------------------------------
// End-to-end WGSL → IR tests
// ---------------------------------------------------------------------------

#[test]
fn test_e2e_compute_with_barrier_emits_op_bar() {
    let wgsl = r"
        @compute @workgroup_size(64)
        fn main() {
            workgroupBarrier();
        }
    ";
    let module = parse_wgsl(wgsl).unwrap();
    let sm = sm70();
    let shader = translate(&module, &sm, "main").unwrap();
    let mut has_bar = false;
    shader.for_each_instr(&mut |instr| {
        if matches!(instr.op, Op::Bar(_)) {
            has_bar = true;
        }
    });
    assert!(has_bar, "shader with workgroupBarrier should emit OpBar");
}

#[test]
fn test_e2e_compute_with_global_invocation_id_emits_s2r() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> data: array<f32>;
        @compute @workgroup_size(64)
        fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            data[gid.x] = data[gid.x] + 1.0;
        }
    ";
    let module = parse_wgsl(wgsl).unwrap();
    let sm = sm70();
    let shader = translate(&module, &sm, "main").unwrap();
    let mut has_s2r = false;
    shader.for_each_instr(&mut |instr| {
        if matches!(instr.op, Op::S2R(_)) {
            has_s2r = true;
        }
    });
    assert!(
        has_s2r,
        "shader with global_invocation_id should emit S2R (system register reads)"
    );
}

#[test]
fn test_e2e_compute_with_binary_arithmetic_emits_ops() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> data: array<f32>;
        @compute @workgroup_size(64)
        fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            data[gid.x] = data[gid.x] + 1.0;
        }
    ";
    let module = parse_wgsl(wgsl).unwrap();
    let sm = sm70();
    let shader = translate(&module, &sm, "main").unwrap();
    let mut has_fadd = false;
    shader.for_each_instr(&mut |instr| {
        if matches!(instr.op, Op::FAdd(_)) {
            has_fadd = true;
        }
    });
    assert!(
        has_fadd,
        "shader with f32 + should emit OpFAdd or equivalent arithmetic"
    );
}

#[test]
fn test_e2e_compute_with_if_else_has_multiple_blocks() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> data: array<f32>;
        @compute @workgroup_size(64)
        fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            if gid.x < 32u {
                data[gid.x] = 1.0;
            } else {
                data[gid.x] = 2.0;
            }
        }
    ";
    let module = parse_wgsl(wgsl).unwrap();
    let sm = sm70();
    let shader = translate(&module, &sm, "main").unwrap();
    let mut total_blocks = 0;
    for func in &shader.functions {
        total_blocks += func.blocks.len();
    }
    assert!(
        total_blocks > 1,
        "shader with if/else should have multiple CFG blocks, got {total_blocks}"
    );
}

#[test]
fn test_e2e_compute_with_loop_has_back_edge() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> data: array<f32>;
        @compute @workgroup_size(64)
        fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            var i: u32 = 0u;
            loop {
                if i >= 4u {
                    break;
                }
                data[gid.x] = data[gid.x] + 1.0;
                i = i + 1u;
            }
        }
    ";
    let module = parse_wgsl(wgsl).unwrap();
    let sm = sm70();
    let shader = translate(&module, &sm, "main").unwrap();
    let mut has_back_edge = false;
    for func in &shader.functions {
        for b in 0..func.blocks.len() {
            for &succ in func.blocks.successors(b) {
                if func.blocks.predecessors(succ).contains(&b) {
                    has_back_edge = true;
                    break;
                }
            }
        }
    }
    assert!(has_back_edge, "shader with loop should have CFG back edge");
}

// ---------------------------------------------------------------------------
// Expression translation coverage tests
// ---------------------------------------------------------------------------

#[test]
fn test_translate_cast_operations() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> data: array<f32>;
        @compute @workgroup_size(64)
        fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            let f = f32(gid.x);
            let i = i32(f);
            let u = u32(i);
            data[gid.x] = f32(u);
        }
    ";
    let module = parse_wgsl(wgsl).expect("valid WGSL");
    let sm = sm70();
    let result = translate(&module, &sm, "main");
    assert!(result.is_ok(), "cast operations should translate");
    let shader = result.unwrap();
    let mut has_f2i = false;
    let mut has_i2f = false;
    shader.for_each_instr(&mut |instr| {
        if matches!(instr.op, Op::F2I(_)) {
            has_f2i = true;
        }
        if matches!(instr.op, Op::I2F(_)) {
            has_i2f = true;
        }
    });
    assert!(has_f2i, "cast f32->i32 should emit F2I");
    assert!(has_i2f, "cast u32->f32 should emit I2F");
}

#[test]
fn test_translate_select_operations() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> data: array<f32>;
        @compute @workgroup_size(64)
        fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            let a = data[gid.x];
            let b = data[gid.x + 1u];
            data[gid.x] = select(a, b, a < b);
        }
    ";
    let module = parse_wgsl(wgsl).expect("valid WGSL");
    let sm = sm70();
    let result = translate(&module, &sm, "main");
    assert!(result.is_ok(), "select operations should translate");
    let shader = result.unwrap();
    let mut has_sel = false;
    let mut has_cmp = false;
    shader.for_each_instr(&mut |instr| {
        if matches!(instr.op, Op::Sel(_)) {
            has_sel = true;
        }
        if matches!(instr.op, Op::FSetP(_) | Op::ISetP(_)) {
            has_cmp = true;
        }
    });
    assert!(has_sel, "select() should emit OpSel");
    assert!(has_cmp, "a < b comparison should emit FSetP or ISetP");
}

#[test]
fn test_translate_unary_operations() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> data: array<f32>;
        @compute @workgroup_size(64)
        fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            let x = data[gid.x];
            data[gid.x] = abs(-x);
        }
    ";
    let module = parse_wgsl(wgsl).expect("valid WGSL");
    let sm = sm70();
    let result = translate(&module, &sm, "main");
    assert!(result.is_ok(), "unary ops should translate");
    let shader = result.unwrap();
    let mut has_fadd = false;
    shader.for_each_instr(&mut |instr| {
        if matches!(instr.op, Op::FAdd(_)) {
            has_fadd = true;
        }
    });
    assert!(has_fadd, "abs(-x) uses FAdd with fneg/fabs modifiers");
}

#[test]
fn test_translate_trig_sin_cos() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> data: array<f32>;
        @compute @workgroup_size(64)
        fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            let x = data[gid.x];
            data[gid.x] = sin(x) + cos(x);
        }
    ";
    let module = parse_wgsl(wgsl).expect("valid WGSL");
    let sm = sm70();
    let result = translate(&module, &sm, "main");
    assert!(result.is_ok(), "sin/cos should translate");
    let shader = result.unwrap();
    let mut has_trig = false;
    shader.for_each_instr(&mut |instr| {
        if matches!(instr.op, Op::Transcendental(_)) {
            has_trig = true;
        }
    });
    assert!(has_trig, "sin/cos should emit OpTranscendental");
}

#[test]
fn test_translate_trig_tan() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> data: array<f32>;
        @compute @workgroup_size(64)
        fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            let x = data[gid.x];
            data[gid.x] = tan(x);
        }
    ";
    let module = parse_wgsl(wgsl).expect("valid WGSL");
    let sm = sm70();
    let result = translate(&module, &sm, "main");
    assert!(result.is_ok(), "tan should translate");
}

#[test]
fn test_translate_trig_atan_atan2() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> data: array<f32>;
        @compute @workgroup_size(64)
        fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            let x = data[gid.x];
            let y = data[gid.x + 1u];
            data[gid.x] = atan(x) + atan2(y, x);
        }
    ";
    let module = parse_wgsl(wgsl).expect("valid WGSL");
    let sm = sm70();
    let result = translate(&module, &sm, "main");
    assert!(result.is_ok(), "atan/atan2 should translate");
}

#[test]
fn test_translate_trig_asin_acos() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> data: array<f32>;
        @compute @workgroup_size(64)
        fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            let x = data[gid.x];
            data[gid.x] = asin(x) + acos(x);
        }
    ";
    let module = parse_wgsl(wgsl).expect("valid WGSL");
    let sm = sm70();
    let result = translate(&module, &sm, "main");
    assert!(result.is_ok(), "asin/acos should translate");
}

#[test]
fn test_translate_vector_component_access() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> data: array<f32>;
        @compute @workgroup_size(64)
        fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            let sum = gid.x + gid.y + gid.z;
            data[sum] = 1.0;
        }
    ";
    let module = parse_wgsl(wgsl).expect("valid WGSL");
    let sm = sm70();
    let result = translate(&module, &sm, "main");
    assert!(result.is_ok(), "vector component access should translate");
    let shader = result.unwrap();
    let mut s2r_count = 0u32;
    shader.for_each_instr(&mut |instr| {
        if matches!(instr.op, Op::S2R(_)) {
            s2r_count += 1;
        }
    });
    assert!(
        s2r_count >= 3,
        "gid.x + gid.y + gid.z should emit multiple S2R, got {s2r_count}"
    );
}

#[test]
fn test_translate_multiple_bindings() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read> input: array<f32>;
        @group(0) @binding(1) var<storage, read_write> output: array<f32>;
        @compute @workgroup_size(64)
        fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            output[gid.x] = input[gid.x] * 2.0;
        }
    ";
    let module = parse_wgsl(wgsl).expect("valid WGSL");
    let sm = sm70();
    let result = translate(&module, &sm, "main");
    assert!(result.is_ok(), "multiple bindings should translate");
    let shader = result.unwrap();
    let mut has_fmul = false;
    let mut has_ld = false;
    shader.for_each_instr(&mut |instr| {
        if matches!(instr.op, Op::FMul(_)) {
            has_fmul = true;
        }
        if matches!(instr.op, Op::Ld(_)) {
            has_ld = true;
        }
    });
    assert!(has_fmul, "input * 2.0 should emit FMul");
    assert!(has_ld, "array loads should emit Ld");
}

#[test]
fn test_translate_nested_control_flow() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> data: array<u32>;
        @compute @workgroup_size(64)
        fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            var sum = 0u;
            for (var i = 0u; i < 10u; i = i + 1u) {
                if (i < 5u) {
                    sum = sum + data[i];
                }
            }
            data[gid.x] = sum;
        }
    ";
    let module = parse_wgsl(wgsl).expect("valid WGSL");
    let sm = sm70();
    let result = translate(&module, &sm, "main");
    assert!(result.is_ok(), "nested control flow should translate");
    let shader = result.unwrap();
    let mut total_blocks = 0;
    for func in &shader.functions {
        total_blocks += func.blocks.len();
    }
    assert!(
        total_blocks > 1,
        "nested if+loop should have multiple CFG blocks, got {total_blocks}"
    );
}

#[test]
fn test_translate_math_function_variety() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> data: array<f32>;
        @compute @workgroup_size(64)
        fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            let x = data[gid.x];
            data[gid.x] = clamp(floor(x), 0.0, 1.0);
        }
    ";
    let module = parse_wgsl(wgsl).expect("valid WGSL");
    let sm = sm70();
    let result = translate(&module, &sm, "main");
    assert!(result.is_ok(), "math functions should translate");
    let shader = result.unwrap();
    let mut has_fmnmx = false;
    let mut has_frnd = false;
    shader.for_each_instr(&mut |instr| {
        if matches!(instr.op, Op::FMnMx(_)) {
            has_fmnmx = true;
        }
        if matches!(instr.op, Op::FRnd(_)) {
            has_frnd = true;
        }
    });
    assert!(has_fmnmx, "clamp should emit FMnMx");
    assert!(has_frnd, "floor should emit FRnd");
}

// ---------------------------------------------------------------------------
// Exponential & logarithmic coverage
// ---------------------------------------------------------------------------

#[test]
fn test_translate_exp2() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> data: array<f32>;
        @compute @workgroup_size(64)
        fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            data[gid.x] = exp2(data[gid.x]);
        }
    ";
    let module = parse_wgsl(wgsl).expect("valid WGSL");
    let sm = sm70();
    let shader = translate(&module, &sm, "main").expect("exp2 should translate");
    let mut has_transcendental = false;
    shader.for_each_instr(&mut |instr| {
        if matches!(instr.op, Op::Transcendental(_)) {
            has_transcendental = true;
        }
    });
    assert!(
        has_transcendental,
        "exp2 should emit OpTranscendental(Exp2)"
    );
}

#[test]
fn test_translate_log2() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> data: array<f32>;
        @compute @workgroup_size(64)
        fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            data[gid.x] = log2(data[gid.x]);
        }
    ";
    let module = parse_wgsl(wgsl).expect("valid WGSL");
    let sm = sm70();
    let shader = translate(&module, &sm, "main").expect("log2 should translate");
    let mut has_transcendental = false;
    shader.for_each_instr(&mut |instr| {
        if matches!(instr.op, Op::Transcendental(_)) {
            has_transcendental = true;
        }
    });
    assert!(
        has_transcendental,
        "log2 should emit OpTranscendental(Log2)"
    );
}

#[test]
fn test_translate_exp() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> data: array<f32>;
        @compute @workgroup_size(64)
        fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            data[gid.x] = exp(data[gid.x]);
        }
    ";
    let module = parse_wgsl(wgsl).expect("valid WGSL");
    let sm = sm70();
    let shader = translate(&module, &sm, "main").expect("exp should translate");
    let mut has_fmul = false;
    let mut has_transcendental = false;
    shader.for_each_instr(&mut |instr| {
        if matches!(instr.op, Op::FMul(_)) {
            has_fmul = true;
        }
        if matches!(instr.op, Op::Transcendental(_)) {
            has_transcendental = true;
        }
    });
    assert!(has_fmul, "exp(x) = exp2(x*log2(e)) needs FMul for scaling");
    assert!(has_transcendental, "exp needs Transcendental(Exp2)");
}

#[test]
fn test_translate_log() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> data: array<f32>;
        @compute @workgroup_size(64)
        fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            data[gid.x] = log(data[gid.x]);
        }
    ";
    let module = parse_wgsl(wgsl).expect("valid WGSL");
    let sm = sm70();
    let shader = translate(&module, &sm, "main").expect("log should translate");
    let mut has_fmul = false;
    let mut has_transcendental = false;
    shader.for_each_instr(&mut |instr| {
        if matches!(instr.op, Op::FMul(_)) {
            has_fmul = true;
        }
        if matches!(instr.op, Op::Transcendental(_)) {
            has_transcendental = true;
        }
    });
    assert!(has_fmul, "log(x) = log2(x)*ln(2) needs FMul for scaling");
    assert!(has_transcendental, "log needs Transcendental(Log2)");
}

#[test]
fn test_translate_pow() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> data: array<f32>;
        @compute @workgroup_size(64)
        fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            let base = data[gid.x];
            let exponent = data[gid.x + 1u];
            data[gid.x] = pow(base, exponent);
        }
    ";
    let module = parse_wgsl(wgsl).expect("valid WGSL");
    let sm = sm70();
    let shader = translate(&module, &sm, "main").expect("pow should translate");
    let mut transcendental_count = 0u32;
    shader.for_each_instr(&mut |instr| {
        if matches!(instr.op, Op::Transcendental(_)) {
            transcendental_count += 1;
        }
    });
    assert!(
        transcendental_count >= 2,
        "pow(a,b) = exp2(b*log2(a)) needs Log2 + Exp2, got {transcendental_count}"
    );
}

// ---------------------------------------------------------------------------
// Hyperbolic trig coverage
// ---------------------------------------------------------------------------

#[test]
fn test_translate_sinh_cosh() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> data: array<f32>;
        @compute @workgroup_size(64)
        fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            let x = data[gid.x];
            data[gid.x] = sinh(x) + cosh(x);
        }
    ";
    let module = parse_wgsl(wgsl).expect("valid WGSL");
    let sm = sm70();
    let shader = translate(&module, &sm, "main").expect("sinh/cosh should translate");
    let mut transcendental_count = 0u32;
    shader.for_each_instr(&mut |instr| {
        if matches!(instr.op, Op::Transcendental(_)) {
            transcendental_count += 1;
        }
    });
    assert!(
        transcendental_count >= 4,
        "sinh + cosh each need 2x Exp2, got {transcendental_count}"
    );
}

#[test]
fn test_translate_tanh() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> data: array<f32>;
        @compute @workgroup_size(64)
        fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            data[gid.x] = tanh(data[gid.x]);
        }
    ";
    let module = parse_wgsl(wgsl).expect("valid WGSL");
    let sm = sm70();
    let shader = translate(&module, &sm, "main").expect("tanh should translate");
    let mut has_transcendental = false;
    shader.for_each_instr(&mut |instr| {
        if matches!(instr.op, Op::Transcendental(_)) {
            has_transcendental = true;
        }
    });
    assert!(
        has_transcendental,
        "tanh needs Exp2 and Rcp transcendentals"
    );
}

#[test]
fn test_translate_asinh_acosh_atanh() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> data: array<f32>;
        @compute @workgroup_size(64)
        fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            let x = data[gid.x];
            data[gid.x] = asinh(x);
        }
    ";
    let module = parse_wgsl(wgsl).expect("valid WGSL");
    let sm = sm70();
    let shader = translate(&module, &sm, "main").expect("asinh should translate");
    let mut transcendental_count = 0u32;
    shader.for_each_instr(&mut |instr| {
        if matches!(instr.op, Op::Transcendental(_)) {
            transcendental_count += 1;
        }
    });
    assert!(
        transcendental_count >= 3,
        "asinh uses Rsq + Rcp + Log2, got {transcendental_count}"
    );

    let wgsl2 = r"
        @group(0) @binding(0) var<storage, read_write> data: array<f32>;
        @compute @workgroup_size(64)
        fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            data[gid.x] = acosh(data[gid.x]);
        }
    ";
    let module2 = parse_wgsl(wgsl2).expect("valid WGSL");
    let sm2 = sm70();
    assert!(
        translate(&module2, &sm2, "main").is_ok(),
        "acosh should translate"
    );

    let wgsl3 = r"
        @group(0) @binding(0) var<storage, read_write> data: array<f32>;
        @compute @workgroup_size(64)
        fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            data[gid.x] = atanh(data[gid.x]);
        }
    ";
    let module3 = parse_wgsl(wgsl3).expect("valid WGSL");
    let sm3 = sm70();
    assert!(
        translate(&module3, &sm3, "main").is_ok(),
        "atanh should translate"
    );
}

// ---------------------------------------------------------------------------
// Square root coverage
// ---------------------------------------------------------------------------

#[test]
fn test_translate_sqrt() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> data: array<f32>;
        @compute @workgroup_size(64)
        fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            data[gid.x] = sqrt(data[gid.x]);
        }
    ";
    let module = parse_wgsl(wgsl).expect("valid WGSL");
    let sm = sm70();
    let shader = translate(&module, &sm, "main").expect("sqrt should translate");
    let mut transcendental_count = 0u32;
    shader.for_each_instr(&mut |instr| {
        if matches!(instr.op, Op::Transcendental(_)) {
            transcendental_count += 1;
        }
    });
    assert!(
        transcendental_count >= 2,
        "sqrt = rcp(rsq(x)) needs Rsq + Rcp, got {transcendental_count}"
    );
}

#[test]
fn test_translate_inverse_sqrt() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> data: array<f32>;
        @compute @workgroup_size(64)
        fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            data[gid.x] = inverseSqrt(data[gid.x]);
        }
    ";
    let module = parse_wgsl(wgsl).expect("valid WGSL");
    let sm = sm70();
    let shader = translate(&module, &sm, "main").expect("inverseSqrt should translate");
    let mut has_transcendental = false;
    shader.for_each_instr(&mut |instr| {
        if matches!(instr.op, Op::Transcendental(_)) {
            has_transcendental = true;
        }
    });
    assert!(
        has_transcendental,
        "inverseSqrt should emit Rsq transcendental"
    );
}

// ---------------------------------------------------------------------------
// Rounding coverage (ceil, round, trunc, fract)
// ---------------------------------------------------------------------------

#[test]
fn test_translate_ceil() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> data: array<f32>;
        @compute @workgroup_size(64)
        fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            data[gid.x] = ceil(data[gid.x]);
        }
    ";
    let module = parse_wgsl(wgsl).expect("valid WGSL");
    let sm = sm70();
    let shader = translate(&module, &sm, "main").expect("ceil should translate");
    let mut has_frnd = false;
    shader.for_each_instr(&mut |instr| {
        if matches!(instr.op, Op::FRnd(_)) {
            has_frnd = true;
        }
    });
    assert!(has_frnd, "ceil should emit FRnd with PosInf mode");
}

#[test]
fn test_translate_round() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> data: array<f32>;
        @compute @workgroup_size(64)
        fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            data[gid.x] = round(data[gid.x]);
        }
    ";
    let module = parse_wgsl(wgsl).expect("valid WGSL");
    let sm = sm70();
    let shader = translate(&module, &sm, "main").expect("round should translate");
    let mut has_frnd = false;
    shader.for_each_instr(&mut |instr| {
        if matches!(instr.op, Op::FRnd(_)) {
            has_frnd = true;
        }
    });
    assert!(has_frnd, "round should emit FRnd with NearestEven mode");
}

#[test]
fn test_translate_trunc() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> data: array<f32>;
        @compute @workgroup_size(64)
        fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            data[gid.x] = trunc(data[gid.x]);
        }
    ";
    let module = parse_wgsl(wgsl).expect("valid WGSL");
    let sm = sm70();
    let shader = translate(&module, &sm, "main").expect("trunc should translate");
    let mut has_frnd = false;
    shader.for_each_instr(&mut |instr| {
        if matches!(instr.op, Op::FRnd(_)) {
            has_frnd = true;
        }
    });
    assert!(has_frnd, "trunc should emit FRnd with Zero mode");
}

#[test]
fn test_translate_fract() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> data: array<f32>;
        @compute @workgroup_size(64)
        fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            data[gid.x] = fract(data[gid.x]);
        }
    ";
    let module = parse_wgsl(wgsl).expect("valid WGSL");
    let sm = sm70();
    let shader = translate(&module, &sm, "main").expect("fract should translate");
    let mut has_frnd = false;
    let mut has_fadd = false;
    shader.for_each_instr(&mut |instr| {
        if matches!(instr.op, Op::FRnd(_)) {
            has_frnd = true;
        }
        if matches!(instr.op, Op::FAdd(_)) {
            has_fadd = true;
        }
    });
    assert!(has_frnd, "fract needs floor via FRnd");
    assert!(has_fadd, "fract = x - floor(x) needs FAdd with fneg");
}

// ---------------------------------------------------------------------------
// Vector math coverage
// ---------------------------------------------------------------------------

#[test]
fn test_translate_dot_product() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> data: array<f32>;
        @compute @workgroup_size(64)
        fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            let a = vec3<f32>(data[0u], data[1u], data[2u]);
            let b = vec3<f32>(data[3u], data[4u], data[5u]);
            data[gid.x] = dot(a, b);
        }
    ";
    let module = parse_wgsl(wgsl).expect("valid WGSL");
    let sm = sm70();
    let shader = translate(&module, &sm, "main").expect("dot should translate");
    let mut has_ffma = false;
    let mut has_fmul = false;
    shader.for_each_instr(&mut |instr| {
        if matches!(instr.op, Op::FFma(_)) {
            has_ffma = true;
        }
        if matches!(instr.op, Op::FMul(_)) {
            has_fmul = true;
        }
    });
    assert!(has_fmul, "dot product starts with FMul for first component");
    assert!(has_ffma, "dot product uses FFma for accumulation");
}

#[test]
fn test_translate_cross_product() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> data: array<f32>;
        @compute @workgroup_size(64)
        fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            let a = vec3<f32>(data[0u], data[1u], data[2u]);
            let b = vec3<f32>(data[3u], data[4u], data[5u]);
            let c = cross(a, b);
            data[gid.x] = c.x + c.y + c.z;
        }
    ";
    let module = parse_wgsl(wgsl).expect("valid WGSL");
    let sm = sm70();
    let shader = translate(&module, &sm, "main").expect("cross should translate");
    let mut ffma_count = 0u32;
    shader.for_each_instr(&mut |instr| {
        if matches!(instr.op, Op::FFma(_)) {
            ffma_count += 1;
        }
    });
    assert!(
        ffma_count >= 3,
        "cross product needs 3 FFma (one per component), got {ffma_count}"
    );
}

#[test]
fn test_translate_length() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> data: array<f32>;
        @compute @workgroup_size(64)
        fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            let v = vec3<f32>(data[0u], data[1u], data[2u]);
            data[gid.x] = length(v);
        }
    ";
    let module = parse_wgsl(wgsl).expect("valid WGSL");
    let sm = sm70();
    let shader = translate(&module, &sm, "main").expect("length should translate");
    let mut transcendental_count = 0u32;
    shader.for_each_instr(&mut |instr| {
        if matches!(instr.op, Op::Transcendental(_)) {
            transcendental_count += 1;
        }
    });
    assert!(
        transcendental_count >= 2,
        "length = rcp(rsq(dot(v,v))) needs Rsq + Rcp, got {transcendental_count}"
    );
}

#[test]
fn test_translate_normalize() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> data: array<f32>;
        @compute @workgroup_size(64)
        fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            let v = vec3<f32>(data[0u], data[1u], data[2u]);
            let n = normalize(v);
            data[gid.x] = n.x + n.y + n.z;
        }
    ";
    let module = parse_wgsl(wgsl).expect("valid WGSL");
    let sm = sm70();
    let shader = translate(&module, &sm, "main").expect("normalize should translate");
    let mut has_transcendental = false;
    shader.for_each_instr(&mut |instr| {
        if matches!(instr.op, Op::Transcendental(_)) {
            has_transcendental = true;
        }
    });
    assert!(has_transcendental, "normalize uses Rsq for inverse length");
}

#[test]
fn test_translate_distance() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> data: array<f32>;
        @compute @workgroup_size(64)
        fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            let a = vec3<f32>(data[0u], data[1u], data[2u]);
            let b = vec3<f32>(data[3u], data[4u], data[5u]);
            data[gid.x] = distance(a, b);
        }
    ";
    let module = parse_wgsl(wgsl).expect("valid WGSL");
    let sm = sm70();
    let shader = translate(&module, &sm, "main").expect("distance should translate");
    let mut has_fadd = false;
    let mut has_transcendental = false;
    shader.for_each_instr(&mut |instr| {
        if matches!(instr.op, Op::FAdd(_)) {
            has_fadd = true;
        }
        if matches!(instr.op, Op::Transcendental(_)) {
            has_transcendental = true;
        }
    });
    assert!(
        has_fadd,
        "distance = length(a-b) needs FAdd for subtraction"
    );
    assert!(has_transcendental, "distance uses Rsq + Rcp for length");
}

// ---------------------------------------------------------------------------
// Bitwise operations coverage
// ---------------------------------------------------------------------------

#[test]
fn test_translate_count_one_bits() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> data: array<u32>;
        @compute @workgroup_size(64)
        fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            data[gid.x] = countOneBits(data[gid.x]);
        }
    ";
    let module = parse_wgsl(wgsl).expect("valid WGSL");
    let sm = sm70();
    let shader = translate(&module, &sm, "main").expect("countOneBits should translate");
    let mut has_popc = false;
    shader.for_each_instr(&mut |instr| {
        if matches!(instr.op, Op::PopC(_)) {
            has_popc = true;
        }
    });
    assert!(has_popc, "countOneBits should emit OpPopC");
}

#[test]
fn test_translate_reverse_bits() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> data: array<u32>;
        @compute @workgroup_size(64)
        fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            data[gid.x] = reverseBits(data[gid.x]);
        }
    ";
    let module = parse_wgsl(wgsl).expect("valid WGSL");
    let sm = sm70();
    let shader = translate(&module, &sm, "main").expect("reverseBits should translate");
    let mut has_brev = false;
    shader.for_each_instr(&mut |instr| {
        if matches!(instr.op, Op::BRev(_)) {
            has_brev = true;
        }
    });
    assert!(has_brev, "reverseBits should emit OpBRev");
}

#[test]
fn test_translate_first_leading_bit() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> data: array<u32>;
        @compute @workgroup_size(64)
        fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            data[gid.x] = firstLeadingBit(data[gid.x]);
        }
    ";
    let module = parse_wgsl(wgsl).expect("valid WGSL");
    let sm = sm70();
    let shader = translate(&module, &sm, "main").expect("firstLeadingBit should translate");
    let mut has_flo = false;
    shader.for_each_instr(&mut |instr| {
        if matches!(instr.op, Op::Flo(_)) {
            has_flo = true;
        }
    });
    assert!(has_flo, "firstLeadingBit should emit OpFlo");
}

#[test]
fn test_translate_count_leading_zeros() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> data: array<u32>;
        @compute @workgroup_size(64)
        fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            data[gid.x] = countLeadingZeros(data[gid.x]);
        }
    ";
    let module = parse_wgsl(wgsl).expect("valid WGSL");
    let sm = sm70();
    let shader = translate(&module, &sm, "main").expect("countLeadingZeros should translate");
    let mut has_flo = false;
    shader.for_each_instr(&mut |instr| {
        if matches!(instr.op, Op::Flo(_)) {
            has_flo = true;
        }
    });
    assert!(
        has_flo,
        "countLeadingZeros should emit OpFlo with return_shift_amount"
    );
}

#[test]
fn test_translate_first_trailing_bit() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> data: array<u32>;
        @compute @workgroup_size(64)
        fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            data[gid.x] = firstTrailingBit(data[gid.x]);
        }
    ";
    let module = parse_wgsl(wgsl).expect("valid WGSL");
    let sm = sm70();
    let shader = translate(&module, &sm, "main").expect("firstTrailingBit should translate");
    let mut has_brev = false;
    let mut has_flo = false;
    shader.for_each_instr(&mut |instr| {
        if matches!(instr.op, Op::BRev(_)) {
            has_brev = true;
        }
        if matches!(instr.op, Op::Flo(_)) {
            has_flo = true;
        }
    });
    assert!(
        has_brev,
        "firstTrailingBit lowered as clz(reverseBits(x)) needs BRev"
    );
    assert!(
        has_flo,
        "firstTrailingBit lowered as clz(reverseBits(x)) needs Flo"
    );
}

// ---------------------------------------------------------------------------
// Interpolation & misc math coverage
// ---------------------------------------------------------------------------

#[test]
fn test_translate_fma() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> data: array<f32>;
        @compute @workgroup_size(64)
        fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            let a = data[gid.x];
            let b = data[gid.x + 1u];
            let c = data[gid.x + 2u];
            data[gid.x] = fma(a, b, c);
        }
    ";
    let module = parse_wgsl(wgsl).expect("valid WGSL");
    let sm = sm70();
    let shader = translate(&module, &sm, "main").expect("fma should translate");
    let mut has_ffma = false;
    shader.for_each_instr(&mut |instr| {
        if matches!(instr.op, Op::FFma(_)) {
            has_ffma = true;
        }
    });
    assert!(has_ffma, "fma should emit OpFFma");
}

#[test]
fn test_translate_sign() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> data: array<f32>;
        @compute @workgroup_size(64)
        fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            data[gid.x] = sign(data[gid.x]);
        }
    ";
    let module = parse_wgsl(wgsl).expect("valid WGSL");
    let sm = sm70();
    let shader = translate(&module, &sm, "main").expect("sign should translate");
    let mut has_fsetp = false;
    let mut has_sel = false;
    shader.for_each_instr(&mut |instr| {
        if matches!(instr.op, Op::FSetP(_)) {
            has_fsetp = true;
        }
        if matches!(instr.op, Op::Sel(_)) {
            has_sel = true;
        }
    });
    assert!(has_fsetp, "sign needs FSetP for >0 and <0 comparisons");
    assert!(has_sel, "sign needs Sel to choose -1/0/+1");
}

#[test]
fn test_translate_mix() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> data: array<f32>;
        @compute @workgroup_size(64)
        fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            let a = data[0u];
            let b = data[1u];
            let t = data[2u];
            data[gid.x] = mix(a, b, t);
        }
    ";
    let module = parse_wgsl(wgsl).expect("valid WGSL");
    let sm = sm70();
    let shader = translate(&module, &sm, "main").expect("mix should translate");
    let mut has_ffma = false;
    let mut has_fadd = false;
    shader.for_each_instr(&mut |instr| {
        if matches!(instr.op, Op::FFma(_)) {
            has_ffma = true;
        }
        if matches!(instr.op, Op::FAdd(_)) {
            has_fadd = true;
        }
    });
    assert!(has_fadd, "mix = a + t*(b-a) needs FAdd for (b-a)");
    assert!(has_ffma, "mix = (b-a)*t + a needs FFma");
}

#[test]
fn test_translate_step() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> data: array<f32>;
        @compute @workgroup_size(64)
        fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            let edge = data[0u];
            let x = data[gid.x];
            data[gid.x] = step(edge, x);
        }
    ";
    let module = parse_wgsl(wgsl).expect("valid WGSL");
    let sm = sm70();
    let shader = translate(&module, &sm, "main").expect("step should translate");
    let mut has_fsetp = false;
    let mut has_sel = false;
    shader.for_each_instr(&mut |instr| {
        if matches!(instr.op, Op::FSetP(_)) {
            has_fsetp = true;
        }
        if matches!(instr.op, Op::Sel(_)) {
            has_sel = true;
        }
    });
    assert!(has_fsetp, "step needs FSetP for x >= edge");
    assert!(has_sel, "step needs Sel to choose 0.0 or 1.0");
}

#[test]
fn test_translate_smoothstep() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> data: array<f32>;
        @compute @workgroup_size(64)
        fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            let lo = data[0u];
            let hi = data[1u];
            let x = data[gid.x];
            data[gid.x] = smoothstep(lo, hi, x);
        }
    ";
    let module = parse_wgsl(wgsl).expect("valid WGSL");
    let sm = sm70();
    let shader = translate(&module, &sm, "main").expect("smoothstep should translate");
    let mut has_transcendental = false;
    let mut fmul_count = 0u32;
    shader.for_each_instr(&mut |instr| {
        if matches!(instr.op, Op::Transcendental(_)) {
            has_transcendental = true;
        }
        if matches!(instr.op, Op::FMul(_)) {
            fmul_count += 1;
        }
    });
    assert!(has_transcendental, "smoothstep uses Rcp for 1/(hi-lo)");
    assert!(
        fmul_count >= 3,
        "smoothstep needs multiple FMul for t*t*(3-2*t), got {fmul_count}"
    );
}

// ---------------------------------------------------------------------------
// Min / Max coverage
// ---------------------------------------------------------------------------

#[test]
fn test_translate_min_max() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> data: array<f32>;
        @compute @workgroup_size(64)
        fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            let a = data[gid.x];
            let b = data[gid.x + 1u];
            data[gid.x] = min(a, b) + max(a, b);
        }
    ";
    let module = parse_wgsl(wgsl).expect("valid WGSL");
    let sm = sm70();
    let shader = translate(&module, &sm, "main").expect("min/max should translate");
    let mut fmnmx_count = 0u32;
    shader.for_each_instr(&mut |instr| {
        if matches!(instr.op, Op::FMnMx(_)) {
            fmnmx_count += 1;
        }
    });
    assert!(
        fmnmx_count >= 2,
        "min + max should emit at least 2 FMnMx, got {fmnmx_count}"
    );
}

// ---------------------------------------------------------------------------
// Builtin resolution coverage
// ---------------------------------------------------------------------------

#[test]
fn test_translate_local_invocation_id() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> data: array<u32>;
        @compute @workgroup_size(64)
        fn main(@builtin(local_invocation_id) lid: vec3<u32>) {
            data[lid.x] = lid.y + lid.z;
        }
    ";
    let module = parse_wgsl(wgsl).expect("valid WGSL");
    let sm = sm70();
    let shader = translate(&module, &sm, "main").expect("local_invocation_id should translate");
    let mut s2r_count = 0u32;
    shader.for_each_instr(&mut |instr| {
        if matches!(instr.op, Op::S2R(_)) {
            s2r_count += 1;
        }
    });
    assert!(
        s2r_count >= 3,
        "local_invocation_id needs 3 S2R (TID_X/Y/Z), got {s2r_count}"
    );
}

#[test]
fn test_translate_workgroup_id() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> data: array<u32>;
        @compute @workgroup_size(64)
        fn main(@builtin(workgroup_id) wg: vec3<u32>) {
            data[wg.x] = wg.y + wg.z;
        }
    ";
    let module = parse_wgsl(wgsl).expect("valid WGSL");
    let sm = sm70();
    let shader = translate(&module, &sm, "main").expect("workgroup_id should translate");
    let mut s2r_count = 0u32;
    shader.for_each_instr(&mut |instr| {
        if matches!(instr.op, Op::S2R(_)) {
            s2r_count += 1;
        }
    });
    assert!(
        s2r_count >= 3,
        "workgroup_id needs 3 S2R (CTAID_X/Y/Z), got {s2r_count}"
    );
}

#[test]
fn test_translate_num_workgroups() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> data: array<u32>;
        @compute @workgroup_size(64)
        fn main(@builtin(num_workgroups) nwg: vec3<u32>) {
            data[0u] = nwg.x * nwg.y * nwg.z;
        }
    ";
    let module = parse_wgsl(wgsl).expect("valid WGSL");
    let sm = sm70();
    let shader = translate(&module, &sm, "main").expect("num_workgroups should translate");
    let mut s2r_count = 0u32;
    shader.for_each_instr(&mut |instr| {
        if matches!(instr.op, Op::S2R(_)) {
            s2r_count += 1;
        }
    });
    assert!(
        s2r_count >= 3,
        "num_workgroups needs 3 S2R (NCTAID_X/Y/Z), got {s2r_count}"
    );
}

#[test]
fn test_translate_local_invocation_index() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> data: array<u32>;
        @compute @workgroup_size(8, 4, 2)
        fn main(@builtin(local_invocation_index) idx: u32) {
            data[idx] = idx;
        }
    ";
    let module = parse_wgsl(wgsl).expect("valid WGSL");
    let sm = sm70();
    let shader = translate(&module, &sm, "main").expect("local_invocation_index should translate");
    let mut s2r_count = 0u32;
    let mut has_imad = false;
    shader.for_each_instr(&mut |instr| {
        if matches!(instr.op, Op::S2R(_)) {
            s2r_count += 1;
        }
        if matches!(instr.op, Op::IMad(_)) {
            has_imad = true;
        }
    });
    assert!(
        s2r_count >= 3,
        "local_invocation_index needs TID_X/Y/Z reads, got {s2r_count}"
    );
    assert!(has_imad, "local_invocation_index linearization uses IMad");
}
