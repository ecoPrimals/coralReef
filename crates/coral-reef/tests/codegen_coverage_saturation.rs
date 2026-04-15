// SPDX-License-Identifier: AGPL-3.0-or-later
//! WGSL integration saturation — data-operation pipeline (`naga` → IR → opt → encode).
//!
//! Sections 1–30: integer arithmetic, float builtins, vector/matrix, control flow,
//! memory access patterns, and type conversions. Workgroup/kernel/edge/legacy tests
//! are in `codegen_coverage_saturation_compute.rs`.

use coral_reef::{CompileOptions, GpuTarget, NvArch};

fn opts_for(nv: NvArch) -> CompileOptions {
    CompileOptions {
        target: GpuTarget::Nvidia(nv),
        opt_level: 2,
        debug_info: false,
        fp64_software: true,
        ..CompileOptions::default()
    }
}

fn compile_for(wgsl: &str, nv: NvArch) -> Result<Vec<u8>, coral_reef::CompileError> {
    coral_reef::compile_wgsl(wgsl, &opts_for(nv))
}

fn compile_fixture_all_nv(wgsl: &str) {
    for &nv in NvArch::ALL {
        match compile_for(wgsl, nv) {
            Ok(bin) => assert!(!bin.is_empty(), "{nv}: empty binary"),
            Err(e) => panic!("{nv}: {e}"),
        }
    }
}

// -----------------------------------------------------------------------------
// 1–5: Integer arithmetic (signed/unsigned, overflow-ish patterns, div/mod, mul_hi)
// -----------------------------------------------------------------------------

#[test]
fn sat_int_signed_chain_abs_clamp() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> o: array<i32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let u = gid.x % 32u;
    let x = i32(u);
    let n = -x;
    let p = x * -3i;
    let q = clamp(n + p, -40i, 40i);
    o[gid.x] = abs(q);
}
";
    compile_fixture_all_nv(wgsl);
}

#[test]
fn sat_int_unsigned_bitops_rotate_mix() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> o: array<u32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let a = gid.x ^ 0xA5A5A5A5u;
    let b = (a << 3u) | (a >> 29u);
    let c = (b & 0xFF00FFu) | ((b & 0x00FF00FFu) << 8u);
    o[gid.x] = c + countOneBits(a) + countLeadingZeros(b);
}
";
    compile_fixture_all_nv(wgsl);
}

#[test]
fn sat_int_div_mod_signed_unsigned() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> si: array<i32>;
@group(0) @binding(1) var<storage, read_write> su: array<u32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = i32(gid.x % 32u);
    let di = i / max(i, 1i);
    let mi = i % max(i, 1i);
    let u = gid.x + 3u;
    let du = u / 11u;
    let mu = u % 11u;
    si[gid.x] = di + mi;
    su[gid.x] = du + mu;
}
";
    compile_fixture_all_nv(wgsl);
}

#[test]
fn sat_int_mul_hi_u32_manual() {
    let wgsl = r"
fn u32_mul_hi(a: u32, b: u32) -> u32 {
    let a_lo = a & 0xFFFFu;
    let a_hi = a >> 16u;
    let b_lo = b & 0xFFFFu;
    let b_hi = b >> 16u;
    let t = a_lo * b_lo;
    let u = a_lo * b_hi + a_hi * b_lo + (t >> 16u);
    return a_hi * b_hi + (u >> 16u);
}
@group(0) @binding(0) var<storage, read_write> o: array<u32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let a = gid.x * 2654435761u;
    let b = gid.y * 2246822519u + 1u;
    o[gid.x] = u32_mul_hi(a, b) + (a * b);
}
";
    compile_fixture_all_nv(wgsl);
}

#[test]
fn sat_int_overflow_wrap_negate_patterns() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> o: array<i32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let k = i32(gid.x % 64u);
    let hi = 2147483647i;
    let a = hi - k;
    let b = -2147483647i - 1i + k;
    o[gid.x] = a + b / max(k, 1i);
}
";
    compile_fixture_all_nv(wgsl);
}

