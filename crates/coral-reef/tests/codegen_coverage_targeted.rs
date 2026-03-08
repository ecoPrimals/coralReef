// SPDX-License-Identifier: AGPL-3.0-only
//! Targeted coverage tests for specific codegen paths.
//!
//! Each test targets a gap identified via `cargo llvm-cov`:
//! spiller, `lower_copy_swap`, copy propagation, control flow encoding,
//! ALU select/predicate, memory patterns, builder/emit, and Maxwell paths.

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

fn compile_fixture_legacy_nv(wgsl: &str) {
    for sm in [50, 32, 30, 21, 20] {
        let r = coral_reef::compile_wgsl_raw_sm(wgsl, sm);
        assert!(r.is_ok(), "SM{sm}: {}", r.unwrap_err());
    }
}

// --- Coverage gap: spill_values/spiller.rs (128+ live values) ---

#[test]
fn coverage_spill_extreme_128_live_values() {
    let mut wgsl = String::from(
        "@group(0) @binding(0) var<storage, read_write> out: array<f32>;\n\
         @group(0) @binding(1) var<storage, read> inp: array<f32>;\n\
         @compute @workgroup_size(1) fn main() {\n",
    );
    for i in 0..128 {
        let _ = writeln!(wgsl, "  let v{i} = inp[{i}] + f32({i});");
    }
    wgsl.push_str("  var sum: f32 = 0.0;\n");
    for i in 0..128 {
        let _ = writeln!(wgsl, "  sum = sum + v{i};");
    }
    wgsl.push_str("  out[0] = sum;\n}\n");
    compile_fixture_sm70(&wgsl);
}

// --- Coverage gap: lower_copy_swap.rs (phi nodes with many live values) ---

#[test]
fn coverage_lower_copy_swap_phi_many_live() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@group(0) @binding(1) var<storage, read> inp: array<f32>;
@compute @workgroup_size(1)
fn main() {
    var a = inp[0];
    var b = inp[1];
    var c = inp[2];
    var d = inp[3];
    var i: u32 = 0u;
    loop {
        if i >= 16u { break; }
        let t0 = a;
        a = b;
        b = c;
        c = d;
        d = t0 + f32(i);
        i = i + 1u;
    }
    var j: u32 = 0u;
    loop {
        if j >= 8u { break; }
        if a > b {
            let t = a;
            a = b;
            b = t;
        }
        if c > d {
            let t = c;
            c = d;
            d = t;
        }
        j = j + 1u;
    }
    out[0] = a + b + c + d;
}
";
    compile_fixture_sm70(wgsl);
}

// --- Coverage gap: opt_copy_prop (copy propagation patterns) ---

#[test]
fn coverage_opt_copy_prop_intermediate_vars() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@group(0) @binding(1) var<storage, read> inp: array<f32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let x = inp[gid.x];
    let y = inp[gid.x + 1u];
    let a = x + y;
    let b = a * 2.0;
    let c = b - x;
    let d = c + a;
    let e = d * 0.5;
    out[gid.x] = e;
}
";
    compile_fixture_sm70(wgsl);
}

#[test]
fn coverage_opt_copy_prop_select_chain() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@group(0) @binding(1) var<storage, read> inp: array<f32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let x = inp[gid.x];
    let y = inp[gid.x + 1u];
    let c = x > y;
    let r = select(y, x, c);
    out[gid.x] = r;
}
";
    compile_fixture_sm70(wgsl);
}

// --- Coverage gap: sm70_encode/control.rs (switch-like, deep nesting) ---

#[test]
fn coverage_sm70_control_switch_like() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@group(0) @binding(1) var<storage, read> mode: array<u32>;
@compute @workgroup_size(1)
fn main() {
    let m = mode[0];
    var r: f32 = 0.0;
    if m == 0u {
        r = 1.0;
    } else if m == 1u {
        r = 2.0;
    } else if m == 2u {
        r = 3.0;
    } else if m == 3u {
        r = 4.0;
    } else if m == 4u {
        r = 5.0;
    } else if m == 5u {
        r = 6.0;
    } else {
        r = 7.0;
    }
    out[0] = r;
}
";
    compile_fixture_sm70(wgsl);
}

#[test]
fn coverage_sm70_control_deep_nested() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@group(0) @binding(1) var<storage, read> inp: array<f32>;
@compute @workgroup_size(1)
fn main() {
    let a = inp[0];
    let b = inp[1];
    var r: f32 = 0.0;
    if a > 0.0 {
        if b > 0.0 {
            if a > b {
                r = 1.0;
            } else {
                r = 2.0;
            }
        } else {
            if a > -b {
                r = 3.0;
            } else {
                r = 4.0;
            }
        }
    } else {
        if b > 0.0 {
            r = 5.0;
        } else {
            r = 6.0;
        }
    }
    out[0] = r;
}
";
    compile_fixture_sm70(wgsl);
}

