// SPDX-License-Identifier: AGPL-3.0-or-later
//! Math function coverage tests: exponential, logarithmic, hyperbolic, sqrt, rounding, vector, and bitwise.

use super::super::ir::{Op, ShaderModelInfo};
use super::{parse_wgsl, translate};

fn sm70() -> ShaderModelInfo {
    ShaderModelInfo::new(70, 64)
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