// -----------------------------------------------------------------------------
// 6–10: Float special functions (trig, exp/log, pow, mixed)
// -----------------------------------------------------------------------------

#[test]
fn sat_float_trig_sin_cos_tan() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> o: array<f32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let t = f32(gid.x) * 0.0314159265;
    o[gid.x] = sin(t) * cos(t * 2.0) + tan(t * 0.25);
}
";
    compile_fixture_all_nv(wgsl);
}

#[test]
fn sat_float_atan2_asin_acos_hypot() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> o: array<f32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let x = f32(gid.x % 64u) * 0.1 - 3.0;
    let y = f32(gid.y % 8u) * 0.07 + 0.25;
    let h = sqrt(x * x + y * y);
    o[gid.x] = atan2(y, x + 0.001) + asin(clamp(x * 0.1, -1.0, 1.0)) + acos(clamp(y * 0.05, -1.0, 1.0)) + h;
}
";
    compile_fixture_all_nv(wgsl);
}

#[test]
fn sat_float_exp_log_pow() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> o: array<f32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let x = max(f32(gid.x) * 0.02, 0.001);
    let e = exp2(log2(x) * 1.5);
    let p = pow(x, 3.3);
    o[gid.x] = e + p + log(x + 1.0);
}
";
    compile_fixture_all_nv(wgsl);
}

#[test]
fn sat_float_sqrt_rsqrt_inversesqrt() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> o: array<f32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let x = f32(gid.x % 128u) * 0.1 + 0.25;
    o[gid.x] = sqrt(x) * inverseSqrt(x + 0.5) + 1.0 / x;
}
";
    compile_fixture_all_nv(wgsl);
}

#[test]
fn sat_float_degrees_radians_step_smoothstep() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> o: array<f32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let a = f32(gid.x);
    let rad = a * 0.1 * 0.01745329252;
    let s = step(10.0, a);
    let m = smoothstep(5.0, 15.0, a);
    o[gid.x] = sin(rad) + cos(rad * 0.5) + s + m;
}
";
    compile_fixture_all_nv(wgsl);
}

// -----------------------------------------------------------------------------
// 11–15: Vector/matrix construction and decomposition
// -----------------------------------------------------------------------------

#[test]
fn sat_vec234_swizzle_arithmetic() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> o: array<f32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let a = vec2<f32>(f32(gid.x), f32(gid.y));
    let b = vec3<f32>(a.x, a.y, a.x + a.y);
    let c = vec4<f32>(b.xy, b.z, 1.0);
    o[gid.x] = c.x * c.y + c.z + c.w + b.z;
}
";
    compile_fixture_all_nv(wgsl);
}

#[test]
fn sat_mat2_construct_mul_vec() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> o: array<f32>;
@compute @workgroup_size(1)
fn main() {
    let m = mat2x2<f32>(vec2<f32>(2.0, 3.0), vec2<f32>(5.0, 7.0));
    let v = vec2<f32>(11.0, 13.0);
    let r = m * v;
    o[0] = r.x + r.y + m[0].x + m[1].y;
}
";
    compile_fixture_all_nv(wgsl);
}

#[test]
fn sat_mat3_outer_dot_cross() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> o: array<f32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let u = vec3<f32>(f32(gid.x), f32(gid.y), 1.0);
    let v = vec3<f32>(0.5, -0.25, 2.0);
    let m = mat3x3<f32>(u * v.x, u * v.y, u * v.z);
    let c = cross(u, v);
    o[gid.x] = dot(m[0], c) + dot(m[1], v);
}
";
    compile_fixture_all_nv(wgsl);
}

