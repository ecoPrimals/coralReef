// SPDX-License-Identifier: AGPL-3.0-only
//! Targeted coverage for `expr.rs` and `func_ops.rs`: compose/splat/swizzle,
//! f64 vector lowering, relational builtins, casts, `arrayLength` stride path,
//! logical/bitwise ops, and matrix expressions.

use super::super::ir::{Op, ShaderModelInfo, SrcRef, TranscendentalOp};
use super::{parse_glsl, parse_wgsl, translate};

fn sm70() -> ShaderModelInfo {
    ShaderModelInfo::new(70, 64)
}

fn assert_compiles_translate(wgsl: &str, label: &str) {
    let module = parse_wgsl(wgsl).expect("WGSL should parse");
    let sm = sm70();
    let result = translate(&module, &sm, "main");
    assert!(result.is_ok(), "{label}: {:?}", result.err());
}

// ---------------------------------------------------------------------------
// `expr.rs`: Compose, Splat, Swizzle, As (casts)
// ---------------------------------------------------------------------------

#[test]
fn expr_compose_vec_from_scalars_and_swizzle() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> out: array<f32>;
        @compute @workgroup_size(64)
        fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            let a = f32(gid.x);
            let b = a + 1.0;
            let c = a + 2.0;
            let v3 = vec3<f32>(a, b, c);
            let v4 = vec4<f32>(v3, 1.0);
            let w = v4.wzyx;
            out[gid.x] = w.x + w.y + w.z + w.w + v3.x;
        }
    ";
    assert_compiles_translate(wgsl, "compose and swizzle");
    let module = parse_wgsl(wgsl).expect("valid WGSL");
    let sm = sm70();
    let shader = translate(&module, &sm, "main").expect("translate");
    let mut copies = 0u32;
    shader.for_each_instr(&mut |instr| {
        if matches!(instr.op, Op::Copy(_)) {
            copies += 1;
        }
    });
    assert!(
        copies >= 4,
        "compose/swizzle should emit OpCopy for vector wiring, got {copies}"
    );
}

#[test]
fn expr_splat_and_vec2_u32_ops() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> out: array<u32>;
        @compute @workgroup_size(64)
        fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            let s = gid.x;
            let v = vec2<u32>(s);
            let w = v + vec2<u32>(1u, 2u);
            out[gid.x] = w.x * w.y + w.y;
        }
    ";
    assert_compiles_translate(wgsl, "splat vec2<u32>");
}

#[test]
fn expr_matrix_construct_and_mul() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> out: array<f32>;
        @compute @workgroup_size(64)
        fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            let m = mat2x2<f32>(
                vec2<f32>(1.0, 2.0),
                vec2<f32>(3.0, 4.0),
            );
            let v = vec2<f32>(f32(gid.x), 1.0);
            let r = m * v;
            out[gid.x] = r.x + r.y;
        }
    ";
    assert_compiles_translate(wgsl, "mat2x2");
}

#[test]
fn expr_casts_f64_i32_u32_paths() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> out: array<f64>;
        @compute @workgroup_size(64)
        fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            let i = i32(gid.x);
            let u = u32(i);
            let f = f64(i);
            let g = f64(u);
            out[gid.x] = f + g;
        }
    ";
    assert_compiles_translate(wgsl, "f64 casts");
    let module = parse_wgsl(wgsl).expect("valid WGSL");
    let sm = sm70();
    let shader = translate(&module, &sm, "main").expect("translate");
    let mut has_f2f = false;
    shader.for_each_instr(&mut |instr| {
        if matches!(instr.op, Op::F2F(_)) {
            has_f2f = true;
        }
    });
    assert!(has_f2f, "i32/u32 to f64 should use F2F for widening");
}

// ---------------------------------------------------------------------------
// `func_ops.rs`: f64 componentwise (multi-element), f64 cmp, relational
// ---------------------------------------------------------------------------

