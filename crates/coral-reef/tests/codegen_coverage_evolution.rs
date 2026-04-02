// SPDX-License-Identifier: AGPL-3.0-only
#![cfg(feature = "naga")]
//! Evolution coverage tests — targets low-coverage codegen paths.
//!
//! Each test exercises a specific codegen module:
//! - `opt_instr_sched_prepass`: instruction scheduling pre-pass
//! - `spill_values`: register spill/fill logic
//! - `assign_regs`: register allocation
//! - `legalize`: IR legalization passes
//! - `lower_f64/poly`: f64 polynomial approximations (trig, `exp2`, `log2`)
//! - `repair_ssa`: SSA repair pass (triggered via `spill_values`)
//! - `nv/sm50`, `nv/sm32`: older SM encoding paths

use std::fmt::Write;

use coral_reef::{CompileOptions, GpuArch};

fn opts_sm70_f64() -> CompileOptions {
    CompileOptions {
        target: GpuArch::Sm70.into(),
        opt_level: 2,
        debug_info: false,
        fp64_software: true,
        ..CompileOptions::default()
    }
}

fn compile_sm70(wgsl: &str) -> Result<Vec<u8>, coral_reef::CompileError> {
    coral_reef::compile_wgsl(wgsl, &opts_sm70_f64())
}

fn compile_raw_sm(wgsl: &str, sm: u8) -> Result<Vec<u8>, coral_reef::CompileError> {
    coral_reef::compile_wgsl_raw_sm(wgsl, sm)
}

fn assert_compile_sm70(wgsl: &str) {
    let r = compile_sm70(wgsl);
    assert!(r.is_ok(), "SM70 compile failed: {:?}", r.err());
    assert!(!r.unwrap().is_empty(), "SM70 produced empty binary");
}

fn assert_compile_raw_sm(wgsl: &str, sm: u8) {
    let r = compile_raw_sm(wgsl, sm);
    assert!(r.is_ok(), "SM{sm} compile failed: {:?}", r.err());
    assert!(!r.unwrap().is_empty(), "SM{sm} produced empty binary");
}

// =============================================================================
// 1. opt_instr_sched_prepass — instruction scheduling pre-pass
// =============================================================================

/// Many independent instructions in a block — exercises schedule unit formation.
#[test]
fn evolution_instr_sched_prepass_parallel_chains() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@group(0) @binding(1) var<storage, read> inp: array<f32>;
@compute @workgroup_size(1)
fn main() {
    let a = inp[0];
    let b = inp[1];
    let c = inp[2];
    let d = inp[3];
    let e = a + b;
    let f = c * d;
    let g = a - b;
    let h = c + d;
    let i = e * f;
    let j = g + h;
    let k = i - j;
    let l = k * 2.0;
    out[0] = l;
}
";
    assert_compile_sm70(wgsl);
}

/// Loop with back-edge — exercises live-in from back-edge pred.
#[test]
fn evolution_instr_sched_prepass_loop_back_edge() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@group(0) @binding(1) var<storage, read> inp: array<f32>;
@compute @workgroup_size(1)
fn main() {
    var sum: f32 = 0.0;
    var i: u32 = 0u;
    loop {
        if i >= 32u { break; }
        let v = inp[i];
        sum = sum + v * f32(i);
        i = i + 1u;
    }
    out[0] = sum;
}
";
    assert_compile_sm70(wgsl);
}

/// Nested loops — multiple blocks with live-out.
#[test]
fn evolution_instr_sched_prepass_nested_loops() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(1)
fn main() {
    var acc: f32 = 0.0;
    for (var i: u32 = 0u; i < 8u; i++) {
        for (var j: u32 = 0u; j < 8u; j++) {
            acc = acc + f32(i * 8u + j) * 0.01;
        }
    }
    out[0] = acc;
}
";
    assert_compile_sm70(wgsl);
}

// =============================================================================
// 2. spill_values — register spill/fill logic
// =============================================================================

