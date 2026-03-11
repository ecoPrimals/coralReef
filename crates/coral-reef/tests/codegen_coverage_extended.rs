// SPDX-License-Identifier: AGPL-3.0-only
//! Extended coverage tests — memory, control flow, ALU, SM variants, AMD RDNA2.
//!
//! Split from `codegen_coverage_targeted.rs` to stay under 1000 LOC.

use std::fmt::Write;

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
// sm70_encode/mem.rs — global/shared/local memory patterns
// ---------------------------------------------------------------------------

#[test]
fn coverage_sm70_mem_shared_memory_workgroup() {
    let wgsl = r"
var<workgroup> shared_data: array<f32, 64>;

@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(64)
fn main(@builtin(local_invocation_id) lid: vec3<u32>) {
    shared_data[lid.x] = f32(lid.x);
    workgroupBarrier();
    let idx = (lid.x + 1u) % 64u;
    out[lid.x] = shared_data[idx];
}
";
    compile_fixture_sm70(wgsl);
}

#[test]
fn coverage_sm70_mem_multiple_storage_buffers() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read> a: array<f32>;
@group(0) @binding(1) var<storage, read> b: array<f32>;
@group(0) @binding(2) var<storage, read_write> c: array<f32>;
@group(0) @binding(3) var<storage, read_write> d: array<f32>;
@compute @workgroup_size(1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    c[i] = a[i] + b[i];
    d[i] = a[i] * b[i];
}
";
    compile_fixture_sm70(wgsl);
}

#[test]
fn coverage_sm70_mem_uniform_struct() {
    let wgsl = r"
struct Params {
    a: vec4<f32>,
    b: vec4<f32>,
}
@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(1) fn main() {
    let sum = params.a + params.b;
    out[0] = sum.x + sum.y + sum.z + sum.w;
}
";
    compile_fixture_sm70(wgsl);
}

// ---------------------------------------------------------------------------
// sm70_encode/control.rs — branch, loop, switch patterns
// ---------------------------------------------------------------------------

#[test]
fn coverage_sm70_control_deep_loop_nesting() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(1)
fn main() {
    var total: f32 = 0.0;
    for (var i: u32 = 0u; i < 4u; i++) {
        for (var j: u32 = 0u; j < 4u; j++) {
            for (var k: u32 = 0u; k < 4u; k++) {
                total += f32(i * 16u + j * 4u + k);
            }
        }
    }
    out[0] = total;
}
";
    compile_fixture_sm70(wgsl);
}

#[test]
fn coverage_sm70_control_early_return_in_loop() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@group(0) @binding(1) var<storage, read> inp: array<f32>;
@compute @workgroup_size(1)
fn main() {
    for (var i: u32 = 0u; i < 32u; i++) {
        if inp[i] < 0.0 {
            out[0] = f32(i);
            return;
        }
    }
    out[0] = -1.0;
}
";
    compile_fixture_sm70(wgsl);
}

#[test]
fn coverage_sm70_control_loop_with_continue() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(1)
fn main() {
    var acc: f32 = 0.0;
    for (var i: u32 = 0u; i < 64u; i++) {
        if i % 3u == 0u { continue; }
        acc += f32(i);
    }
    out[0] = acc;
}
";
    compile_fixture_sm70(wgsl);
}

#[test]
fn coverage_sm70_control_break_from_nested() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<u32>;
@compute @workgroup_size(1)
fn main() {
    var found: u32 = 0u;
    for (var i: u32 = 0u; i < 16u; i++) {
        for (var j: u32 = 0u; j < 16u; j++) {
            if i * j > 100u {
                found = i * 16u + j;
                break;
            }
        }
        if found > 0u { break; }
    }
    out[0] = found;
}
";
    compile_fixture_sm70(wgsl);
}

// ---------------------------------------------------------------------------
// sm70_encode/alu/*.rs — int ops, misc ops
// ---------------------------------------------------------------------------

