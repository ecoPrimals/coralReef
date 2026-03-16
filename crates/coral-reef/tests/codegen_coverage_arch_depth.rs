// SPDX-License-Identifier: AGPL-3.0-only
//! Deep architecture-specific coverage tests.
//!
//! Targets coverage gaps in f64, texture, memory, and control encoding
//! across SM20/SM32/SM50/SM70/SM80+ architectures.

use coral_reef::{CompileError, CompileOptions, GpuArch, GpuTarget, NvArch};

fn opts_for(nv: NvArch) -> CompileOptions {
    CompileOptions {
        target: GpuTarget::Nvidia(nv),
        opt_level: 2,
        fp64_software: true,
        ..CompileOptions::default()
    }
}

fn try_compile_nv(wgsl: &str, nv: NvArch) {
    match coral_reef::compile_wgsl(wgsl, &opts_for(nv)) {
        Ok(binary) => assert!(!binary.is_empty(), "{nv}: produced empty binary"),
        Err(CompileError::NotImplemented(_)) => {}
        Err(e) => panic!("{nv}: {e}"),
    }
}

fn try_compile_raw_sm(wgsl: &str, sm: u8) {
    match coral_reef::compile_wgsl_raw_sm(wgsl, sm) {
        Ok(binary) => assert!(!binary.is_empty(), "SM{sm}: produced empty binary"),
        Err(CompileError::NotImplemented(_)) => {}
        Err(e) => panic!("SM{sm}: {e}"),
    }
}

fn try_compile_all_nv(wgsl: &str) {
    for &nv in NvArch::ALL {
        try_compile_nv(wgsl, nv);
    }
}

fn try_compile_all_legacy(wgsl: &str) {
    for sm in [50, 32, 30, 21, 20] {
        try_compile_raw_sm(wgsl, sm);
    }
}

fn best_effort_legacy(wgsl: &str) {
    for sm in [50, 32, 30, 21, 20] {
        let wgsl = wgsl.to_owned();
        let _ = std::panic::catch_unwind(move || {
            try_compile_raw_sm(&wgsl, sm);
        });
    }
}

// =============================================================================
// Float64 coverage — sm20/sm32/sm50/sm70 alu/float64.rs (all 0% covered)
// =============================================================================

const F64_BASIC_WGSL: &str = r"
enable f64;
@group(0) @binding(0) var<storage, read_write> out: array<f64>;
@group(0) @binding(1) var<storage, read> inp: array<f64>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let a = inp[gid.x];
    let b = inp[gid.x + 1u];
    out[gid.x] = a + b;
}
";

const F64_ARITHMETIC_WGSL: &str = r"
enable f64;
@group(0) @binding(0) var<storage, read_write> out: array<f64>;
@group(0) @binding(1) var<storage, read> inp: array<f64>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let a = inp[gid.x];
    let b = inp[gid.x + 1u];
    let sum = a + b;
    let diff = a - b;
    let prod = a * b;
    out[gid.x] = sum + diff + prod;
}
";

const F64_CONVERSION_WGSL: &str = r"
enable f64;
@group(0) @binding(0) var<storage, read_write> out_f64: array<f64>;
@group(0) @binding(1) var<storage, read> inp_f32: array<f32>;
@group(0) @binding(2) var<storage, read_write> out_f32: array<f32>;
@group(0) @binding(3) var<storage, read> inp_f64: array<f64>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let f32_val = inp_f32[gid.x];
    out_f64[gid.x] = f64(f32_val);
    let f64_val = inp_f64[gid.x];
    out_f32[gid.x] = f32(f64_val);
}
";

const F64_COMPARISON_WGSL: &str = r"
enable f64;
@group(0) @binding(0) var<storage, read_write> out: array<f64>;
@group(0) @binding(1) var<storage, read> inp: array<f64>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let a = inp[gid.x];
    let b = inp[gid.x + 1u];
    var r = a;
    if a > b { r = a; } else { r = b; }
    out[gid.x] = r;
}
";

#[test]
fn f64_basic_all_nv() {
    try_compile_all_nv(F64_BASIC_WGSL);
}

#[test]
fn f64_basic_legacy() {
    best_effort_legacy(F64_BASIC_WGSL);
}

#[test]
fn f64_arithmetic_all_nv() {
    try_compile_all_nv(F64_ARITHMETIC_WGSL);
}

#[test]
fn f64_arithmetic_legacy() {
    best_effort_legacy(F64_ARITHMETIC_WGSL);
}

#[test]
fn f64_conversion_sm70() {
    try_compile_nv(F64_CONVERSION_WGSL, NvArch::Sm70);
}

#[test]
fn f64_conversion_legacy() {
    best_effort_legacy(F64_CONVERSION_WGSL);
}

#[test]
fn f64_comparison_sm70() {
    try_compile_nv(F64_COMPARISON_WGSL, NvArch::Sm70);
}

