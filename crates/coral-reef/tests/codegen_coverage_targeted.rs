// SPDX-License-Identifier: AGPL-3.0-or-later
//! Targeted coverage tests for specific codegen paths.
//!
//! Each test targets a gap identified via `cargo llvm-cov`:
//! spiller, `lower_copy_swap`, copy propagation, control flow encoding,
//! ALU select/predicate, memory patterns, builder/emit, and legacy SM paths.

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