/// Extreme register pressure — 96 live values to force spilling.
#[test]
fn evolution_spill_extreme_96_live() {
    let mut wgsl = String::from(
        "@group(0) @binding(0) var<storage, read_write> out: array<f32>;\n\
         @group(0) @binding(1) var<storage, read> inp: array<f32>;\n\
         @compute @workgroup_size(1) fn main() {\n",
    );
    for i in 0..96 {
        let _ = writeln!(wgsl, "  let v{i} = inp[{i} % 64] + f32({i});");
    }
    wgsl.push_str("  var sum: f32 = 0.0;\n");
    for i in 0..96 {
        let _ = writeln!(wgsl, "  sum = sum + v{i};");
    }
    wgsl.push_str("  out[0] = sum;\n}\n");
    assert_compile_sm70(&wgsl);
}

/// Spill pressure with loop — values live across loop iterations.
#[test]
fn evolution_spill_loop_many_live() {
    let mut wgsl = String::from(
        "@group(0) @binding(0) var<storage, read_write> out: array<f32>;\n\
         @group(0) @binding(1) var<storage, read> inp: array<f32>;\n\
         @compute @workgroup_size(1) fn main() {\n",
    );
    for i in 0..56 {
        let _ = writeln!(wgsl, "  var v{i} = inp[{i} % 32];");
    }
    wgsl.push_str("  var i: u32 = 0u;\n  loop {\n");
    wgsl.push_str("    if i >= 16u { break; }\n");
    for i in 0..56 {
        let prev = if i == 0 { 55 } else { i - 1 };
        let _ = writeln!(wgsl, "    v{i} = v{i} + v{prev} * 0.1;");
    }
    wgsl.push_str("    i = i + 1u;\n  }\n  var s: f32 = 0.0;\n");
    for i in 0..56 {
        let _ = writeln!(wgsl, "  s = s + v{i};");
    }
    wgsl.push_str("  out[0] = s;\n}\n");
    assert_compile_sm70(&wgsl);
}

/// Branch with many phis — `repair_ssa` + spill interaction.
#[test]
fn evolution_spill_branch_many_phis() {
    let mut wgsl = String::from(
        "@group(0) @binding(0) var<storage, read_write> out: array<f32>;\n\
         @group(0) @binding(1) var<storage, read> inp: array<f32>;\n\
         @compute @workgroup_size(1) fn main() {\n",
    );
    for i in 0..40 {
        let _ = writeln!(wgsl, "  let a{i} = inp[{i}];");
    }
    wgsl.push_str("  var r0: f32; var r1: f32; var r2: f32; var r3: f32;\n");
    wgsl.push_str("  if a0 > a1 {\n");
    for i in 0..4 {
        let j = i + 1;
        let _ = writeln!(wgsl, "    r{i} = a{i} + a{j};");
    }
    wgsl.push_str("  } else {\n");
    for i in 0..4 {
        let j = i + 1;
        let _ = writeln!(wgsl, "    r{i} = a{i} - a{j};");
    }
    wgsl.push_str("  }\n  var s: f32 = 0.0;\n");
    for i in 0..40 {
        let _ = writeln!(wgsl, "  s = s + a{i};");
    }
    wgsl.push_str("  out[0] = r0 + r1 + r2 + r3 + s;\n}\n");
    assert_compile_sm70(&wgsl);
}

// =============================================================================
// 3. assign_regs — register allocation
// =============================================================================

/// Complex control flow — multiple branches and merges.
#[test]
fn evolution_assign_regs_complex_cfg() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@group(0) @binding(1) var<storage, read> inp: array<f32>;
@compute @workgroup_size(1)
fn main() {
    let a = inp[0];
    let b = inp[1];
    var r: f32;
    if a > 0.0 {
        if b > 0.0 {
            r = a + b;
        } else {
            r = a - b;
        }
    } else {
        if b > 0.0 {
            r = b - a;
        } else {
            r = -a - b;
        }
    }
    out[0] = r;
}
";
    assert_compile_sm70(wgsl);
}

