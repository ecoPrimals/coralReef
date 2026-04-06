// SPDX-License-Identifier: AGPL-3.0-or-later
//! Deep integration coverage — scheduling, spilling, copy propagation, CFG, memory,
//! integer/float ALU, barriers, phi lowering, legalization, textures, and legacy SM paths.
//!
//! Each test targets multiple codegen modules (`sm70`/`sm120` latencies, spiller,
//! `opt_copy_prop`, `opt_bar_prop`, `naga_translate`, encoders, `legalize`, etc.).

use std::fmt::Write;

use coral_reef::{CompileError, CompileOptions, GpuTarget, NvArch};

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

/// Compile `wgsl` for every supported NVIDIA architecture (SM35–SM120).
fn compile_fixture_all_nv(wgsl: &str) {
    for &nv in NvArch::ALL {
        let bin = compile_for(wgsl, nv).unwrap_or_else(|e| panic!("{nv}: {e}"));
        assert!(!bin.is_empty(), "{nv}: empty binary");
    }
}

fn compile_wgsl_raw_sm(wgsl: &str, sm: u8) {
    let bin = coral_reef::compile_wgsl_raw_sm(wgsl, sm)
        .unwrap_or_else(|e| panic!("SM{sm} compile failed: {e}"));
    assert!(!bin.is_empty(), "SM{sm}: empty binary");
}

/// `textureLoad` in compute is not wired for all targets yet; succeed on `Ok` or `NotImplemented`.
#[test]
fn deep_texture_load_try_each_nv() {
    let wgsl = r"
@group(0) @binding(0) var tex: texture_2d<f32>;
@group(0) @binding(1) var<storage, read_write> out: array<vec4<f32>>;
@compute @workgroup_size(16)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    out[gid.x] = textureLoad(tex, vec2<i32>(i32(gid.x), 0), 0);
}
";
    for &nv in NvArch::ALL {
        match compile_for(wgsl, nv) {
            Ok(bin) => assert!(!bin.is_empty(), "{nv}: empty binary"),
            Err(CompileError::NotImplemented(_)) => {}
            Err(e) => panic!("{nv}: {e}"),
        }
    }
}

// =============================================================================
// Instruction scheduling — long chains, ILP, mixed ALU + memory (SM70/SM120)
// =============================================================================

#[test]
fn deep_sched_long_serial_fma_then_load_store() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> mem: array<f32>;
@group(0) @binding(1) var<storage, read> inp: array<f32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    var a = inp[i];
    a = fma(a, 1.01, inp[i ^ 1u]);
    a = fma(a, 1.02, inp[i ^ 2u]);
    a = fma(a, 1.03, inp[i ^ 3u]);
    a = fma(a, 1.04, mem[i]);
    a = fma(a, 1.05, inp[i ^ 5u]);
    a = fma(a, 1.06, inp[i ^ 6u]);
    a = fma(a, 1.07, inp[i ^ 7u]);
    a = fma(a, 1.08, mem[i ^ 8u]);
    mem[i] = a;
}
";
    compile_fixture_all_nv(wgsl);
}

#[test]
fn deep_sched_ilp_four_independent_chains() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@group(0) @binding(1) var<storage, read> a: array<f32>;
@group(0) @binding(2) var<storage, read> b: array<f32>;
@compute @workgroup_size(128)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    var x = a[i];
    var y = b[i];
    var u = a[i ^ 3u];
    var v = b[i ^ 5u];
    x = fma(x, y, u);
    y = fma(y, u, v);
    u = fma(u, v, x);
    v = fma(v, x, y);
    x = sqrt(abs(x)) + floor(y);
    y = sqrt(abs(y)) + ceil(u);
    u = sqrt(abs(u)) + fract(v);
    v = sqrt(abs(v)) + trunc(x);
    out[i] = x + y + u + v;
}
";
    compile_fixture_all_nv(wgsl);
}

#[test]
fn deep_sched_interleaved_global_and_shared() {
    let wgsl = r"
var<workgroup> scratch: array<f32, 128>;
@group(0) @binding(0) var<storage, read_write> g: array<f32>;
@compute @workgroup_size(128)
fn main(@builtin(local_invocation_id) lid: vec3<u32>, @builtin(global_invocation_id) gid: vec3<u32>) {
    let i = lid.x;
    scratch[i] = g[gid.x];
    workgroupBarrier();
    let j = (i + 7u) % 128u;
    let t = fma(scratch[i], scratch[j], g[gid.x ^ 1u]);
    workgroupBarrier();
    scratch[j] = t;
    workgroupBarrier();
    g[gid.x] = scratch[i] + scratch[(i + 1u) % 128u];
}
";
    compile_fixture_all_nv(wgsl);
}

