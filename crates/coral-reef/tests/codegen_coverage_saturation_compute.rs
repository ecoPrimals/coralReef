// SPDX-License-Identifier: AGPL-3.0-or-later
//! WGSL integration saturation — workgroup, kernel, edge-case, and legacy encoder patterns.
//!
//! Split from `codegen_coverage_saturation.rs` (data-operation tests remain there).
//! Each fixture targets distinct compute patterns: shared memory, real-world kernels,
//! edge cases (DCE, const folding, unreachable), and legacy SM20/SM50 raw paths.

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

fn compile_wgsl_raw_sm(wgsl: &str, sm: u8) {
    let bin = coral_reef::compile_wgsl_raw_sm(wgsl, sm).expect("compile_wgsl_raw_sm");
    assert!(!bin.is_empty(), "SM{sm}: empty binary");
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
// 36–40: "Real-world-ish" kernels
// -----------------------------------------------------------------------------

#[test]
fn sat_kernel_matmul_tile_2x2() {
    let wgsl = r"
var<workgroup> sa: array<f32, 16>;
var<workgroup> sb: array<f32, 16>;
@group(0) @binding(0) var<storage, read> a: array<f32>;
@group(0) @binding(1) var<storage, read> b: array<f32>;
@group(0) @binding(2) var<storage, read_write> c: array<f32>;
@compute @workgroup_size(4, 4, 1)
fn main(@builtin(local_invocation_id) lid: vec3<u32>, @builtin(workgroup_id) wid: vec3<u32>) {
    let row = wid.y * 4u + lid.y;
    let col = wid.x * 4u + lid.x;
    let i = lid.y * 4u + lid.x;
    sa[i] = a[row * 8u + lid.x];
    sb[i] = b[lid.y * 8u + col];
    workgroupBarrier();
    var acc: f32 = 0.0;
    for (var k: u32 = 0u; k < 4u; k = k + 1u) {
        acc = acc + sa[lid.y * 4u + k] * sb[k * 4u + lid.x];
    }
    c[row * 8u + col] = acc;
}
";
    compile_fixture_all_nv(wgsl);
}

#[test]
fn sat_kernel_conv_1d_three_tap() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read> inp: array<f32>;
@group(0) @binding(1) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i0 = gid.x;
    let s = inp[i0] * 0.25 + inp[i0 + 1u] * 0.5 + inp[i0 + 2u] * 0.25;
    out[gid.x] = s;
}
";
    compile_fixture_all_nv(wgsl);
}

#[test]
fn sat_kernel_bitonic_sort_network_8() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> k: array<f32>;
@compute @workgroup_size(1)
fn main() {
    var a: array<f32, 8>;
    for (var i: u32 = 0u; i < 8u; i = i + 1u) {
        a[i] = k[i];
    }
    for (var p: u32 = 0u; p < 3u; p = p + 1u) {
        for (var q: u32 = 0u; q < 3u - p; q = q + 1u) {
            let step = 1u << (3u - p - q);
            let grp = 1u << (2u - p + q);
            for (var i: u32 = 0u; i < 8u; i = i + 1u) {
                if (i / step) % 2u == 0u {
                    let j = i + step;
                    if (i / grp) % 2u == 0u {
                        if a[i] > a[j] {
                            let t = a[i];
                            a[i] = a[j];
                            a[j] = t;
                        }
                    } else {
                        if a[i] < a[j] {
                            let t = a[i];
                            a[i] = a[j];
                            a[j] = t;
                        }
                    }
                }
            }
        }
    }
    for (var i: u32 = 0u; i < 8u; i = i + 1u) {
        k[i] = a[i];
    }
}
";
    compile_fixture_all_nv(wgsl);
}

#[test]
fn sat_kernel_fft_butterfly_dit_f32() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> re: array<f32>;
@group(0) @binding(1) var<storage, read_write> im: array<f32>;
@compute @workgroup_size(1)
fn main() {
    let a0 = re[0];
    let b0 = re[1];
    let ai = im[0];
    let bi = im[1];
    let ang = 1.57079632679;
    let wr = cos(ang);
    let wi = sin(ang);
    let t0 = wr * b0 - wi * bi;
    let t1 = wr * bi + wi * b0;
    re[0] = a0 + t0;
    re[1] = a0 - t0;
    im[0] = ai + t1;
    im[1] = ai - t1;
}
";
    compile_fixture_all_nv(wgsl);
}

