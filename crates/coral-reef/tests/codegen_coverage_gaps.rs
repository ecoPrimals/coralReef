// SPDX-License-Identifier: AGPL-3.0-only
#![cfg(feature = "naga")]
//! Targeted tests for llvm-cov gaps: `emit`, `op_conv`, `calc_instr_deps`,
//! debug metadata, cross-target fixtures, and legacy SM paths.

use std::fmt::Write;

use coral_reef::{AmdArch, CompileOptions, GpuArch, GpuTarget};

fn opts_sm70_debug() -> CompileOptions {
    CompileOptions {
        target: GpuArch::Sm70.into(),
        opt_level: 2,
        debug_info: true,
        fp64_software: true,
        ..CompileOptions::default()
    }
}

fn opts_rdna2_debug() -> CompileOptions {
    CompileOptions {
        target: GpuTarget::Amd(AmdArch::Rdna2),
        opt_level: 2,
        debug_info: true,
        fp64_software: true,
        ..CompileOptions::default()
    }
}

fn opts_sm70_default() -> CompileOptions {
    CompileOptions {
        target: GpuArch::Sm70.into(),
        opt_level: 2,
        debug_info: false,
        fp64_software: true,
        ..CompileOptions::default()
    }
}

fn compile_sm70(wgsl: &str, opts: &CompileOptions) {
    let r = coral_reef::compile_wgsl(wgsl, opts);
    assert!(r.is_ok(), "SM70: {}", r.unwrap_err());
}

fn compile_rdna2(wgsl: &str, opts: &CompileOptions) {
    let r = coral_reef::compile_wgsl(wgsl, opts);
    assert!(r.is_ok(), "RDNA2: {}", r.unwrap_err());
}

// ---------------------------------------------------------------------------
// Debug info — codegen/debug.rs and related metadata paths
// ---------------------------------------------------------------------------

#[test]
fn gap_debug_info_minimal_sm70() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(1) fn main() { out[0] = 1.0; }
";
    compile_sm70(wgsl, &opts_sm70_debug());
}

#[test]
fn gap_debug_info_minimal_rdna2() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(1) fn main() { out[0] = 1.0; }
";
    compile_rdna2(wgsl, &opts_rdna2_debug());
}

// ---------------------------------------------------------------------------
// Cross-target fixtures — op_conv, builder/emit helpers, signed int (AMD)
// ---------------------------------------------------------------------------

#[test]
fn gap_fixture_op_conv_amd_rdna2() {
    let wgsl = include_str!("fixtures/wgsl/op_conv_conversions.wgsl");
    let opts = CompileOptions {
        target: GpuTarget::Amd(AmdArch::Rdna2),
        opt_level: 2,
        debug_info: false,
        fp64_software: true,
        ..CompileOptions::default()
    };
    compile_sm70(wgsl, &opts);
}

#[test]
fn gap_fixture_builder_emit_complex_amd_rdna3() {
    let wgsl = include_str!("fixtures/wgsl/builder_emit_complex.wgsl");
    let opts = CompileOptions {
        target: GpuTarget::Amd(AmdArch::Rdna3),
        opt_level: 2,
        debug_info: false,
        fp64_software: true,
        ..CompileOptions::default()
    };
    compile_sm70(wgsl, &opts);
}

#[test]
fn gap_legacy_sm30_i32_only_math() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<i32>;
@compute @workgroup_size(1) fn main() {
    let a: i32 = -7;
    let b: i32 = 11;
    out[0] = a * b + (a >> 1);
}
";
    let r = coral_reef::compile_wgsl_raw_sm(wgsl, 30);
    assert!(r.is_ok(), "SM30: {}", r.unwrap_err());
}

// ---------------------------------------------------------------------------
// CFG: large switch — calc_instr_deps / assign_regs block pressure
// ---------------------------------------------------------------------------

#[test]
fn gap_switch_dense_cases_sm70() {
    let mut wgsl = String::from(
        "@group(0) @binding(0) var<storage, read_write> out: array<f32>;\n\
         @compute @workgroup_size(1) fn main() {\n  let sel: u32 = 7u;\n  var acc: f32 = 0.0;\n  switch sel {\n",
    );
    for i in 0..32_u32 {
        let _ = writeln!(wgsl, "    case {i}u: {{ acc = acc + f32({i}); }}");
    }
    wgsl.push_str("    default: { acc = 99.0; }\n  }\n  out[0] = acc;\n}\n");
    compile_sm70(&wgsl, &opts_sm70_default());
}