/// Loop with phi webs — `assign_regs` block coverage.
#[test]
fn evolution_assign_regs_phi_webs() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@group(0) @binding(1) var<storage, read> inp: array<f32>;
@compute @workgroup_size(1)
fn main() {
    var x: f32 = inp[0];
    var y: f32 = inp[1];
    var i: u32 = 0u;
    loop {
        if i >= 20u { break; }
        let t = x;
        x = y;
        y = t + f32(i);
        i = i + 1u;
    }
    out[0] = x + y;
}
";
    assert_compile_sm70(wgsl);
}

/// Many sequential `if`/`else` regions with live values feeding a final sum — stresses `assign_regs/block`.
#[test]
fn evolution_assign_regs_chained_conditional_blocks() {
    let mut wgsl = String::from(
        "@group(0) @binding(0) var<storage, read_write> out: array<f32>;\n\
         @group(0) @binding(1) var<storage, read> inp: array<f32>;\n\
         @compute @workgroup_size(1) fn main() {\n\
           var acc: f32 = 0.0;\n",
    );
    for i in 0..16 {
        let _ = writeln!(
            wgsl,
            "  if inp[{i}] > 0.0 {{ acc = acc + inp[{i}]; }} else {{ acc = acc - inp[{i}] * 0.5; }}"
        );
    }
    wgsl.push_str("  out[0] = acc;\n}\n");
    assert_compile_sm70(&wgsl);
}

// =============================================================================
// 4. legalize — IR legalization passes
// =============================================================================

/// vec4 operations — legalize vector handling.
#[test]
fn evolution_legalize_vec4_ops() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@group(0) @binding(1) var<storage, read> inp: array<vec4<f32>>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let v = inp[gid.x];
    let w = v.xyzw + v.wzyx;
    out[gid.x] = dot(v, w);
}
";
    assert_compile_sm70(wgsl);
}

/// Predicate-heavy — legalize predicate handling.
#[test]
fn evolution_legalize_predicates() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@group(0) @binding(1) var<storage, read> inp: array<f32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let x = inp[gid.x];
    let y = inp[gid.x + 1u];
    let c1 = x > 0.0;
    let c2 = y < 1.0;
    let c3 = c1 && c2;
    let c4 = c1 || !c2;
    let r = select(0.0, x, c3);
    let s = select(y, r, c4);
    out[gid.x] = s;
}
";
    assert_compile_sm70(wgsl);
}

/// Matrix multiply — legalize matrix ops.
#[test]
fn evolution_legalize_matrix() {
    let wgsl = r"
struct Params {
    m: mat3x3<f32>,
    v: vec3<f32>,
}
@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read_write> out: array<vec3<f32>>;
@compute @workgroup_size(1)
fn main() {
    let r = params.m * params.v;
    out[0] = r;
}
";
    assert_compile_sm70(wgsl);
}

// =============================================================================
// 5. lower_f64/poly — f64 polynomial approximations (trig, exp2, log2)
// =============================================================================

/// f64 sin/cos — `lower_f64` trig poly.
#[test]
fn evolution_lower_f64_sin_cos() {
    let wgsl = r"
enable f64;
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(1)
fn main() {
    let x: f64 = 1.5;
    let s = sin(x);
    let c = cos(x);
    out[0] = f32(s + c);
}
";
    assert_compile_sm70(wgsl);
}

/// f64 `exp2` — `lower_f64` exp2 poly.
#[test]
fn evolution_lower_f64_exp2() {
    let wgsl = r"
enable f64;
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(1)
fn main() {
    let x: f64 = 2.3;
    let e = exp2(x);
    out[0] = f32(e);
}
";
    assert_compile_sm70(wgsl);
}

/// f64 `log2` — `lower_f64` log2 poly.
#[test]
fn evolution_lower_f64_log2() {
    let wgsl = r"
enable f64;
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(1)
fn main() {
    let x: f64 = 3.7;
    let l = log2(x);
    out[0] = f32(l);
}
";
    assert_compile_sm70(wgsl);
}