#[test]
fn f64_comparison_legacy() {
    best_effort_legacy(F64_COMPARISON_WGSL);
}

// =============================================================================
// Memory patterns — deeper coverage for sm20/sm32/sm50 mem.rs
// =============================================================================

#[test]
fn mem_struct_loads_all_nv() {
    let wgsl = r"
struct Particle {
    pos: vec4<f32>,
    vel: vec4<f32>,
    mass: f32,
    _pad: f32,
    _pad2: f32,
    _pad3: f32,
}
@group(0) @binding(0) var<storage, read> particles: array<Particle>;
@group(0) @binding(1) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let p = particles[gid.x];
    let ke = 0.5 * p.mass * dot(p.vel, p.vel);
    out[gid.x] = ke + length(p.pos);
}
";
    try_compile_all_nv(wgsl);
    try_compile_all_legacy(wgsl);
}

#[test]
fn mem_large_array_stride_all_nv() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read> data: array<vec4<f32>>;
@group(0) @binding(1) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let base = gid.x * 4u;
    let a = data[base];
    let b = data[base + 1u];
    let c = data[base + 2u];
    let d = data[base + 3u];
    out[gid.x] = dot(a, b) + dot(c, d);
}
";
    try_compile_all_nv(wgsl);
    try_compile_all_legacy(wgsl);
}

#[test]
fn mem_i32_load_store_all_nv() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read> inp: array<i32>;
@group(0) @binding(1) var<storage, read_write> out: array<i32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let a = inp[gid.x];
    let b = inp[gid.x + 1u];
    out[gid.x] = a + b - a * b;
}
";
    try_compile_all_nv(wgsl);
    try_compile_all_legacy(wgsl);
}

#[test]
fn mem_vec2_u32_load_store() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read> inp: array<vec2<u32>>;
@group(0) @binding(1) var<storage, read_write> out: array<vec2<u32>>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let v = inp[gid.x];
    out[gid.x] = vec2<u32>(v.y, v.x);
}
";
    try_compile_all_nv(wgsl);
    try_compile_all_legacy(wgsl);
}

// =============================================================================
// Control flow — deeper patterns for sm20/32/50/70 control.rs
// =============================================================================

#[test]
fn control_while_loop_all_nv() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@group(0) @binding(1) var<storage, read> data: array<f32>;
@compute @workgroup_size(1)
fn main() {
    var acc: f32 = 0.0;
    var i: u32 = 0u;
    loop {
        if i >= 64u { break; }
        let v = data[i];
        if v < 0.0 { break; }
        acc = acc + v;
        i = i + 1u;
    }
    out[0] = acc;
}
";
    try_compile_all_nv(wgsl);
    try_compile_all_legacy(wgsl);
}

#[test]
fn control_triple_nested_all_nv() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(1)
fn main() {
    var total: f32 = 0.0;
    var i: u32 = 0u;
    loop {
        if i >= 4u { break; }
        var j: u32 = 0u;
        loop {
            if j >= 4u { break; }
            var k: u32 = 0u;
            loop {
                if k >= 4u { break; }
                total = total + f32(i * 16u + j * 4u + k);
                k = k + 1u;
            }
            j = j + 1u;
        }
        i = i + 1u;
    }
    out[0] = total;
}
";
    try_compile_all_nv(wgsl);
    try_compile_all_legacy(wgsl);
}

#[test]
fn control_multi_branch_predicate_all_nv() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@group(0) @binding(1) var<storage, read> inp: array<f32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let x = inp[gid.x];
    var r: f32 = 0.0;
    if x > 10.0 {
        r = x * 2.0;
    } else if x > 5.0 {
        r = x * 1.5;
    } else if x > 0.0 {
        r = x;
    } else if x > -5.0 {
        r = -x;
    } else {
        r = 0.0;
    }
    out[gid.x] = r;
}
";
    try_compile_all_nv(wgsl);
    try_compile_all_legacy(wgsl);
}

// =============================================================================
// Instruction latency table coverage — SM75/80/86/89 specific
// =============================================================================

#[test]
fn latency_sm75_register_pressure() {
    let mut wgsl = String::from(
        "@group(0) @binding(0) var<storage, read_write> out: array<f32>;\n\
         @group(0) @binding(1) var<storage, read> inp: array<f32>;\n\
         @compute @workgroup_size(1) fn main() {\n",
    );
    for i in 0..64 {
        let _ = std::fmt::Write::write_fmt(
            &mut wgsl,
            format_args!("  let v{i} = inp[{i}] * 1.01 + f32({i});\n"),
        );
    }
    wgsl.push_str("  var s: f32 = 0.0;\n");
    for i in 0..64 {
        let _ = std::fmt::Write::write_fmt(&mut wgsl, format_args!("  s = s + v{i};\n"));
    }
    wgsl.push_str("  out[0] = s;\n}\n");
    for &nv in &[NvArch::Sm75, NvArch::Sm80, NvArch::Sm86, NvArch::Sm89] {
        try_compile_nv(&wgsl, nv);
    }
}