// =============================================================================
// Spilling — 64+ simultaneously live scalars
// =============================================================================

#[test]
fn deep_spill_sixty_four_u32_live_across_block() {
    let mut wgsl = String::from(
        "@group(0) @binding(0) var<storage, read_write> out: array<u32>;\n\
         @compute @workgroup_size(1) fn main() {\n",
    );
    for i in 0..64 {
        let _ = writeln!(wgsl, "  var r{i}: u32 = {i}u;");
    }
    for i in 0..64 {
        let prev = if i == 0 { 63 } else { i - 1 };
        let _ = writeln!(wgsl, "  r{i} = r{i} ^ r{prev} ^ {i}u;");
    }
    wgsl.push_str("  var acc: u32 = 0u;\n");
    for i in 0..64 {
        let _ = writeln!(wgsl, "  acc = acc + r{i};");
    }
    wgsl.push_str("  out[0] = acc;\n}\n");
    compile_fixture_all_nv(&wgsl);
}

#[test]
fn deep_spill_sixty_four_f32_with_loop_carried_deps() {
    let mut wgsl = String::from(
        "@group(0) @binding(0) var<storage, read_write> out: array<f32>;\n\
         @group(0) @binding(1) var<storage, read> inp: array<f32>;\n\
         @compute @workgroup_size(1) fn main() {\n",
    );
    for i in 0..64 {
        let _ = writeln!(wgsl, "  var s{i} = inp[{i}];");
    }
    wgsl.push_str("  for (var k: u32 = 0u; k < 3u; k = k + 1u) {\n");
    for i in 0..64 {
        let nxt = (i + 1) % 64;
        let _ = writeln!(wgsl, "    s{i} = fma(s{i}, s{nxt}, inp[{i}]);");
    }
    wgsl.push_str("  }\n");
    for i in 0..64 {
        let _ = writeln!(wgsl, "  out[{i}] = s{i};");
    }
    wgsl.push_str("}\n");
    compile_fixture_all_nv(&wgsl);
}

// =============================================================================
// Copy propagation — deep `let` alias chains
// =============================================================================

#[test]
fn deep_copy_prop_chain_of_lets_sixty_four() {
    let mut wgsl = String::from(
        "@group(0) @binding(0) var<storage, read_write> out: array<f32>;\n\
         @compute @workgroup_size(1) fn main() {\n  let seed = out[0];\n",
    );
    for i in 0..64 {
        let _ = writeln!(wgsl, "  let a{i} = seed + f32({i}u);");
    }
    wgsl.push_str("  var s: f32 = 0.0;\n");
    for i in 0..64 {
        let _ = writeln!(wgsl, "  s = s + a{i};");
    }
    wgsl.push_str("  out[1] = s;\n}\n");
    compile_fixture_all_nv(&wgsl);
}

#[test]
fn deep_copy_prop_zigzag_aliases() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(1)
fn main() {
    let a0 = out[0];
    let b0 = a0;
    let c0 = b0;
    let a1 = c0 + 1.0;
    let b1 = a1;
    let c1 = b1;
    let a2 = c1 * 2.0;
    let b2 = a2;
    let c2 = b2;
    out[1] = c2;
}
";
    compile_fixture_all_nv(wgsl);
}

// =============================================================================
// Control flow — large switch, deep nesting, loops with complex exits
// =============================================================================

#[test]
fn deep_cfg_switch_twenty_four_cases() {
    let mut wgsl = String::from(
        "@group(0) @binding(0) var<storage, read_write> out: array<u32>;\n\
         @compute @workgroup_size(1) fn main() {\n  let sel = out[0] % 29u;\n  var r: u32 = 0u;\n  switch sel {\n",
    );
    for i in 0..24_u32 {
        let _ = writeln!(wgsl, "    case {i}u: {{ r = {i}u * 11u; }}");
    }
    wgsl.push_str("    default: { r = 999u; }\n  }\n  out[1] = r;\n}\n");
    compile_fixture_all_nv(&wgsl);
}