#[test]
fn sat_mat4_index_columns_rows() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> o: array<f32>;
@compute @workgroup_size(1)
fn main() {
    let m = mat4x4<f32>(
        vec4<f32>(1.0, 0.0, 0.0, 0.0),
        vec4<f32>(0.0, 2.0, 0.0, 0.0),
        vec4<f32>(0.0, 0.0, 3.0, 0.0),
        vec4<f32>(0.0, 0.0, 0.0, 4.0),
    );
    let a = vec4<f32>(1.0, 1.0, 1.0, 1.0);
    let r = m * a;
    o[0] = m[2].z + r.w + m[0].x + m[3].w;
}
";
    compile_fixture_all_nv(wgsl);
}

#[test]
fn sat_vec_bool_select_mix() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> o: array<f32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let a = vec2<f32>(1.0, 2.0);
    let b = vec2<f32>(3.0, 4.0);
    let m = vec2<bool>(gid.x % 2u == 0u, gid.x % 3u == 0u);
    let s = select(a, b, m);
    o[gid.x] = mix(s.x, s.y, 0.25);
}
";
    compile_fixture_all_nv(wgsl);
}

// -----------------------------------------------------------------------------
// 16–20: Control flow (switch, nested loops, early return, loop result, continue)
// -----------------------------------------------------------------------------

#[test]
fn sat_cfg_switch_dense_cases() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> o: array<f32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let x = gid.x % 7u;
    var acc: f32 = 0.0;
    switch x {
        case 0u: { acc = 1.0; }
        case 1u: { acc = 2.0; }
        case 2u: { acc = 3.0; }
        case 3u: { acc = 4.0; }
        case 4u: { acc = 5.0; }
        case 5u: { acc = 6.0; }
        default: { acc = 7.0; }
    }
    o[gid.x] = acc + f32(gid.x);
}
";
    compile_fixture_all_nv(wgsl);
}

#[test]
fn sat_cfg_nested_loops_break_continue() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> o: array<f32>;
@compute @workgroup_size(1)
fn main() {
    var t: f32 = 0.0;
    var i: u32 = 0u;
    loop {
        if i >= 4u { break; }
        var j: u32 = 0u;
        loop {
            if j >= 4u { break; }
            var k: u32 = 0u;
            loop {
                if k >= 4u { break; }
                if (i + j + k) % 3u == 1u {
                    k = k + 1u;
                    continue;
                }
                t = t + f32(i * j + k);
                k = k + 1u;
            }
            j = j + 1u;
        }
        i = i + 1u;
    }
    o[0] = t;
}
";
    compile_fixture_all_nv(wgsl);
}

#[test]
fn sat_cfg_early_return_guard() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> o: array<f32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    if gid.x == 0u {
        o[0] = 99.0;
        return;
    }
    if gid.x == 1u {
        o[1] = 17.0;
        return;
    }
    o[gid.x] = f32(gid.x) * 0.1;
}
";
    compile_fixture_all_nv(wgsl);
}

/// Staged loop result (`naga` WGSL in this tree does not accept `break <expr>` form).
#[test]
fn sat_cfg_loop_break_with_value() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> o: array<i32>;
@compute @workgroup_size(1)
fn main() {
    var r: i32 = 0i;
    loop {
        r = 42i;
        break;
    }
    o[0] = r;
}
";
    compile_fixture_all_nv(wgsl);
}

#[test]
fn sat_cfg_for_while_style_accum() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> o: array<u32>;
@compute @workgroup_size(1)
fn main() {
    var s: u32 = 0u;
    for (var i: u32 = 0u; i < 16u; i = i + 1u) {
        s = s + i * i;
    }
    var j: u32 = 0u;
    loop {
        if j >= 8u {
            break;
        }
        s = s ^ (j * 31u);
        j = j + 1u;
    }
    o[0] = s;
}
";
    compile_fixture_all_nv(wgsl);
}

// -----------------------------------------------------------------------------
// 21–25: Memory access patterns (stride, vec loads, scattered writes, reduction)
// -----------------------------------------------------------------------------

#[test]
fn sat_mem_stride_xor_indexing() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> m: array<f32>;
@compute @workgroup_size(128)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    let j = i ^ 7u;
    let k = (i * 3u + 5u) % 128u;
    m[i] = m[j] * 0.5 + m[k] * 0.25 + f32(i & 15u);
}
";
    compile_fixture_all_nv(wgsl);
}