// ---------------------------------------------------------------------------
// f64 / vector select — op_float / op_conv / emit paths on AMD and SM70
// ---------------------------------------------------------------------------

#[test]
fn gap_select_and_step_amd_rdna4() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(1) fn main() {
    let a = vec3<f32>(1.0, 2.0, 3.0);
    let b = vec3<f32>(4.0, 5.0, 6.0);
    let c = select(a, b, vec3<bool>(true, false, true));
    out[0] = step(2.0, c.x) + c.y + c.z;
}
";
    let opts = CompileOptions {
        target: GpuTarget::Amd(AmdArch::Rdna4),
        opt_level: 2,
        debug_info: false,
        fp64_software: true,
        ..CompileOptions::default()
    };
    compile_sm70(wgsl, &opts);
}

#[test]
fn gap_f64_dot_product_sm70() {
    let wgsl = r"
enable f64;
@group(0) @binding(0) var<storage, read_write> out: array<f64>;
@compute @workgroup_size(1) fn main() {
    let a = vec3<f64>(1.0, 2.0, 3.0);
    let b = vec3<f64>(4.0, 5.0, 6.0);
    out[0] = dot(a, b);
}
";
    let opts = CompileOptions {
        target: GpuArch::Sm70.into(),
        opt_level: 2,
        debug_info: false,
        fp64_software: true,
        ..CompileOptions::default()
    };
    compile_sm70(wgsl, &opts);
}

// ---------------------------------------------------------------------------
// Legacy SM: funnel shift + firstLeadingBit (emit / encode paths)
// ---------------------------------------------------------------------------

#[test]
fn gap_legacy_sm50_rotate_and_abs() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@group(0) @binding(1) var<storage, read> inp: array<u32>;
@compute @workgroup_size(1) fn main() {
    let x = inp[0];
    let r = (x << 16u) | (x >> 16u);
    let a = firstLeadingBit(r);
    out[0] = f32(a);
}
";
    let r = coral_reef::compile_wgsl_raw_sm(wgsl, 50);
    assert!(r.is_ok(), "{}", r.unwrap_err());
}

// ---------------------------------------------------------------------------
// GCN5 (Vega / MI50 / GFX906) — validate full compile pipeline
// ---------------------------------------------------------------------------

fn opts_gcn5() -> CompileOptions {
    CompileOptions {
        target: GpuTarget::Amd(AmdArch::Gcn5),
        opt_level: 2,
        debug_info: false,
        fp64_software: false,
        ..CompileOptions::default()
    }
}

#[test]
fn gap_gcn5_minimal_store() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(64) fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    out[gid.x] = 42.0;
}
";
    let r = coral_reef::compile_wgsl(wgsl, &opts_gcn5());
    assert!(r.is_ok(), "GCN5 minimal store: {}", r.unwrap_err());
}

#[test]
fn gap_gcn5_f64_fma() {
    let wgsl = r"
enable f64;
@group(0) @binding(0) var<storage, read_write> out: array<f64>;
@group(0) @binding(1) var<storage, read> a: array<f64>;
@group(0) @binding(2) var<storage, read> b: array<f64>;
@compute @workgroup_size(64) fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    out[i] = fma(a[i], b[i], out[i]);
}
";
    let r = coral_reef::compile_wgsl(wgsl, &opts_gcn5());
    assert!(r.is_ok(), "GCN5 f64 fma: {}", r.unwrap_err());
}

#[test]
fn gap_gcn5_integer_alu() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<u32>;
@group(0) @binding(1) var<storage, read> inp: array<u32>;
@compute @workgroup_size(64) fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    let x = inp[i];
    out[i] = (x * 3u + 7u) ^ (x >> 2u);
}
";
    let r = coral_reef::compile_wgsl(wgsl, &opts_gcn5());
    assert!(r.is_ok(), "GCN5 integer ALU: {}", r.unwrap_err());
}