#[test]
fn deep_cfg_nested_if_six_levels() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let x = gid.x;
    var acc: f32 = 0.0;
    if x < 100u {
        if x < 50u {
            if x < 25u {
                if x < 12u {
                    if x < 6u {
                        if x < 3u {
                            acc = 1.0;
                        } else {
                            acc = 2.0;
                        }
                    } else {
                        acc = 3.0;
                    }
                } else {
                    acc = 4.0;
                }
            } else {
                acc = 5.0;
            }
        } else {
            acc = 6.0;
        }
    } else {
        acc = 7.0;
    }
    out[x] = acc;
}
";
    compile_fixture_all_nv(wgsl);
}

#[test]
fn deep_cfg_loop_break_continue_complex() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(1)
fn main() {
    var acc: f32 = 0.0;
    var i: u32 = 0u;
    loop {
        if i >= 40u {
            break;
        }
        if i % 5u == 0u {
            i = i + 1u;
            continue;
        }
        if i > 30u && acc > 100.0 {
            break;
        }
        acc = acc + f32(i);
        i = i + 1u;
    }
    out[0] = acc;
}
";
    compile_fixture_all_nv(wgsl);
}

// =============================================================================
// Memory — multiple bindings, vector loads, structured uniform access
// =============================================================================

#[test]
fn deep_mem_five_bindings_vec4_stride() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read> a: array<vec4<f32>>;
@group(0) @binding(1) var<storage, read> b: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read_write> c: array<vec4<f32>>;
@group(0) @binding(3) var<storage, read_write> d: array<vec4<f32>>;
@group(0) @binding(4) var<storage, read_write> e: array<f32>;
@compute @workgroup_size(32)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    let va = a[i];
    let vb = b[i];
    let m = va * vb + vec4<f32>(1.0, 2.0, 3.0, 4.0);
    c[i] = m;
    d[i] = vec4<f32>(dot(m, vb), dot(va, vb), m.x, m.y);
    e[i] = m.z + m.w;
}
";
    compile_fixture_all_nv(wgsl);
}

#[test]
fn deep_mem_uniform_struct_nested() {
    let wgsl = r"
struct Inner {
    scale: vec2<f32>,
    bias: f32,
}
struct Params {
    a: Inner,
    b: vec4<f32>,
}
@group(0) @binding(0) var<uniform> p: Params;
@group(0) @binding(1) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(8)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let s = p.a.scale * p.a.bias;
    let t = p.b * vec4<f32>(s.x, s.y, 1.0, 1.0);
    out[gid.x] = dot(t, p.b);
}
";
    compile_fixture_all_nv(wgsl);
}

// =============================================================================
// Integer ALU — bitwise, shifts, widening-style chains (32-bit)
// =============================================================================

#[test]
fn deep_int_alu_bitwise_shifts_rotate_style() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<u32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let x = gid.x;
    let a = (x << 3u) | (x >> 29u);
    let b = (a & 0x55555555u) | ((a & 0xAAAAAAAAu) >> 1u);
    let c = countOneBits(b) + countLeadingZeros(b);
    let d = firstLeadingBit(b);
    let e = reverseBits(b ^ 0x12345678u);
    out[x] = a + b + c + d + e;
}
";
    compile_fixture_all_nv(wgsl);
}

#[test]
fn deep_int_alu_i32_min_max_abs_clamp_chain() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<i32>;
@compute @workgroup_size(32)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let xi = i32(gid.x);
    let a = min(xi, 100);
    let b = max(xi, -50);
    let c = clamp(xi, -10, 10);
    let d = abs(a - b);
    let e = c + d;
    out[gid.x] = e;
}
";
    compile_fixture_all_nv(wgsl);
}

#[test]
fn deep_int_alu_u32_mul_add_chain() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<u32>;
@compute @workgroup_size(1)
fn main() {
    var x: u32 = 3u;
    x = x * 7u + 11u;
    x = x * 13u + 17u;
    x = x * 19u + 23u;
    x = x * 29u + 31u;
    x = x ^ (x >> 4u);
    x = x * 37u + 41u;
    out[0] = x;
}
";
    compile_fixture_all_nv(wgsl);
}

// =============================================================================
// Float ALU — fma, min/max, abs, sign, comparisons
// =============================================================================

#[test]
fn deep_float_alu_fma_min_max_abs_sign_chain() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let t = f32(gid.x) * 0.03;
    let a = fma(t, 2.0, -1.0);
    let b = min(max(a, -0.5), 0.5);
    let c = abs(b);
    let d = sign(a) * c;
    let e = select(0.0, 1.0, d > 0.0);
    out[gid.x] = fma(d, e, sqrt(abs(a) + 0.01));
}
";
    compile_fixture_all_nv(wgsl);
}

