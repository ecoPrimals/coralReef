// SPDX-License-Identifier: AGPL-3.0-or-later
//! Cross-architecture coverage tests — vectors, bit-ops, math functions, atomics, RDNA2 variants.

use coral_reef::{AmdArch, CompileOptions, GpuArch, GpuTarget};

fn opts() -> CompileOptions {
    CompileOptions {
        target: GpuArch::Sm70.into(),
        opt_level: 2,
        debug_info: false,
        fp64_software: true,
        ..CompileOptions::default()
    }
}

fn compile_fixture_sm70(wgsl: &str) {
    let r = coral_reef::compile_wgsl(wgsl, &opts());
    assert!(r.is_ok(), "SM70: {}", r.unwrap_err());
}

fn compile_raw_sm(wgsl: &str, sm: u8) {
    let r = coral_reef::compile_wgsl_raw_sm(wgsl, sm);
    assert!(r.is_ok(), "SM{sm}: {}", r.unwrap_err());
}

fn compile_fixture_rdna2(wgsl: &str) {
    let opts = CompileOptions {
        target: GpuTarget::Amd(AmdArch::Rdna2),
        opt_level: 2,
        debug_info: false,
        fp64_software: true,
        ..CompileOptions::default()
    };
    let r = coral_reef::compile_wgsl(wgsl, &opts);
    assert!(r.is_ok(), "RDNA2: {}", r.unwrap_err());
}

// ---------------------------------------------------------------------------
// Cross-arch and vector coverage
// ---------------------------------------------------------------------------

#[test]
fn coverage_multi_arch_compile_all_supported() {
    let wgsl = "@compute @workgroup_size(1) fn main() {}";
    for sm in [70, 75, 80, 86, 89] {
        compile_raw_sm(wgsl, sm);
    }
    compile_fixture_rdna2(wgsl);
}

#[test]
fn coverage_vec_types_all() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(1)
fn main() {
    let v2 = vec2f(1.0, 2.0);
    let v3 = vec3f(1.0, 2.0, 3.0);
    let v4 = vec4f(1.0, 2.0, 3.0, 4.0);
    out[0] = dot(v2, v2);
    out[1] = dot(v3, v3);
    out[2] = dot(v4, v4);
    out[3] = length(v3);
    let n = normalize(v3);
    out[4] = n.x;
    let c = cross(v3, vec3f(0.0, 0.0, 1.0));
    out[5] = c.x;
    out[6] = distance(v2, vec2f(0.0, 0.0));
}
";
    compile_fixture_sm70(wgsl);
}

// ---------------------------------------------------------------------------
// Newly implemented: FirstTrailingBit, reverseBits, Distance
// ---------------------------------------------------------------------------

#[test]
fn coverage_sm70_first_trailing_bit() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<u32>;
@compute @workgroup_size(1)
fn main() {
    out[0] = firstTrailingBit(0x80u);
    out[1] = firstTrailingBit(0x10u);
    out[2] = firstTrailingBit(1u);
    out[3] = firstTrailingBit(0u);
}
";
    compile_fixture_sm70(wgsl);
}

#[test]
fn coverage_sm70_reverse_bits() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<u32>;
@compute @workgroup_size(1)
fn main() {
    out[0] = reverseBits(0x80000000u);
    out[1] = reverseBits(0x00000001u);
    out[2] = reverseBits(0xF0F0F0F0u);
}
";
    compile_fixture_sm70(wgsl);
}

#[test]
fn coverage_sm70_distance() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(1)
fn main() {
    let a = vec3f(1.0, 2.0, 3.0);
    let b = vec3f(4.0, 6.0, 3.0);
    out[0] = distance(a, b);
    let c = vec2f(0.0, 0.0);
    let d = vec2f(3.0, 4.0);
    out[1] = distance(c, d);
}
";
    compile_fixture_sm70(wgsl);
}

#[test]
fn coverage_sm70_first_trailing_bit_signed() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<i32>;
@compute @workgroup_size(1)
fn main() {
    out[0] = firstTrailingBit(-128i);
    out[1] = firstTrailingBit(16i);
}
";
    compile_fixture_sm70(wgsl);
}

#[test]
fn coverage_rdna2_first_trailing_bit() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<u32>;
@compute @workgroup_size(1)
fn main() {
    out[0] = firstTrailingBit(0x80u);
    out[1] = firstTrailingBit(0u);
}
";
    compile_fixture_rdna2(wgsl);
}

#[test]
fn coverage_rdna2_distance() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(1)
fn main() {
    let a = vec3f(1.0, 0.0, 0.0);
    let b = vec3f(0.0, 1.0, 0.0);
    out[0] = distance(a, b);
}
";
    compile_fixture_rdna2(wgsl);
}

// ---------------------------------------------------------------------------
// Interpolation: mix, step, smoothstep, sign
// ---------------------------------------------------------------------------

#[test]
fn coverage_sm70_math_interp() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(1)
fn main() {
    let x: f32 = 0.3;
    let y: f32 = 0.9;
    let t: f32 = 0.5;
    out[0] = mix(x, y, t);
    out[1] = step(0.5, x);
    out[2] = smoothstep(0.0, 1.0, t);
    out[3] = sign(x - 0.5);
    out[4] = sign(-3.0);
    out[5] = sign(0.0);
}
";
    compile_fixture_sm70(wgsl);
}

// ---------------------------------------------------------------------------
// Trigonometry: tan, atan, atan2, asin, acos
// ---------------------------------------------------------------------------

#[test]
fn coverage_sm70_math_trig_extended() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(1)
fn main() {
    let x: f32 = 0.5;
    out[0] = tan(x);
    out[1] = atan(x);
    out[2] = atan2(x, 1.0 - x);
    out[3] = asin(x);
    out[4] = acos(x);
}
";
    compile_fixture_sm70(wgsl);
}