// --- Coverage gap: sm70_encode/alu/misc.rs (select, predicate ops) ---

#[test]
fn coverage_sm70_alu_misc_select() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@group(0) @binding(1) var<storage, read> inp: array<f32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let x = inp[gid.x];
    let y = inp[gid.x + 1u];
    let c = x > 0.0;
    let r = select(0.0, x, c);
    let s = select(y, r, y > r);
    out[gid.x] = s;
}
";
    compile_fixture_sm70(wgsl);
}

#[test]
fn coverage_sm70_alu_misc_select_int() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<u32>;
@group(0) @binding(1) var<storage, read> inp: array<u32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let a = inp[gid.x];
    let b = inp[gid.x + 1u];
    let c = a > b;
    let r = select(b, a, c);
    out[gid.x] = r;
}
";
    compile_fixture_sm70(wgsl);
}

// --- Coverage gap: sm70_encode/mem.rs (diverse load/store patterns) ---

#[test]
fn coverage_sm70_mem_diverse_loads() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read> a: array<f32>;
@group(0) @binding(1) var<storage, read> b: array<u32>;
@group(0) @binding(2) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let fa = a[gid.x];
    let fb = a[gid.x + 1u];
    let ua = b[gid.x];
    let ub = b[gid.x + 1u];
    out[gid.x] = fa + fb + f32(ua) + f32(ub);
}
";
    compile_fixture_sm70(wgsl);
}

#[test]
fn coverage_sm70_mem_multiple_bindings_store() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read> a: array<f32>;
@group(0) @binding(1) var<storage, read> b: array<f32>;
@group(0) @binding(2) var<storage, read_write> c: array<f32>;
@group(0) @binding(3) var<storage, read_write> d: array<f32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let x = a[gid.x] + b[gid.x];
    c[gid.x] = x;
    d[gid.x] = x * 2.0;
}
";
    compile_fixture_sm70(wgsl);
}

#[test]
fn coverage_sm70_mem_atomic_add_exchange() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> counter: atomic<u32>;
@group(0) @binding(1) var<storage, read_write> out: array<u32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let old = atomicAdd(&counter, 1u);
    out[gid.x] = old;
}
";
    compile_fixture_sm70(wgsl);
}

// --- Coverage gap: builder/emit.rs (diverse instruction types) ---

#[test]
fn coverage_builder_emit_bit_shifts() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<u32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let x = gid.x;
    let a = x << 2u;
    let b = x >> 1u;
    let c = (x << 3u) | (x >> 29u);
    out[gid.x] = a + b + c;
}
";
    compile_fixture_sm70(wgsl);
}

#[test]
fn coverage_builder_emit_transcendentals() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let x = f32(gid.x) * 0.01 + 0.1;
    let a = sin(x);
    let b = cos(x);
    let c = exp2(x);
    let d = log2(x + 1.0);
    let e = sqrt(x + 0.01);
    let f = pow(x + 1.0, 2.0);
    out[gid.x] = a + b + c + d + e + f;
}
";
    compile_fixture_sm70(wgsl);
}

#[test]
fn coverage_builder_emit_vectors_swizzle() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let x = f32(gid.x);
    let v = vec4<f32>(x, x * 2.0, x * 3.0, 1.0);
    let w = v.yzxw + v.zwxy;
    out[gid.x] = w.x + w.y + w.z + w.w;
}
";
    compile_fixture_sm70(wgsl);
}

#[test]
fn coverage_builder_emit_min_max_abs() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let x = f32(gid.x) * 0.1 - 3.0;
    let a = min(x, 5.0);
    let b = max(x, -5.0);
    let c = abs(x);
    out[gid.x] = a + b + c;
}
";
    compile_fixture_sm70(wgsl);
}

#[test]
fn coverage_builder_emit_floor_ceil_round() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let x = f32(gid.x) * 0.3;
    let a = floor(x);
    let b = ceil(x);
    let c = round(x);
    out[gid.x] = a + b + c;
}
";
    compile_fixture_sm70(wgsl);
}

#[test]
fn coverage_builder_emit_rcp_rsq() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let x = f32(gid.x) + 1.0;
    let r = 1.0 / x;
    let s = 1.0 / sqrt(x);
    out[gid.x] = r + s;
}
";
    compile_fixture_sm70(wgsl);
}

// --- Legacy SM: copy prop + control ---

#[test]
fn legacy_sm50_copy_prop_control() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(1)
fn main() {
    var x: f32 = 1.0;
    var y: f32 = 2.0;
    var i: u32 = 0u;
    loop {
        if i >= 8u { break; }
        let t = x;
        x = y;
        y = t + 1.0;
        i = i + 1u;
    }
    out[0] = x + y;
}
";
    compile_fixture_legacy_nv(wgsl);
}

#[test]
fn legacy_sm50_select_float() {
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
    compile_fixture_legacy_nv(wgsl);
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