#[test]
fn deep_float_alu_comparison_vec_and_scalar() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(32)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let x = f32(gid.x);
    let v = vec3<f32>(x, x + 1.0, x - 1.0);
    let w = vec3<f32>(x * 0.5, x * 0.25, x * 0.125);
    let m1 = v < w;
    let m2 = v >= w;
    let s1 = select(1.0, 0.0, m1.x);
    let s2 = select(2.0, 0.0, m2.y);
    out[gid.x] = s1 + s2 + length(v - w);
}
";
    compile_fixture_all_nv(wgsl);
}

// =============================================================================
// Barrier propagation — shared memory + repeated barriers
// =============================================================================

#[test]
fn deep_barrier_prop_scan_then_block_reduce() {
    let wgsl = r"
var<workgroup> buf: array<f32, 64>;
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(64)
fn main(@builtin(local_invocation_id) lid: vec3<u32>) {
    let i = lid.x;
    buf[i] = f32(i);
    workgroupBarrier();
    buf[i] = buf[i] + buf[(i + 32u) % 64u];
    workgroupBarrier();
    buf[i] = buf[i] + buf[(i + 16u) % 64u];
    workgroupBarrier();
    buf[i] = buf[i] + buf[(i + 8u) % 64u];
    workgroupBarrier();
    if i == 0u {
        out[0] = buf[0] + buf[1];
    }
}
";
    compile_fixture_all_nv(wgsl);
}

#[test]
fn deep_barrier_shared_reload_pattern() {
    let wgsl = r"
var<workgroup> a: array<f32, 32>;
var<workgroup> b: array<f32, 32>;
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(32)
fn main(@builtin(local_invocation_id) lid: vec3<u32>) {
    let i = lid.x;
    a[i] = f32(i * i);
    workgroupBarrier();
    b[i] = a[(31u - i) % 32u];
    workgroupBarrier();
    a[i] = a[i] + b[i];
    workgroupBarrier();
    out[u32(i)] = a[i];
}
";
    compile_fixture_all_nv(wgsl);
}

// =============================================================================
// Phi / copy-swap — multiple paths merging into one variable
// =============================================================================

#[test]
fn deep_phi_switch_like_ladder_on_one_var() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let k = gid.x % 7u;
    var v: f32 = 0.0;
    if k == 0u {
        v = 1.0;
    } else if k == 1u {
        v = 2.0;
    } else if k == 2u {
        v = 3.0;
    } else if k == 3u {
        v = 4.0;
    } else if k == 4u {
        v = 5.0;
    } else if k == 5u {
        v = 6.0;
    } else {
        v = 7.0;
    }
    out[gid.x] = v + f32(k);
}
";
    compile_fixture_all_nv(wgsl);
}

#[test]
fn deep_phi_loop_with_conditional_store() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(1)
fn main() {
    var acc: f32 = 0.0;
    for (var n: u32 = 0u; n < 16u; n = n + 1u) {
        if n % 2u == 0u {
            acc = acc + f32(n);
        } else {
            acc = acc - f32(n) * 0.5;
        }
    }
    out[0] = acc;
}
";
    compile_fixture_all_nv(wgsl);
}

// =============================================================================
// Legalization-friendly constants and overflow-ish immediates (bounded)
// =============================================================================

#[test]
fn deep_legalize_large_literal_arithmetic() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<u32>;
@compute @workgroup_size(1)
fn main() {
    let a: u32 = 0xFFFF0000u;
    let b: u32 = 0x0000FFFFu;
    let c = a + b;
    let d = a * 2u + b;
    let e = (c ^ d) + 0x13579BDFu;
    out[0] = e;
}
";
    compile_fixture_all_nv(wgsl);
}

// =============================================================================
// Surface-style buffer I/O (tiling / gather-scatter; texture bindings are optional elsewhere)
// =============================================================================

#[test]
fn deep_surface_style_buffer_tiles_and_scatter() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read> src: array<vec4<f32>>;
@group(0) @binding(1) var<storage, read_write> dst: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read_write> aux: array<f32>;
@compute @workgroup_size(32)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    let s = src[i];
    let t = src[(i + 5u) % 128u];
    let u = fma(s, t, vec4<f32>(0.1, 0.2, 0.3, 0.4));
    let v = vec4<f32>(dot(u, s), dot(u, t), u.x, u.y);
    dst[i] = v;
    aux[i] = length(v - s) + length(v - t);
}
";
    compile_fixture_all_nv(wgsl);
}

// =============================================================================
// f64 software lowering
// =============================================================================

