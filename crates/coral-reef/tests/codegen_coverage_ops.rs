// SPDX-License-Identifier: AGPL-3.0-or-later
//! Targeted coverage tests for Maxwell paths, control flow, spiller pressure,
//! and select/predicate op patterns.

use std::fmt::Write;

use coral_reef::{CompileOptions, GpuArch};

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

// =============================================================================
// Maxwell paths (SM50/32/20): shl, shr, iadd, imul, ineg, trig, sel
// =============================================================================

#[test]
fn coverage_maxwell_shl_shr_sm50() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<u32>;
@compute @workgroup_size(1)
fn main() {
    var a: u32 = 0x1234u;
    var b: u32 = 5u;
    let c = a << 2u;
    let d = a >> 3u;
    let e = (a << b) | (a >> (32u - b));
    out[0] = c + d + e;
}
";
    compile_raw_sm(wgsl, 50);
}

#[test]
fn coverage_maxwell_iadd_imul_ineg_sm50() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<u32>;
@compute @workgroup_size(1)
fn main() {
    var a: u32 = 10u;
    var b: u32 = 7u;
    let sum = a + b;
    let prod = a * b;
    let neg_a = -i32(a);
    out[0] = sum + prod + u32(neg_a);
}
";
    compile_raw_sm(wgsl, 50);
}

#[test]
fn coverage_maxwell_fsin_fcos_fexp2_sm50() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(1)
fn main() {
    let x: f32 = 1.5;
    let s = sin(x);
    let c = cos(x);
    let e = exp2(x);
    out[0] = s + c + e;
}
";
    compile_raw_sm(wgsl, 50);
}

#[test]
fn coverage_maxwell_sel_float_sm50() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@group(0) @binding(1) var<storage, read> inp: array<f32>;
@compute @workgroup_size(1)
fn main() {
    let a = inp[0];
    let b = inp[1];
    let cond = a > b;
    out[0] = select(b, a, cond);
}
";
    compile_raw_sm(wgsl, 50);
}

// =============================================================================
// Control flow: break/sync (OpBreak, OpBSSy, OpBSync)
// =============================================================================

#[test]
fn coverage_control_break_early_from_loop() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@group(0) @binding(1) var<storage, read> inp: array<f32>;
@compute @workgroup_size(1)
fn main() {
    var sum: f32 = 0.0;
    var i: u32 = 0u;
    loop {
        if i >= 20u { break; }
        if inp[i] < 0.0 { break; }
        sum = sum + inp[i];
        i = i + 1u;
    }
    out[0] = sum;
}
";
    compile_fixture_sm70(wgsl);
}

#[test]
fn coverage_control_continue_skip_even() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(1)
fn main() {
    var sum: f32 = 0.0;
    var i: u32 = 0u;
    loop {
        if i >= 16u { break; }
        i = i + 1u;
        if (i & 1u) == 0u { continue; }
        sum = sum + f32(i);
    }
    out[0] = sum;
}
";
    compile_fixture_sm70(wgsl);
}

#[test]
fn coverage_control_nested_loop_break_inner() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(1)
fn main() {
    var total: f32 = 0.0;
    var i: u32 = 0u;
    loop {
        if i >= 8u { break; }
        var j: u32 = 0u;
        loop {
            if j >= 8u { break; }
            if i == 4u && j == 4u { break; }
            total = total + f32(i) + f32(j) * 0.1;
            j = j + 1u;
        }
        i = i + 1u;
    }
    out[0] = total;
}
";
    compile_fixture_sm70(wgsl);
}

#[test]
fn coverage_control_break_after_barrier() {
    let wgsl = r"
var<workgroup> wg: array<f32, 64>;
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(64)
fn main(@builtin(local_invocation_id) lid: vec3<u32>) {
    wg[lid.x] = f32(lid.x);
    workgroupBarrier();
    var i: u32 = 0u;
    loop {
        if i >= 4u { break; }
        out[lid.x] = wg[(lid.x + i) % 64u];
        i = i + 1u;
    }
}
";
    compile_fixture_sm70(wgsl);
}

// =============================================================================
// Complex register pressure for spiller (loop header, phi, edge)
// =============================================================================

#[test]
fn coverage_spill_loop_header_many_live() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@group(0) @binding(1) var<storage, read> inp: array<f32>;
@compute @workgroup_size(1)
fn main() {
    var v0 = inp[0];
    var v1 = inp[1];
    var v2 = inp[2];
    var v3 = inp[3];
    var v4 = inp[4];
    var v5 = inp[5];
    var v6 = inp[6];
    var v7 = inp[7];
    var i: u32 = 0u;
    loop {
        if i >= 32u { break; }
        v0 = v0 + v1;
        v1 = v1 + v2;
        v2 = v2 + v3;
        v3 = v3 + v4;
        v4 = v4 + v5;
        v5 = v5 + v6;
        v6 = v6 + v7;
        v7 = v7 + v0 * 0.1;
        i = i + 1u;
    }
    out[0] = v0 + v1 + v2 + v3 + v4 + v5 + v6 + v7;
}
";
    compile_fixture_sm70(wgsl);
}