#[test]
fn sat_mem_vec4_load_store() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read> a: array<vec4<f32>>;
@group(0) @binding(1) var<storage, read_write> b: array<vec4<f32>>;
@compute @workgroup_size(32)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    let v = a[i];
    let w = v * vec4<f32>(1.0, -1.0, 0.5, 2.0) + vec4<f32>(f32(i & 3u));
    b[i] = vec4<f32>(dot(w, v), w.x, w.y + w.z, w.w);
}
";
    compile_fixture_all_nv(wgsl);
}

#[test]
fn sat_mem_scattered_writes_prime_stride() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> m: array<u32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let n = 64u;
    let i = gid.x;
    let t = (i * 17u + 3u) % n;
    m[t] = m[t] + gid.x;
    m[(t ^ 31u) % n] = m[(t ^ 31u) % n] + 1u;
}
";
    compile_fixture_all_nv(wgsl);
}

#[test]
fn sat_mem_reduction_serial_simd_style() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read> inp: array<f32>;
@group(0) @binding(1) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(1)
fn main() {
    var s: f32 = 0.0;
    for (var i: u32 = 0u; i < 32u; i = i + 1u) {
        s = s + inp[i];
    }
    out[0] = s;
}
";
    compile_fixture_all_nv(wgsl);
}

#[test]
fn sat_mem_dynamic_array_length_style() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> buf: array<f32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x % 32u;
    buf[i] = buf[i] + buf[(i + 13u) % 32u];
}
";
    compile_fixture_all_nv(wgsl);
}

// -----------------------------------------------------------------------------
// 26–30: Type conversion chains (i/f/u, f64 software, bitcast)
// -----------------------------------------------------------------------------

#[test]
fn sat_conv_i32_f32_u32_chain() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> o: array<f32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let u = gid.x;
    let i = i32(u);
    let f = f32(i);
    let u2 = u32(floor(f + 0.7));
    o[gid.x] = f32(u2 & 255u);
}
";
    compile_fixture_all_nv(wgsl);
}

#[test]
fn sat_conv_f64_vec_ops() {
    let wgsl = r"
enable f64;
@group(0) @binding(0) var<storage, read_write> o: array<f64>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let x = f64(gid.x);
    let y = x * 1.414213562373095048 + f64(gid.y);
    let v = vec2<f64>(y, x * 0.5);
    let w = v * v + vec2<f64>(2.0, 3.0);
    o[gid.x] = dot(v, vec2<f64>(1.0, -1.0)) + w.x + f64(abs(v.x) * 0.001);
}
";
    compile_fixture_all_nv(wgsl);
}

#[test]
fn sat_conv_bitcast_roundtrip_f32_u32() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> o: array<u32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let x = f32(gid.x) * 0.1;
    let u = bitcast<u32>(x);
    let y = bitcast<f32>(u);
    o[gid.x] = u ^ bitcast<u32>(y + 1.0);
}
";
    compile_fixture_all_nv(wgsl);
}

#[test]
fn sat_conv_u32_i32_trunc_sat() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> o: array<i32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let u = gid.x + 0xFFFF0000u;
    let i = i32(u);
    o[gid.x] = i + i32(gid.x & 255u);
}
";
    compile_fixture_all_nv(wgsl);
}

#[test]
fn sat_conv_mixed_vec_promotion() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> o: array<f32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let iv = vec3<i32>(i32(gid.x), -3i, 17i);
    let fv = vec3<f32>(f32(iv.x), f32(iv.y), f32(iv.z));
    let uv = vec3<u32>(u32(clamp(fv.x, 0.0, 100.0)), gid.y, 3u);
    o[gid.x] = f32(uv.x + uv.y + uv.z) + dot(fv, vec3<f32>(0.1, 0.2, 0.3));
}
";
    compile_fixture_all_nv(wgsl);
}