#[test]
fn deep_f64_dot_mat2_chain() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f64>;
@group(0) @binding(1) var<storage, read> a: array<vec2<f64>>;
@group(0) @binding(2) var<storage, read> b: array<vec2<f64>>;
@compute @workgroup_size(16)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    let u = a[i];
    let v = b[i];
    let m = mat2x2<f64>(
        vec2<f64>(1.0, 2.0),
        vec2<f64>(3.0, 4.0),
    );
    let t = m * u;
    out[i] = dot(t, v) + f64(i);
}
";
    compile_fixture_all_nv(wgsl);
}

// =============================================================================
// Legacy SM20 / SM50 targeted encoders
// =============================================================================

#[test]
fn deep_sm20_fermi_control_and_alu() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(1)
fn main() {
    var x: f32 = 2.0;
    var i: u32 = 0u;
    loop {
        if i >= 8u {
            break;
        }
        x = fma(x, 1.1, f32(i));
        i = i + 1u;
    }
    out[0] = sin(x) * cos(x);
}
";
    compile_wgsl_raw_sm(wgsl, 20);
}

#[test]
fn deep_sm50_maxwell_int_mem_and_vec() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read> in0: array<vec4<f32>>;
@group(0) @binding(1) var<storage, read_write> out0: array<vec4<f32>>;
@compute @workgroup_size(32)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    let v = in0[i];
    let w = v * v + vec4<f32>(0.25, 0.5, 0.75, 1.0);
    out0[i] = vec4<f32>(dot(w, v), length(w), w.x, w.y);
}
";
    compile_wgsl_raw_sm(wgsl, 50);
}

#[test]
fn deep_sm50_and_all_nv_shared_path() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> o: array<u32>;
@compute @workgroup_size(1)
fn main() {
    var x: u32 = 1u;
    for (var k: u32 = 0u; k < 12u; k = k + 1u) {
        x = (x * 1315423911u) ^ (x >> 15u);
    }
    o[0] = x;
}
";
    compile_wgsl_raw_sm(wgsl, 50);
    compile_fixture_all_nv(wgsl);
}

// =============================================================================
// Combined stress — one shader touching several gaps at once
// =============================================================================

#[test]
fn deep_combo_sched_spill_cfg_float_int_barrier() {
    let wgsl = r"
var<workgroup> wg: array<f32, 32>;
@group(0) @binding(0) var<storage, read_write> mem: array<f32>;
@group(0) @binding(1) var<storage, read> inp: array<f32>;
@compute @workgroup_size(32)
fn main(@builtin(local_invocation_id) lid: vec3<u32>, @builtin(global_invocation_id) gid: vec3<u32>) {
    let i = lid.x;
    var a = inp[gid.x];
    var b = inp[gid.x ^ 1u];
    var c = inp[gid.x ^ 2u];
    var d = inp[gid.x ^ 3u];
    var e = inp[gid.x ^ 4u];
    var f = inp[gid.x ^ 5u];
    var g = inp[gid.x ^ 6u];
    var h = inp[gid.x ^ 7u];
    a = fma(a, b, c);
    b = fma(b, c, d);
    c = fma(c, d, e);
    d = fma(d, e, f);
    e = fma(e, f, g);
    f = fma(f, g, h);
    g = fma(g, h, a);
    h = fma(h, a, b);
    let sel = gid.x % 5u;
    var acc: f32 = 0.0;
    switch sel {
        case 0u: { acc = a; }
        case 1u: { acc = b; }
        case 2u: { acc = c; }
        case 3u: { acc = d; }
        default: { acc = e + f; }
    }
    wg[i] = acc;
    workgroupBarrier();
    mem[gid.x] = wg[i] + wg[(i + 1u) % 32u] + f32(countOneBits(gid.x));
}
";
    compile_fixture_all_nv(wgsl);
}

#[test]
fn deep_naga_expr_func_ops_surface_style_buffers() {
    let wgsl = r"
struct P {
    m: mat3x3<f32>,
    v: vec3<f32>,
}
@group(0) @binding(0) var<uniform> p: P;
@group(0) @binding(1) var<storage, read_write> f: array<f32>;
@group(0) @binding(2) var<storage, read_write> u: array<u32>;
@compute @workgroup_size(16)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    let t = p.m * p.v;
    let n = normalize(t);
    f[i] = dot(n, p.v) + length(t);
    let bits = u32(i) * 0x9E3779B9u;
    u[i] = countOneBits(bits) + firstLeadingBit(bits);
}
";
    compile_fixture_all_nv(wgsl);
}