#[test]
fn func_ops_f64_vec2_arithmetic_multi_pair() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> out: array<f64>;
        @compute @workgroup_size(64)
        fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            let a = vec2<f64>(f64(gid.x), f64(gid.x + 1u));
            let b = vec2<f64>(1.0, 2.0);
            let s = a + b - a * b / vec2<f64>(3.0, 4.0);
            out[gid.x] = s.x + s.y;
        }
    ";
    assert_compiles_translate(wgsl, "vec2 f64 ops");
    let module = parse_wgsl(wgsl).expect("valid WGSL");
    let sm = sm70();
    let shader = translate(&module, &sm, "main").expect("translate");
    let mut dadd = 0u32;
    shader.for_each_instr(&mut |instr| {
        if matches!(instr.op, Op::DAdd(_) | Op::DMul(_)) {
            dadd += 1;
        }
    });
    assert!(
        dadd >= 2,
        "vec2 f64 should emit multiple DAdd/DMul, got {dadd}"
    );
}

#[test]
fn func_ops_f64_scalar_comparisons_all_orders() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> out: array<f32>;
        @compute @workgroup_size(64)
        fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            let a = f64(gid.x);
            let b = f64(gid.x + 1u);
            let c =
                select(1.0, 0.0, a < b)
                + select(1.0, 0.0, a <= b)
                + select(1.0, 0.0, a > b)
                + select(1.0, 0.0, a >= b)
                + select(1.0, 0.0, a == b)
                + select(1.0, 0.0, a != b);
            out[gid.x] = c;
        }
    ";
    assert_compiles_translate(wgsl, "f64 compares");
    let module = parse_wgsl(wgsl).expect("valid WGSL");
    let sm = sm70();
    let shader = translate(&module, &sm, "main").expect("translate");
    let mut preds = 0u32;
    let mut sels = 0u32;
    shader.for_each_instr(&mut |instr| {
        if matches!(instr.op, Op::DSetP(_) | Op::FSetP(_) | Op::ISetP(_)) {
            preds += 1;
        }
        if matches!(instr.op, Op::Sel(_)) {
            sels += 1;
        }
    });
    assert!(
        preds >= 1 && sels >= 1,
        "f64 compare/select should emit predicate + Sel (preds={preds}, sels={sels})"
    );
}

#[test]
fn func_ops_relational_is_nan_is_inf_glsl() {
    // Naga's WGSL front (as of 28.x) does not expose isnan/isinf; GLSL does.
    let glsl = r#"#version 450
        layout(local_size_x = 64) in;
        layout(std430, binding = 0) buffer Data { float data[]; };
        void main() {
            uint gid = gl_GlobalInvocationID.x;
            float x = data[gid];
            float y = x * 0.0 / x;
            bool n = isnan(y);
            bool i = isinf(x * 1e38 * 1e38);
            data[gid] = float(n) + float(i);
        }
    "#;
    let module = parse_glsl(glsl).expect("GLSL should parse");
    let sm = sm70();
    let shader = translate(&module, &sm, "main").expect("translate");
    let mut fsetp = 0u32;
    shader.for_each_instr(&mut |instr| {
        if matches!(instr.op, Op::FSetP(_)) {
            fsetp += 1;
        }
    });
    assert!(fsetp >= 2, "isnan/isinf should emit FSetP, got {fsetp}");
}

#[test]
fn func_ops_relational_all_any_vec2_bool() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> out: array<f32>;
        @compute @workgroup_size(64)
        fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            let x = f32(gid.x);
            let v = vec2<f32>(x, x + 1.0);
            let cmp = v < vec2<f32>(100.0, 100.0);
            let a = all(cmp);
            let b = any(v > vec2<f32>(1000.0, 1000.0));
            out[gid.x] = select(1.0, 0.0, a) + select(1.0, 0.0, b);
        }
    ";
    assert_compiles_translate(wgsl, "all/any vec2");
}

#[test]
fn func_ops_logical_and_or_bool() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> out: array<f32>;
        @compute @workgroup_size(64)
        fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            let t = gid.x < 100u;
            let f = gid.x > 1000u;
            let x = t && f;
            let y = t || f;
            out[gid.x] = select(0.0, 1.0, x) + select(0.0, 1.0, y);
        }
    ";
    assert_compiles_translate(wgsl, "logical and/or");
    let module = parse_wgsl(wgsl).expect("valid WGSL");
    let sm = sm70();
    let shader = translate(&module, &sm, "main").expect("translate");
    let mut blocks = 0usize;
    for func in &shader.functions {
        blocks += func.blocks.len();
    }
    // Short-circuit `&&` / `||` may compile to branches (multiple blocks) or to PLop3.
    let mut pred_logic = false;
    shader.for_each_instr(&mut |instr| {
        if matches!(instr.op, Op::PLop3(_) | Op::PSetP(_)) {
            pred_logic = true;
        }
    });
    assert!(
        pred_logic || blocks > 1,
        "&& / || should use predicate ops or branchy CFG (pred_logic={pred_logic}, blocks={blocks})"
    );
}

