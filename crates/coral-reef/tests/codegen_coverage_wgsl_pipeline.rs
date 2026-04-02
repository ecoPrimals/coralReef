// SPDX-License-Identifier: AGPL-3.0-only
#![cfg(feature = "naga")]
//! Full-pipeline WGSL compilation tests across all [`coral_reef::NvArch`] targets.
//!
//! Each fixture exercises naga translate → IR → optimization → legalization → NV encoding.
//! Focus: control flow, memory/atomics, barriers, conversions, f16/f64, register pressure.

use std::fmt::Write;

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

/// Compile `wgsl` for every supported NVIDIA architecture (SM35–SM120).
fn compile_fixture_all_nv(wgsl: &str) {
    for &nv in NvArch::ALL {
        match compile_for(wgsl, nv) {
            Ok(bin) => assert!(!bin.is_empty(), "{nv}: empty binary"),
            Err(e) => panic!("{nv}: {e}"),
        }
    }
}

// =============================================================================
// Control flow: nested branches, switch, loops with break/continue, early return
// =============================================================================

#[test]
fn pipeline_cfg_nested_if_else_switch_early_return() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let x = gid.x;
    var acc: f32 = 0.0;
    if x < 4u {
        acc = acc + 1.0;
        if x < 2u {
            acc = acc + 2.0;
        } else {
            acc = acc + 3.0;
        }
    } else if x < 8u {
        acc = acc + 4.0;
    } else {
        acc = acc + 5.0;
    }
    var sel: u32 = x % 5u;
    switch sel {
        case 0u: { acc = acc + 0.1; }
        case 1u: { acc = acc + 0.2; }
        case 2u: { acc = acc + 0.3; }
        default: { acc = acc + 0.9; }
    }
    var i: u32 = 0u;
    loop {
        if i >= 6u { break; }
        if i == 3u {
            i = i + 1u;
            continue;
        }
        acc = acc + f32(i) * 0.01;
        i = i + 1u;
    }
    if x == 17u {
        out[gid.x] = 99.0;
        return;
    }
    out[gid.x] = acc + f32(x);
}
";
    compile_fixture_all_nv(wgsl);
}

#[test]
fn pipeline_cfg_triple_nested_loops_break() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(1)
fn main() {
    var t: f32 = 0.0;
    var a: u32 = 0u;
    loop {
        if a >= 5u { break; }
        var b: u32 = 0u;
        loop {
            if b >= 5u { break; }
            var c: u32 = 0u;
            loop {
                if c >= 5u { break; }
                if a + b + c == 7u {
                    t = t + 1.0;
                }
                c = c + 1u;
            }
            b = b + 1u;
        }
        a = a + 1u;
    }
    out[0] = t;
}
";
    compile_fixture_all_nv(wgsl);
}

#[test]
fn pipeline_cfg_for_while_style_and_dense_switch() {
    let mut wgsl = String::from(
        "@group(0) @binding(0) var<storage, read_write> out: array<f32>;\n\
         @compute @workgroup_size(1) fn main() {\n  let sel: u32 = 3u;\n  var acc: f32 = 0.0;\n  switch sel {\n",
    );
    for i in 0..16_u32 {
        let _ = writeln!(wgsl, "    case {i}u: {{ acc = acc + f32({i}); }}");
    }
    wgsl.push_str(
        "    default: { acc = 7.0; }\n  }\n  for (var k: i32 = 0; k < 8; k = k + 1) {\n\
         acc = acc + f32(k);\n  }\n  out[0] = acc;\n}\n",
    );
    compile_fixture_all_nv(&wgsl);
}

// =============================================================================
// Atomics — `atomicLoad`/`atomicSub`/`atomicExchange` chains currently trigger
// `opt_copy_prop` assertion failures; use add/min/max/bitwise/CAS paths that
// match existing stable coverage (`codegen_coverage_extended` / multi_arch).
// =============================================================================