#[test]
fn sat_kernel_norm_l2_vec4() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read> inp: array<vec4<f32>>;
@group(0) @binding(1) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let v = inp[gid.x];
    out[gid.x] = length(v) + normalize(v).x;
}
";
    compile_fixture_all_nv(wgsl);
}

// -----------------------------------------------------------------------------
// 41–45: Edge cases
// -----------------------------------------------------------------------------

#[test]
fn sat_edge_single_store() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> o: array<f32>;
@compute @workgroup_size(1)
fn main() {
    o[0] = 1.0;
}
";
    compile_fixture_all_nv(wgsl);
}

#[test]
fn sat_edge_empty_loop_break_immediate() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> o: array<f32>;
@compute @workgroup_size(1)
fn main() {
    loop {
        break;
    }
    o[0] = 2.0;
}
";
    compile_fixture_all_nv(wgsl);
}

#[test]
fn sat_edge_dead_assignments() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> o: array<f32>;
@compute @workgroup_size(1)
fn main() {
    var x: f32 = 3.0;
    var y: f32 = 4.0;
    x = y * 2.0;
    y = x + 1.0;
    o[0] = 5.0;
}
";
    compile_fixture_all_nv(wgsl);
}

#[test]
fn sat_edge_unreachable_branch() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> o: array<f32>;
@compute @workgroup_size(1)
fn main() {
    let f = false;
    if f {
        o[0] = 1.0;
        return;
    }
    o[0] = 2.0;
}
";
    compile_fixture_all_nv(wgsl);
}

#[test]
fn sat_edge_const_fold_heavy() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> o: array<f32>;
@compute @workgroup_size(1)
fn main() {
    let a = 1.0 + 2.0 * 3.0 - 4.0 / 2.0;
    let b = (a + 0.1) * (a - 0.05);
    o[0] = b;
}
";
    compile_fixture_all_nv(wgsl);
}

// -----------------------------------------------------------------------------
// Extra: legacy encoder paths (SM20 / SM50)
// -----------------------------------------------------------------------------

#[test]
fn sat_raw_sm20_loop_trig() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(1)
fn main() {
    var x: f32 = 1.0;
    var i: u32 = 0u;
    loop {
        if i >= 6u {
            break;
        }
        x = fma(x, 1.02, sin(f32(i)));
        i = i + 1u;
    }
    out[0] = x;
}
";
    compile_wgsl_raw_sm(wgsl, 20);
}

#[test]
fn sat_raw_sm20_int_switch() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> o: array<i32>;
@compute @workgroup_size(1)
fn main() {
    var x: i32 = 2i;
    switch x {
        case 0i: { x = 1i; }
        case 1i: { x = 2i; }
        case 2i: { x = 3i; }
        default: { x = 4i; }
    }
    o[0] = x;
}
";
    compile_wgsl_raw_sm(wgsl, 20);
}

#[test]
fn sat_raw_sm50_vec_dot_length() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read> a: array<vec4<f32>>;
@group(0) @binding(1) var<storage, read_write> o: array<f32>;
@compute @workgroup_size(32)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let v = a[gid.x];
    o[gid.x] = dot(v, v * 2.0) + length(v);
}
";
    compile_wgsl_raw_sm(wgsl, 50);
}

#[test]
fn sat_raw_sm50_and_all_nv_u32_lfsr() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> s: array<u32>;
@compute @workgroup_size(1)
fn main() {
    var x: u32 = 0xC0FFEEu;
    for (var k: u32 = 0u; k < 20u; k = k + 1u) {
        x = x ^ (x << 13u);
        x = x ^ (x >> 17u);
        x = x ^ (x << 5u);
    }
    s[0] = x;
}
";
    compile_wgsl_raw_sm(wgsl, 50);
    compile_fixture_all_nv(wgsl);
}
