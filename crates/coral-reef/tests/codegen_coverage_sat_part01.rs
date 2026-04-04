// SPDX-License-Identifier: AGPL-3.0-only
//! WGSL codegen saturation coverage (part 1 / 3).

#[path = "codegen_sat/helpers.rs"]
mod codegen_coverage_sat_helpers;
use codegen_coverage_sat_helpers::*;

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