#[test]
fn pipeline_atomics_u32_add_min_max() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> counter: atomic<u32>;
@group(0) @binding(1) var<storage, read_write> out: array<u32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let a = atomicAdd(&counter, 1u);
    let b = atomicMin(&counter, 0u);
    let c = atomicMax(&counter, 100u);
    out[gid.x] = a + b + c;
}
";
    compile_fixture_all_nv(wgsl);
}

#[test]
fn pipeline_atomics_u32_bitwise_and_exchange() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> counter: atomic<u32>;
@group(0) @binding(1) var<storage, read_write> out: array<u32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let a = atomicAnd(&counter, 0xFFu);
    let b = atomicOr(&counter, 0x100u);
    let c = atomicXor(&counter, 0x0Fu);
    let d = atomicExchange(&counter, 42u);
    out[gid.x] = a + b + c + d;
}
";
    compile_fixture_all_nv(wgsl);
}

/// `atomicCompareExchangeWeak` path (kept separate to avoid copy-prop edge cases with CAS).
#[test]
fn pipeline_atomics_compare_exchange_weak_u32() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> a: atomic<u32>;
@group(0) @binding(1) var<storage, read_write> out: array<u32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    atomicStore(&a, 8u);
    let r = atomicCompareExchangeWeak(&a, 8u, 99u);
    out[gid.x] = r.old_value + select(0u, 1u, r.exchanged);
}
";
    compile_fixture_all_nv(wgsl);
}

// =============================================================================
// Barriers: workgroup + storage
// =============================================================================

#[test]
fn pipeline_workgroup_and_storage_barriers() {
    let wgsl = r"
var<workgroup> scratch: array<u32, 128>;
@group(0) @binding(0) var<storage, read_write> mem: array<u32>;
@compute @workgroup_size(128)
fn main(@builtin(local_invocation_id) lid: vec3<u32>) {
    scratch[lid.x] = lid.x;
    workgroupBarrier();
    let v = scratch[127u - lid.x];
    mem[lid.x] = v;
    storageBarrier();
    let w = mem[(lid.x + 1u) % 128u];
    mem[lid.x] = w + 1u;
}
";
    compile_fixture_all_nv(wgsl);
}

// =============================================================================
// Type conversions: scalars and vectors, mixed select
// =============================================================================

#[test]
fn pipeline_conversions_i_u_f_vec_mat() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let ux = gid.x;
    let ix = i32(ux);
    let fx = f32(ix);
    let u2 = u32(floor(fx));
    let v2 = vec2<u32>(ux, u2);
    let v3 = vec3<i32>(ix, ix + 1i, ix - 1i);
    let vf = vec3<f32>(f32(v3.x), f32(v3.y), f32(v3.z));
    let m = mat3x3<f32>(
        vec3<f32>(1.0, 0.0, 0.0),
        vec3<f32>(0.0, 1.0, 0.0),
        vec3<f32>(0.0, 0.0, 1.0),
    );
    let r = m * vf;
    let pick = select(vec2<f32>(0.0, 1.0), vec2<f32>(2.0, 3.0), vec2<bool>(ux < 10u, ix > 0i));
    out[gid.x] = r.x + r.y + r.z + pick.x + pick.y + f32(v2.x & 7u);
}
";
    compile_fixture_all_nv(wgsl);
}

// =============================================================================
// Half-precision-style vector math (f32): `enable f16` is stripped by
// `prepare_wgsl`, so we use packed vec4<f32> patterns instead of `f16` types.
// =============================================================================

#[test]
fn pipeline_vec4_packed_math_conversions() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let h = vec4<f32>(f32(gid.x) * 0.01, 1.0, 2.0, 3.0);
    let g = h * vec4<f32>(2.0) + vec4<f32>(0.5);
    let s = g.x + g.y + g.z + g.w;
    let u = vec2<u32>(gid.x, gid.y);
    let uf = vec2<f32>(f32(u.x), f32(u.y));
    out[gid.x] = s + uf.x * 0.001 + uf.y * 0.002;
}
";
    compile_fixture_all_nv(wgsl);
}

// =============================================================================
// Structs, arrays, dynamic indexing
// =============================================================================