#[test]
fn coverage_sm70_alu_int_bitwise_heavy() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<u32>;
@compute @workgroup_size(1)
fn main() {
    let a: u32 = 0xDEADBEEFu;
    let b: u32 = 0xCAFEBABEu;
    let c = a & b;
    let d = a | b;
    let e = a ^ b;
    let f = ~a;
    let g = countOneBits(a);
    let h = firstLeadingBit(a);
    out[0] = c;
    out[1] = d;
    out[2] = e;
    out[3] = f;
    out[4] = g;
    out[5] = h;
}
";
    compile_fixture_sm70(wgsl);
}

#[test]
fn coverage_sm70_alu_int_minmax_clamp() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<i32>;
@compute @workgroup_size(1)
fn main() {
    let a: i32 = -100;
    let b: i32 = 200;
    out[0] = min(a, b);
    out[1] = max(a, b);
    out[2] = clamp(a, -50, 50);
    out[3] = abs(a);

    let c: u32 = 100u;
    let d: u32 = 200u;
    out[4] = i32(min(c, d));
    out[5] = i32(max(c, d));
}
";
    compile_fixture_sm70(wgsl);
}

#[test]
fn coverage_sm70_alu_float_transcendentals_full() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(1)
fn main() {
    let x: f32 = 1.5;
    out[0] = sin(x);
    out[1] = cos(x);
    out[2] = exp2(x);
    out[3] = log2(x);
    out[4] = sqrt(x);
    out[5] = inverseSqrt(x);
    out[6] = floor(x);
    out[7] = ceil(x);
    out[8] = round(x);
    out[9] = trunc(x);
    out[10] = fract(x);
    out[11] = abs(x);
    out[12] = sign(x);
}
";
    compile_fixture_sm70(wgsl);
}

// ---------------------------------------------------------------------------
// sm70_encode/alu/conv.rs — type conversions
// ---------------------------------------------------------------------------

#[test]
fn coverage_sm70_conv_all_numeric_types() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out_f: array<f32>;
@group(0) @binding(1) var<storage, read_write> out_u: array<u32>;
@group(0) @binding(2) var<storage, read_write> out_i: array<i32>;
@compute @workgroup_size(1)
fn main() {
    let fi: f32 = 42.5;
    let ui: u32 = 42u;
    let si: i32 = -42;

    out_f[0] = f32(ui);
    out_f[1] = f32(si);
    out_u[0] = u32(fi);
    out_u[1] = u32(si);
    out_i[0] = i32(fi);
    out_i[1] = i32(ui);

    out_f[2] = f32(true);
    out_u[2] = u32(true);
    out_i[2] = i32(false);
}
";
    compile_fixture_sm70(wgsl);
}

// ---------------------------------------------------------------------------
// spill_values/spiller.rs — heavy register pressure
// ---------------------------------------------------------------------------

#[test]
fn coverage_spill_loop_with_high_pressure() {
    let mut wgsl = String::from(
        "@group(0) @binding(0) var<storage, read_write> out: array<f32>;\n\
         @group(0) @binding(1) var<storage, read> inp: array<f32>;\n\
         @compute @workgroup_size(1) fn main() {\n",
    );
    for i in 0..64 {
        let _ = writeln!(wgsl, "  var v{i} = inp[{i}];");
    }
    wgsl.push_str("  for (var iter: u32 = 0u; iter < 4u; iter++) {\n");
    for i in 0..64 {
        let prev = if i == 0 { 63 } else { i - 1 };
        let _ = writeln!(wgsl, "    v{i} = v{i} + v{prev} * 0.5;");
    }
    wgsl.push_str("  }\n");
    for i in 0..64 {
        let _ = writeln!(wgsl, "  out[{i}] = v{i};");
    }
    wgsl.push_str("}\n");
    compile_fixture_sm70(&wgsl);
}

#[test]
fn coverage_spill_branchy_with_many_live() {
    let mut wgsl = String::from(
        "@group(0) @binding(0) var<storage, read_write> out: array<f32>;\n\
         @group(0) @binding(1) var<storage, read> inp: array<f32>;\n\
         @compute @workgroup_size(1) fn main() {\n",
    );
    for i in 0..48 {
        let _ = writeln!(wgsl, "  let v{i} = inp[{i}];");
    }
    for i in 0..48 {
        let _ = writeln!(
            wgsl,
            "  if v{i} > 0.0 {{ out[{i}] = v{i} * 2.0; }} else {{ out[{i}] = v{i} + 1.0; }}"
        );
    }
    wgsl.push_str("}\n");
    compile_fixture_sm70(&wgsl);
}

