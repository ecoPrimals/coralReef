// SPDX-License-Identifier: AGPL-3.0-only
//! WGSL codegen saturation coverage (part 2 / 3).

#[path = "codegen_sat/helpers.rs"]
mod codegen_coverage_sat_helpers;
use codegen_coverage_sat_helpers::*;

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

// -----------------------------------------------------------------------------
// 31–35: Workgroup shared memory patterns
// -----------------------------------------------------------------------------

#[test]
fn sat_wg_prefix_sum_hillis_steele_small() {
    let wgsl = r"
var<workgroup> s: array<u32, 32>;
@group(0) @binding(0) var<storage, read_write> o: array<u32>;
@compute @workgroup_size(32)
fn main(@builtin(local_invocation_id) lid: vec3<u32>) {
    let i = lid.x;
    s[i] = i + 1u;
    workgroupBarrier();
    var off: u32 = 1u;
    loop {
        if off >= 32u {
            break;
        }
        workgroupBarrier();
        var t: u32 = s[i];
        if i >= off {
            t = t + s[i - off];
        }
        workgroupBarrier();
        s[i] = t;
        off = off * 2u;
    }
    workgroupBarrier();
    o[i] = s[i];
}
";
    compile_fixture_all_nv(wgsl);
}

#[test]
fn sat_wg_histogram_atomic_bins() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> bins: array<atomic<u32>, 16>;
@group(0) @binding(1) var<storage, read_write> scratch: array<u32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let v = gid.x % 16u;
    let prev = atomicAdd(&bins[v], 1u);
    let merged = atomicMax(&bins[(v + 1u) % 16u], prev);
    scratch[gid.x] = merged;
}
";
    compile_fixture_all_nv(wgsl);
}

#[test]
fn sat_wg_transpose_tile_4x4() {
    let wgsl = r"
var<workgroup> tile: array<f32, 16>;
@group(0) @binding(0) var<storage, read_write> o: array<f32>;
@compute @workgroup_size(16)
fn main(@builtin(local_invocation_id) lid: vec3<u32>) {
    let i = lid.x;
    let r = i / 4u;
    let c = i % 4u;
    tile[i] = f32(r * 10u + c);
    workgroupBarrier();
    let tr = c;
    let tc = r;
    let j = tr * 4u + tc;
    workgroupBarrier();
    o[i] = tile[j] + f32(i);
}
";
    compile_fixture_all_nv(wgsl);
}

#[test]
fn sat_wg_reduction_tree_pairwise() {
    let wgsl = r"
var<workgroup> t: array<f32, 32>;
@group(0) @binding(0) var<storage, read_write> o: array<f32>;
@compute @workgroup_size(32)
fn main(@builtin(local_invocation_id) lid: vec3<u32>) {
    let i = lid.x;
    t[i] = f32(i + 1u);
    workgroupBarrier();
    var stride: u32 = 16u;
    loop {
        if stride == 0u {
            break;
        }
        workgroupBarrier();
        if i < stride {
            t[i] = t[i] + t[i + stride];
        }
        stride = stride / 2u;
    }
    workgroupBarrier();
    if i == 0u {
        o[0] = t[0];
    }
}
";
    compile_fixture_all_nv(wgsl);
}

#[test]
fn sat_wg_ping_pong_barriers() {
    let wgsl = r"
var<workgroup> a: array<f32, 64>;
var<workgroup> b: array<f32, 64>;
@group(0) @binding(0) var<storage, read_write> o: array<f32>;
@compute @workgroup_size(64)
fn main(@builtin(local_invocation_id) lid: vec3<u32>) {
    let i = lid.x;
    a[i] = f32(i);
    workgroupBarrier();
    b[i] = a[i] * 2.0;
    workgroupBarrier();
    a[i] = b[(i + 1u) % 64u] + 1.0;
    workgroupBarrier();
    o[i] = a[i];
}
";
    compile_fixture_all_nv(wgsl);
}

// -----------------------------------------------------------------------------
// 36–40: “Real-world-ish” kernels
// -----------------------------------------------------------------------------