#[test]
fn pipeline_struct_nested_and_array_indexing() {
    let wgsl = r"
struct Inner {
    z: f32,
    w: vec2<f32>,
}
struct Outer {
    tag: u32,
    body: array<Inner, 3>,
}
@group(0) @binding(0) var<storage, read_write> buf: array<Outer>;
@group(0) @binding(1) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x % 8u;
    let o = buf[i];
    let j = o.tag % 3u;
    let inn = o.body[j];
    let t = inn.z + inn.w.x * inn.w.y;
    buf[i].body[0u].z = t + 1.0;
    out[gid.x] = t;
}
";
    compile_fixture_all_nv(wgsl);
}

// =============================================================================
// Register pressure: many live SSA values (spiller / RA)
// =============================================================================

#[test]
fn pipeline_register_pressure_many_live_vars() {
    let mut wgsl = String::from(
        "@group(0) @binding(0) var<storage, read_write> out: array<f32>;\n\
         @compute @workgroup_size(1) fn main() {\n",
    );
    for i in 0..48 {
        let _ = writeln!(wgsl, "  var v{i}: f32 = f32({i}u);");
    }
    for i in 0..48 {
        let j = (i + 13) % 48;
        let _ = writeln!(wgsl, "  v{i} = v{i} * 1.001 + v{j} * 0.0001;");
    }
    wgsl.push_str("  out[0] = v0 + v47 + v23;\n}\n");
    compile_fixture_all_nv(&wgsl);
}

// =============================================================================
// Matrix multiply + manual outer-product columns (`transpose` / `determinant`
// are not implemented on all SM targets in naga_translate).
// =============================================================================

#[test]
fn pipeline_matrix_mul_and_outer_manual() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(1)
fn main() {
    let m = mat4x4<f32>(
        vec4<f32>(2.0, 0.0, 0.0, 0.0),
        vec4<f32>(0.0, 3.0, 0.0, 0.0),
        vec4<f32>(0.0, 0.0, 4.0, 0.0),
        vec4<f32>(0.0, 0.0, 0.0, 5.0),
    );
    let a = vec4<f32>(1.0, 1.0, 1.0, 1.0);
    let b = vec4<f32>(0.5, 0.25, 0.125, 1.0);
    let op = mat4x4<f32>(a * b.x, a * b.y, a * b.z, a * b.w);
    let r = m * a;
    out[0] = r.x + r.y + r.z + r.w + op[0].x + op[1].y;
}
";
    compile_fixture_all_nv(wgsl);
}

// =============================================================================
// f64 software paths + control flow (shader header / dfma paths)
// =============================================================================

#[test]
fn pipeline_f64_branching_and_vec_ops() {
    let wgsl = r"
enable f64;
@group(0) @binding(0) var<storage, read_write> out: array<f64>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let x = f64(gid.x);
    var y = x * 1.414213562373095048;
    if x > 10.0 {
        y = y + sin(x * 0.1);
    } else {
        y = y - cos(x * 0.05);
    }
    let v = vec2<f64>(y, x);
    let n = normalize(v + vec2<f64>(1.0, 0.0));
    out[gid.x] = dot(n, v);
}
";
    compile_fixture_all_nv(wgsl);
}

// =============================================================================
// Integer division / remainder (signed and unsigned)
// =============================================================================

#[test]
fn pipeline_int_div_mod_and_abs() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> oi: array<i32>;
@group(0) @binding(1) var<storage, read_write> ou: array<u32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = i32(gid.x % 32u);
    let a = i / max(i, 1i);
    let b = i % max(i, 1i);
    let u = gid.x;
    let c = u / 7u;
    let d = u % 7u;
    oi[gid.x] = abs(a + b);
    ou[gid.x] = c + d;
}
";
    compile_fixture_all_nv(wgsl);
}

// =============================================================================
// Copy-prop friendly chains + builtins (step, smoothstep, mix)
// =============================================================================