/// f64 all transcendentals — exercises full poly suite.
#[test]
fn evolution_lower_f64_all_poly() {
    let wgsl = r"
enable f64;
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(1)
fn main() {
    let x: f64 = 1.2;
    let s = sin(x);
    let c = cos(x);
    let e = exp2(x);
    let l = log2(x);
    out[0] = f32(s + c + e + l);
}
";
    assert_compile_sm70(wgsl);
}

// =============================================================================
// 6. repair_ssa — SSA repair pass (triggered by spill_values)
// =============================================================================

/// Phi nodes with dominance violation pattern — `repair_ssa`.
#[test]
fn evolution_repair_ssa_phi_dominance() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@group(0) @binding(1) var<storage, read> inp: array<f32>;
@compute @workgroup_size(1)
fn main() {
    var a = inp[0];
    var b = inp[1];
    var i: u32 = 0u;
    loop {
        if i >= 12u { break; }
        if a > b {
            let t = a;
            a = b;
            b = t;
        }
        i = i + 1u;
    }
    out[0] = a + b;
}
";
    assert_compile_sm70(wgsl);
}

/// Multi-block phi — `repair_ssa` multi-def.
#[test]
fn evolution_repair_ssa_multi_def() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@group(0) @binding(1) var<storage, read> inp: array<f32>;
@compute @workgroup_size(1)
fn main() {
    let a = inp[0];
    let b = inp[1];
    var r: f32;
    if a > 0.0 {
        r = a * 2.0;
    } else {
        r = b * 3.0;
    }
    if b > 0.0 {
        r = r + 1.0;
    } else {
        r = r - 1.0;
    }
    out[0] = r;
}
";
    assert_compile_sm70(wgsl);
}

// =============================================================================
// 7. nv/sm50 — Maxwell encoding paths
// =============================================================================

/// SM50: f32 transcendentals (sin, cos, exp2).
#[test]
fn evolution_sm50_trig_exp2() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(1)
fn main() {
    let x: f32 = 0.7;
    let s = sin(x);
    let c = cos(x);
    let e = exp2(x);
    out[0] = s + c + e;
}
";
    assert_compile_raw_sm(wgsl, 50);
}

/// SM50: integer shift and arithmetic.
#[test]
fn evolution_sm50_int_shift_arith() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<u32>;
@compute @workgroup_size(1)
fn main() {
    var a: u32 = 0x1234u;
    let b = a << 3u;
    let c = a >> 2u;
    let d = (a << 1u) | (a >> 31u);
    out[0] = b + c + d;
}
";
    assert_compile_raw_sm(wgsl, 50);
}

/// SM50: select and control flow.
#[test]
fn evolution_sm50_select_control() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@group(0) @binding(1) var<storage, read> inp: array<f32>;
@compute @workgroup_size(1)
fn main() {
    let a = inp[0];
    let b = inp[1];
    let c = a > b;
    out[0] = select(b, a, c);
}
";
    assert_compile_raw_sm(wgsl, 50);
}

// =============================================================================
// 8. nv/sm32 — Kepler encoding paths
// =============================================================================

/// SM32: f32 sin/cos.
#[test]
fn evolution_sm32_trig() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(1)
fn main() {
    let x: f32 = 0.5;
    let s = sin(x);
    let c = cos(x);
    out[0] = s + c;
}
";
    assert_compile_raw_sm(wgsl, 32);
}

/// SM32: integer and float mix.
#[test]
fn evolution_sm32_int_float_mix() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out_f: array<f32>;
@group(0) @binding(1) var<storage, read_write> out_u: array<u32>;
@compute @workgroup_size(1)
fn main() {
    let x: f32 = 0.5;
    out_f[0] = sin(x) + cos(x);
    var a: u32 = 16u;
    let b = a << 2u;
    let c = a >> 1u;
    out_u[0] = b + c;
}
";
    assert_compile_raw_sm(wgsl, 32);
}

/// SM32: loop with barrier-like pattern.
#[test]
fn evolution_sm32_loop_control() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(1)
fn main() {
    var sum: f32 = 0.0;
    for (var i: u32 = 0u; i < 16u; i++) {
        sum = sum + f32(i);
    }
    out[0] = sum;
}
";
    assert_compile_raw_sm(wgsl, 32);
}