#[test]
fn func_ops_integer_bitwise_not_and_negate() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> out: array<i32>;
        @compute @workgroup_size(64)
        fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            let x = i32(gid.x);
            out[gid.x] = ~x + (-x >> 1);
        }
    ";
    assert_compiles_translate(wgsl, "i32 ~ and neg");
    let module = parse_wgsl(wgsl).expect("valid WGSL");
    let sm = sm70();
    let shader = translate(&module, &sm, "main").expect("translate");
    let mut has_lop = false;
    let mut has_iadd = false;
    shader.for_each_instr(&mut |instr| {
        if matches!(instr.op, Op::Lop3(_) | Op::Lop2(_)) {
            has_lop = true;
        }
        if matches!(instr.op, Op::IAdd3(_) | Op::IAdd2(_)) {
            has_iadd = true;
        }
    });
    assert!(has_lop, "bitwise not should emit Lop");
    assert!(has_iadd, "i32 negate should emit IAdd");
}

#[test]
fn func_ops_select_per_component_vec3() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> out: array<f32>;
        @compute @workgroup_size(64)
        fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            let lo = vec3<f32>(1.0, 2.0, 3.0);
            let hi = vec3<f32>(10.0, 20.0, 30.0);
            let c = vec3<bool>(gid.x < 4u, gid.x < 8u, gid.x < 12u);
            let r = select(lo, hi, c);
            out[gid.x] = r.x + r.y + r.z;
        }
    ";
    assert_compiles_translate(wgsl, "select vec3");
}

// ---------------------------------------------------------------------------
// `translate_array_length` with non-unit element stride
// ---------------------------------------------------------------------------

#[test]
fn func_ops_array_length_vec4_stride_path() {
    let wgsl = r"
        @group(0) @binding(0) var<storage, read_write> buf: array<vec4<f32>>;
        @compute @workgroup_size(64)
        fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
            let n = arrayLength(&buf);
            let i = gid.x % n;
            let v = buf[i];
            buf[gid.x] = v * vec4<f32>(2.0, 2.0, 2.0, 2.0);
        }
    ";
    assert_compiles_translate(wgsl, "arrayLength vec4 stride");
    let module = parse_wgsl(wgsl).expect("valid WGSL");
    let sm = sm70();
    let shader = translate(&module, &sm, "main").expect("translate");
    let mut has_rcp = false;
    let mut has_cbuf = false;
    shader.for_each_instr(&mut |instr| {
        if let Op::Transcendental(t) = &instr.op {
            if t.op == TranscendentalOp::Rcp {
                has_rcp = true;
            }
        }
        if let Op::Copy(c) = &instr.op {
            if matches!(c.src.reference, SrcRef::CBuf(_)) {
                has_cbuf = true;
            }
        }
    });
    assert!(has_cbuf, "arrayLength should read buffer size from CBuf");
    assert!(
        has_rcp,
        "non-unit stride arrayLength should use float division (Rcp)"
    );
}

// ---------------------------------------------------------------------------
// Full pipeline (encoder + RA): same shaders as integration smoke
// ---------------------------------------------------------------------------

#[test]
fn expr_func_ops_full_compile_sm70() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@group(0) @binding(1) var<storage, read_write> di: array<f64>;
@compute @workgroup_size(32)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let a = vec2<f64>(f64(gid.x), 1.0);
    let b = vec2<f64>(2.0, 3.0);
    let d = a + b;
    let m = mat2x2<f32>(vec2<f32>(1.0, 0.0), vec2<f32>(0.0, 1.0));
    let v = vec2<f32>(f32(d.x), f32(d.y));
    let t = m * v;
    let x = f32(gid.x);
    let cmp = all(vec2<f32>(x, x + 1.0) < vec2<f32>(1e6, 1e6));
    out[gid.x] = t.x + t.y + select(0.0, 1.0, cmp) + f32(arrayLength(&di));
}
";
    let r = crate::compile_wgsl_raw_sm(wgsl, 70);
    assert!(
        r.is_ok(),
        "full compile expr/func_ops kitchen sink: {:?}",
        r.err()
    );
}