#[test]
fn pipeline_let_chains_interp_builtins() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let t = f32(gid.x) * 0.01;
    let a = t + 1.0;
    let b = a;
    let c = b;
    let lo = vec3<f32>(0.0, 0.1, 0.2);
    let hi = vec3<f32>(1.0, 1.1, 1.2);
    let m = mix(lo, hi, vec3<f32>(c));
    let s = smoothstep(0.0, 1.0, t);
    let st = step(0.5, t);
    out[gid.x] = m.x + m.y + m.z + s + st;
}
";
    compile_fixture_all_nv(wgsl);
}

// =============================================================================
// Builtin: local_invocation_index + packed workgroup addressing
// =============================================================================

#[test]
fn pipeline_builtin_local_invocation_index_workgroup_id() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<u32>;
@compute @workgroup_size(8, 4, 2)
fn main(
    @builtin(workgroup_id) wgid: vec3<u32>,
    @builtin(local_invocation_index) lidx: u32,
) {
    let flat = wgid.x * 1000u + wgid.y * 100u + lidx;
    out[0] = flat;
}
";
    compile_fixture_all_nv(wgsl);
}

// =============================================================================
// More math / vector builtins (fract, modf, inverseSqrt, reflect, refract)
// =============================================================================

#[test]
fn pipeline_math_fract_inverse_sqrt_fma() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let x = f32(gid.x) * 0.03 + 0.1;
    let fr = fract(x);
    let inv = inverseSqrt(x + 1.0);
    let y = fma(x, 0.5, fr);
    out[gid.x] = fr + inv + y;
}
";
    compile_fixture_all_nv(wgsl);
}

#[test]
fn pipeline_vec_cross_length_normalize() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let a = vec3<f32>(f32(gid.x), 1.0, 0.5);
    let b = vec3<f32>(0.0, 1.0, 0.0);
    let c = cross(a, b);
    let u = normalize(a + b);
    out[gid.x] = dot(c, u) + length(a);
}
";
    compile_fixture_all_nv(wgsl);
}

#[test]
fn pipeline_math_atan2_clamp() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let x = f32(gid.x) * 0.1;
    let a = atan2(x, 1.0);
    let c = clamp(x, 0.0, 5.0);
    let m = min(max(x * 0.2, 0.0), 1.0);
    out[gid.x] = a + c + m;
}
";
    compile_fixture_all_nv(wgsl);
}

// =============================================================================
// Bitcast + u32 bit patterns (float↔uint reinterpret)
// =============================================================================

#[test]
fn pipeline_bitcast_f32_u32_roundtrip() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<u32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let x = f32(gid.x) * 0.1 + 1.0;
    let u = bitcast<u32>(x);
    let y = bitcast<f32>(u);
    out[gid.x] = u ^ bitcast<u32>(y);
}
";
    compile_fixture_all_nv(wgsl);
}

// =============================================================================
// Uniform buffer + mat2x2 (loads / matrix × vector)
// =============================================================================

#[test]
fn pipeline_uniform_mat2_and_vec2_mul() {
    let wgsl = r"
struct Ubo {
    m: mat2x2<f32>,
    v: vec2<f32>,
}
@group(0) @binding(0) var<uniform> ubo: Ubo;
@group(0) @binding(1) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let r = ubo.m * ubo.v;
    out[gid.x] = r.x + r.y + ubo.m[0].x;
}
";
    compile_fixture_all_nv(wgsl);
}

// =============================================================================
// Array copy pattern + dynamic index (storage → locals → storage)
// =============================================================================

#[test]
fn pipeline_array_copy_block_dynamic_indices() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> buf: array<f32, 64>;
@compute @workgroup_size(64)
fn main(@builtin(local_invocation_id) lid: vec3<u32>) {
    var tmp: array<f32, 8>;
    let base = lid.x % 8u;
    for (var k: u32 = 0u; k < 8u; k = k + 1u) {
        tmp[k] = buf[(base + k) % 64u];
    }
    var s: f32 = 0.0;
    for (var j: u32 = 0u; j < 8u; j = j + 1u) {
        s = s + tmp[j];
    }
    buf[lid.x] = s;
}
";
    compile_fixture_all_nv(wgsl);
}