// ---------------------------------------------------------------------------
// SM variant coverage (SM75, SM80, SM86, SM89)
// ---------------------------------------------------------------------------

#[test]
fn coverage_sm75_turing_compute_shader() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@group(0) @binding(1) var<storage, read> inp: array<f32>;
@compute @workgroup_size(32)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    let a = inp[i];
    let b = inp[i + 32u];
    out[i] = fma(a, b, a + b);
}
";
    compile_raw_sm(wgsl, 75);
}

#[test]
fn coverage_sm80_ampere_compute_shader() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@group(0) @binding(1) var<storage, read> inp: array<f32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    var sum: f32 = 0.0;
    for (var j: u32 = 0u; j < 8u; j++) {
        sum += inp[i * 8u + j];
    }
    out[i] = sqrt(abs(sum));
}
";
    compile_raw_sm(wgsl, 80);
}

#[test]
fn coverage_sm86_ga102_compute_shader() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<u32>;
@compute @workgroup_size(1)
fn main() {
    var a: u32 = 1u;
    for (var i: u32 = 0u; i < 20u; i++) {
        a = a * 3u + 1u;
    }
    out[0] = a;
}
";
    compile_raw_sm(wgsl, 86);
}

#[test]
fn coverage_sm89_ada_compute_shader() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(1)
fn main() {
    var val: f32 = 1.0;
    for (var i: u32 = 0u; i < 10u; i++) {
        val = sin(val) + cos(val);
    }
    out[0] = val;
}
";
    compile_raw_sm(wgsl, 89);
}

// ---------------------------------------------------------------------------
// AMD RDNA2 encoding paths
// ---------------------------------------------------------------------------

#[test]
fn coverage_rdna2_memory_heavy() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@group(0) @binding(1) var<storage, read> a: array<f32>;
@group(0) @binding(2) var<storage, read> b: array<f32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    out[i] = a[i] + b[i] * 2.0;
}
";
    compile_fixture_rdna2(wgsl);
}

#[test]
fn coverage_rdna2_control_flow() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@group(0) @binding(1) var<storage, read> inp: array<f32>;
@compute @workgroup_size(1)
fn main() {
    var sum: f32 = 0.0;
    for (var i: u32 = 0u; i < 32u; i++) {
        let val = inp[i];
        if val > 0.0 {
            sum += val;
        } else {
            sum -= val;
        }
        if sum > 1000.0 { break; }
    }
    out[0] = sum;
}
";
    compile_fixture_rdna2(wgsl);
}

#[test]
fn coverage_rdna2_transcendentals() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(1)
fn main() {
    let x: f32 = 0.7;
    out[0] = sin(x);
    out[1] = cos(x);
    out[2] = exp2(x);
    out[3] = log2(x);
    out[4] = sqrt(x);
    out[5] = floor(x);
    out[6] = ceil(x);
}
";
    compile_fixture_rdna2(wgsl);
}

#[test]
fn coverage_rdna2_integer_ops() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<u32>;
@compute @workgroup_size(1)
fn main() {
    let a = 0xABCDu;
    let b = 0x1234u;
    out[0] = a + b;
    out[1] = a - b;
    out[2] = a * b;
    out[3] = a & b;
    out[4] = a | b;
    out[5] = a ^ b;
    out[6] = a << 4u;
    out[7] = a >> 4u;
    out[8] = min(a, b);
    out[9] = max(a, b);
    out[10] = countOneBits(a);
}
";
    compile_fixture_rdna2(wgsl);
}

#[test]
fn coverage_rdna2_type_conversions() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out_f: array<f32>;
@group(0) @binding(1) var<storage, read_write> out_u: array<u32>;
@compute @workgroup_size(1)
fn main() {
    let f: f32 = 42.7;
    let u: u32 = 100u;
    let i: i32 = -50;
    out_f[0] = f32(u);
    out_f[1] = f32(i);
    out_u[0] = u32(f);
    out_u[1] = u32(abs(i));
}
";
    compile_fixture_rdna2(wgsl);
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