// ---------------------------------------------------------------------------
// Exponential / hyperbolic: exp, log, tanh, sinh, cosh, asinh, acosh, atanh
// ---------------------------------------------------------------------------

#[test]
fn coverage_sm70_math_exp_log() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(1)
fn main() {
    let x: f32 = 0.7;
    out[0] = exp(x);
    out[1] = log(x);
    out[2] = tanh(x);
    out[3] = sinh(x * 0.5);
    out[4] = cosh(x * 0.5);
    out[5] = asinh(x);
    out[6] = acosh(1.5);
    out[7] = atanh(0.5);
}
";
    compile_fixture_sm70(wgsl);
}

// ---------------------------------------------------------------------------
// Builtins: WorkGroupId, NumWorkGroups, LocalInvocationIndex
// ---------------------------------------------------------------------------

#[test]
fn coverage_sm70_builtins_extended() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<u32>;
@compute @workgroup_size(8)
fn main(
    @builtin(workgroup_id) wgid: vec3<u32>,
    @builtin(num_workgroups) nwg: vec3<u32>,
    @builtin(local_invocation_index) lidx: u32
) {
    out[0] = wgid.x + wgid.y * 100u + wgid.z * 10000u;
    out[1] = nwg.x;
    out[2] = lidx;
}
";
    compile_fixture_sm70(wgsl);
}

// ---------------------------------------------------------------------------
// Atomic operations: Sub, And, Or, Xor, Min, Max, Exchange
// ---------------------------------------------------------------------------

#[test]
fn coverage_sm70_atomics_add_minmax() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> counter: atomic<u32>;
@group(0) @binding(1) var<storage, read_write> out: array<u32>;
@compute @workgroup_size(1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let a = atomicAdd(&counter, 1u);
    let b = atomicMin(&counter, 0u);
    let c = atomicMax(&counter, 100u);
    out[gid.x] = a + b + c;
}
";
    compile_fixture_sm70(wgsl);
}

#[test]
fn coverage_sm70_atomics_bitwise() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> counter: atomic<u32>;
@group(0) @binding(1) var<storage, read_write> out: array<u32>;
@compute @workgroup_size(1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let a = atomicAnd(&counter, 0xFFu);
    let b = atomicOr(&counter, 0x100u);
    let c = atomicXor(&counter, 0x0Fu);
    let d = atomicExchange(&counter, 42u);
    out[gid.x] = a + b + c + d;
}
";
    compile_fixture_sm70(wgsl);
}

// ---------------------------------------------------------------------------
// Float modulo
// ---------------------------------------------------------------------------

#[test]
fn coverage_sm70_float_modulo() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(1)
fn main() {
    let a: f32 = 7.5;
    let b: f32 = 2.3;
    out[0] = a % b;
}
";
    compile_fixture_sm70(wgsl);
}

// ---------------------------------------------------------------------------
// Uniform matrix load
// ---------------------------------------------------------------------------

#[test]
fn coverage_sm70_uniform_matrix() {
    let wgsl = r"
struct Params {
    m: mat2x2<f32>,
    v: vec2<f32>,
}
@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(1) fn main() {
    let r = params.m * params.v;
    out[0] = r.x;
    out[1] = r.y;
}
";
    compile_fixture_sm70(wgsl);
}

// ---------------------------------------------------------------------------
// Signed firstLeadingBit and countLeadingZeros
// ---------------------------------------------------------------------------

#[test]
fn coverage_sm70_bitops_signed() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out_i: array<i32>;
@group(0) @binding(1) var<storage, read_write> out_u: array<u32>;
@compute @workgroup_size(1)
fn main() {
    out_i[0] = firstLeadingBit(-128i);
    out_i[1] = firstLeadingBit(16i);
    out_u[0] = countLeadingZeros(0x0000FFFFu);
    out_u[1] = countLeadingZeros(0u);
    out_u[2] = countOneBits(0xAAAAAAAAu);
}
";
    compile_fixture_sm70(wgsl);
}

// ---------------------------------------------------------------------------
// RDNA2 variants for newly tested paths
// ---------------------------------------------------------------------------

#[test]
fn coverage_rdna2_math_interp() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(1)
fn main() {
    let a: f32 = 0.0;
    let b: f32 = 1.0;
    let t: f32 = 0.5;
    out[0] = mix(a, b, t);
    out[1] = step(t, 0.3);
    out[2] = smoothstep(a, b, t);
    out[3] = sign(-3.0f);
}
";
    compile_fixture_rdna2(wgsl);
}

#[test]
fn coverage_rdna2_trig_extended() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(1)
fn main() {
    out[0] = tan(0.5);
    out[1] = atan(0.5);
    out[2] = atan2(1.0, 2.0);
    out[3] = asin(0.5);
    out[4] = acos(0.5);
}
";
    compile_fixture_rdna2(wgsl);
}

#[test]
fn coverage_rdna2_exp_log() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(1)
fn main() {
    out[0] = exp(0.7);
    out[1] = log(0.7);
    out[2] = tanh(0.7);
    out[3] = sinh(0.5);
    out[4] = cosh(0.5);
}
";
    compile_fixture_rdna2(wgsl);
}

#[test]
fn coverage_rdna2_atomics() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> counter: atomic<u32>;
@group(0) @binding(1) var<storage, read_write> out: array<u32>;
@compute @workgroup_size(1)
fn main() {
    let a = atomicAdd(&counter, 1u);
    let b = atomicMin(&counter, 0u);
    let c = atomicMax(&counter, 100u);
    let d = atomicExchange(&counter, 42u);
    out[0] = a + b + c + d;
}
";
    compile_fixture_rdna2(wgsl);
}