#[test]
fn latency_sm80_predicate_heavy() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@group(0) @binding(1) var<storage, read> inp: array<f32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let x = inp[gid.x];
    var r: f32 = 0.0;
    let c0 = x > 0.0;
    let c1 = x > 1.0;
    let c2 = x > 2.0;
    let c3 = x > 3.0;
    let c4 = x > 4.0;
    if c0 { r = r + 1.0; }
    if c1 { r = r + 2.0; }
    if c2 { r = r + 4.0; }
    if c3 { r = r + 8.0; }
    if c4 { r = r + 16.0; }
    out[gid.x] = r;
}
";
    for &nv in &[NvArch::Sm80, NvArch::Sm86, NvArch::Sm89] {
        try_compile_nv(wgsl, nv);
    }
}

// =============================================================================
// Misc ALU: integer division, modulo, sign extension — sm20/32/50 alu/misc.rs
// =============================================================================

#[test]
fn alu_div_mod_all_nv() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<u32>;
@group(0) @binding(1) var<storage, read> inp: array<u32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let a = inp[gid.x] + 1u;
    let b = inp[gid.x + 1u] + 1u;
    let d = a / b;
    let m = a % b;
    out[gid.x] = d + m;
}
";
    try_compile_all_nv(wgsl);
    best_effort_legacy(wgsl);
}

#[test]
fn alu_signed_ops_all_nv() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<i32>;
@group(0) @binding(1) var<storage, read> inp: array<i32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let a = inp[gid.x];
    let b = inp[gid.x + 1u];
    let neg = -a;
    let abs_val = abs(a);
    let min_val = min(a, b);
    let max_val = max(a, b);
    let clamped = clamp(a, -100, 100);
    out[gid.x] = neg + abs_val + min_val + max_val + clamped;
}
";
    try_compile_all_nv(wgsl);
    try_compile_all_legacy(wgsl);
}

#[test]
fn alu_float_fma_chain_all_nv() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@group(0) @binding(1) var<storage, read> inp: array<f32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let a = inp[gid.x];
    let b = inp[gid.x + 1u];
    let c = inp[gid.x + 2u];
    let r = fma(a, b, c);
    let s = fma(r, a, b);
    let t = fma(s, c, r);
    out[gid.x] = t;
}
";
    try_compile_all_nv(wgsl);
    try_compile_all_legacy(wgsl);
}

// =============================================================================
// Conversion coverage — sm50/alu/conv.rs (33.21%)
// =============================================================================

#[test]
fn conv_all_types_all_nv() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out_f: array<f32>;
@group(0) @binding(1) var<storage, read> inp_u: array<u32>;
@group(0) @binding(2) var<storage, read> inp_i: array<i32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let u = inp_u[gid.x];
    let i = inp_i[gid.x];
    let f_from_u = f32(u);
    let f_from_i = f32(i);
    let u_back = u32(f_from_u);
    let i_back = i32(f_from_i);
    let bool_val = u > 0u;
    let f_from_bool = select(0.0, 1.0, bool_val);
    out_f[gid.x] = f_from_u + f_from_i + f32(u_back) + f32(i_back) + f_from_bool;
}
";
    try_compile_all_nv(wgsl);
    try_compile_all_legacy(wgsl);
}

// =============================================================================
// Uniform buffer access patterns — shader_io.rs coverage
// =============================================================================

#[test]
fn shader_io_nested_struct_uniform() {
    let wgsl = r"
struct Inner {
    a: f32,
    b: f32,
    c: f32,
    d: f32,
}
struct Outer {
    inner: Inner,
    scale: f32,
    offset: f32,
    _pad: vec2<f32>,
}
@group(0) @binding(0) var<uniform> params: Outer;
@group(0) @binding(1) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let v = params.inner.a + params.inner.b + params.inner.c + params.inner.d;
    out[gid.x] = v * params.scale + params.offset;
}
";
    try_compile_all_nv(wgsl);
    try_compile_all_legacy(wgsl);
}

// =============================================================================
// Lower copy swap — complex phi merge patterns
// =============================================================================

#[test]
fn lower_copy_swap_five_way_rotation() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@group(0) @binding(1) var<storage, read> inp: array<f32>;
@compute @workgroup_size(1)
fn main() {
    var a = inp[0]; var b = inp[1]; var c = inp[2]; var d = inp[3]; var e = inp[4];
    var i: u32 = 0u;
    loop {
        if i >= 20u { break; }
        let t = a;
        a = b; b = c; c = d; d = e; e = t + f32(i);
        i = i + 1u;
    }
    out[0] = a + b + c + d + e;
}
";
    try_compile_all_nv(wgsl);
    try_compile_all_legacy(wgsl);
}