#[test]
fn coverage_spill_branches_many_phis() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@group(0) @binding(1) var<storage, read> inp: array<f32>;
@compute @workgroup_size(1)
fn main() {
    let a = inp[0];
    let b = inp[1];
    let c = inp[2];
    let d = inp[3];
    var r0: f32;
    var r1: f32;
    var r2: f32;
    var r3: f32;
    if a > b {
        r0 = a + b;
        r1 = a - b;
        r2 = a * b;
        r3 = a;
    } else {
        r0 = b - a;
        r1 = b + a;
        r2 = b;
        r3 = b * a;
    }
    if c > d {
        r0 = r0 + c;
        r1 = r1 - d;
    } else {
        r0 = r0 - c;
        r1 = r1 + d;
    }
    out[0] = r0 + r1 + r2 + r3;
}
";
    compile_fixture_sm70(wgsl);
}

#[test]
fn coverage_spill_edge_fill_loop_exit() {
    let mut wgsl = String::from(
        "@group(0) @binding(0) var<storage, read_write> out: array<f32>;\n\
         @group(0) @binding(1) var<storage, read> inp: array<f32>;\n\
         @compute @workgroup_size(1) fn main() {\n",
    );
    for i in 0..48 {
        let _ = writeln!(wgsl, "  var v{i} = inp[{i} % 64];");
    }
    wgsl.push_str("  var i: u32 = 0u;\n  loop {\n");
    wgsl.push_str("    if i >= 8u { break; }\n");
    for i in 0..48 {
        let _ = writeln!(wgsl, "    v{i} = v{i} * 1.01 + f32(i);");
    }
    wgsl.push_str("    i = i + 1u;\n  }\n  var s: f32 = 0.0;\n");
    for i in 0..48 {
        let _ = writeln!(wgsl, "  s = s + v{i};");
    }
    wgsl.push_str("  out[0] = s;\n}\n");
    compile_fixture_sm70(&wgsl);
}

// =============================================================================
// Select/predicate patterns for alu/misc.rs
// =============================================================================

#[test]
fn coverage_select_float_lt_gt_eq() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@group(0) @binding(1) var<storage, read> inp: array<f32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let x = inp[gid.x];
    let y = inp[gid.x + 1u];
    let r_lt = select(1.0, 0.0, x < y);
    let r_gt = select(0.0, 1.0, x > y);
    let r_eq = select(0.0, 1.0, x == y);
    out[gid.x] = r_lt + r_gt * 0.5 + r_eq * 0.25;
}
";
    compile_fixture_sm70(wgsl);
}

#[test]
fn coverage_select_u32_comparisons() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<u32>;
@group(0) @binding(1) var<storage, read> inp: array<u32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let a = inp[gid.x];
    let b = inp[gid.x + 1u];
    let r1 = select(0u, 1u, a < b);
    let r2 = select(0u, 1u, a >= b);
    let r3 = select(0u, 1u, a != b);
    out[gid.x] = r1 + r2 * 2u + r3 * 4u;
}
";
    compile_fixture_sm70(wgsl);
}

#[test]
fn coverage_select_i32_signed_cmp() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<i32>;
@group(0) @binding(1) var<storage, read> inp: array<i32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let a = inp[gid.x];
    let b = inp[gid.x + 1u];
    let r = select(b, a, a > b);
    out[gid.x] = r;
}
";
    compile_fixture_sm70(wgsl);
}

#[test]
fn coverage_select_vec2_float() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<vec2<f32>>;
@group(0) @binding(1) var<storage, read> inp: array<vec2<f32>>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let a = inp[gid.x];
    let b = inp[gid.x + 1u];
    let c = a.x > b.x;
    let r = select(b, a, c);
    out[gid.x] = r;
}
";
    compile_fixture_sm70(wgsl);
}

#[test]
fn coverage_select_vec4_cond() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@group(0) @binding(1) var<storage, read> inp: array<vec4<f32>>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let v = inp[gid.x];
    let c = v.x > 0.5;
    let r = select(v.y, v.x, c);
    out[gid.x] = r + v.z + v.w;
}
";
    compile_fixture_sm70(wgsl);
}

// =============================================================================
// Additional Maxwell/legacy SM coverage
// =============================================================================

#[test]
fn coverage_maxwell_bitwise_sm50() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<u32>;
@compute @workgroup_size(1)
fn main() {
    var a: u32 = 0xFF00u;
    var b: u32 = 0x0F0Fu;
    let and_val = a & b;
    let or_val = a | b;
    let xor_val = a ^ b;
    out[0] = and_val + or_val + xor_val;
}
";
    compile_raw_sm(wgsl, 50);
}

#[test]
fn coverage_legacy_sm32_maxwell_ops() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out_f: array<f32>;
@group(0) @binding(1) var<storage, read_write> out_u: array<u32>;
@compute @workgroup_size(1)
fn main() {
    var x: f32 = 0.5;
    x = sin(x) + cos(x) + exp2(x);
    var a: u32 = 8u;
    let b = a << 1u;
    let c = a >> 2u;
    out_f[0] = x;
    out_u[0] = b + c;
}
";
    compile_raw_sm(wgsl, 32);
}

#[test]
fn coverage_legacy_sm20_fermi_ops() {
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
    compile_raw_sm(wgsl, 20);
}
