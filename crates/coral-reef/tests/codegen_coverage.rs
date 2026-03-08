// SPDX-License-Identifier: AGPL-3.0-only
//! Coverage-focused tests that exercise deep codegen paths.
//!
//! Each test targets a specific optimization or lowering pass by crafting
//! WGSL shaders that trigger those code paths through the full pipeline.
//!
//! Multi-architecture tests are in `codegen_coverage_multi_arch.rs`.
//! Gap-targeted tests are in `codegen_coverage_targeted.rs`.

use std::fmt::Write;

use coral_reef::{AmdArch, CompileOptions, GpuArch, GpuTarget};

fn opts() -> CompileOptions {
    CompileOptions {
        target: GpuArch::Sm70.into(),
        opt_level: 2,
        debug_info: false,
        fp64_software: true,
        ..CompileOptions::default()
    }
}

fn amd_opts() -> CompileOptions {
    CompileOptions {
        target: GpuTarget::Amd(AmdArch::Rdna2),
        opt_level: 2,
        debug_info: false,
        fp64_software: false,
        ..CompileOptions::default()
    }
}

fn compile(wgsl: &str) -> Result<Vec<u8>, coral_reef::CompileError> {
    coral_reef::compile_wgsl(wgsl, &opts())
}

fn compile_amd(wgsl: &str) -> Result<Vec<u8>, coral_reef::CompileError> {
    coral_reef::compile_wgsl(wgsl, &amd_opts())
}

fn compile_with_opt(wgsl: &str, opt: u32) -> Result<Vec<u8>, coral_reef::CompileError> {
    let mut o = opts();
    o.opt_level = opt;
    coral_reef::compile_wgsl(wgsl, &o)
}

fn compile_fixture_both(wgsl: &str) {
    let r_sm70 = compile(wgsl);
    assert!(r_sm70.is_ok(), "SM70: {}", r_sm70.unwrap_err());
    let r_amd = compile_amd(wgsl);
    assert!(r_amd.is_ok(), "AMD: {}", r_amd.unwrap_err());
}

fn compile_fixture_sm70(wgsl: &str) {
    let r = compile(wgsl);
    assert!(r.is_ok(), "SM70: {}", r.unwrap_err());
}

// --- Register pressure / spiller ---

#[test]
fn coverage_high_register_pressure() {
    let mut wgsl = String::from(
        "@group(0) @binding(0) var<storage, read_write> out: array<f32>;\n\
         @compute @workgroup_size(1) fn main() {\n",
    );
    for i in 0..64 {
        let _ = writeln!(wgsl, "  var v{i}: f32 = f32({i});");
    }
    for i in 0..64 {
        let _ = writeln!(wgsl, "  v{i} = v{i} * 2.0 + 1.0;");
    }
    wgsl.push_str("  out[0] = v0 + v63;\n}\n");
    let _ = compile(&wgsl);
}

// --- Phi nodes / to_cssa ---

#[test]
fn coverage_phi_nodes_if_else() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(1)
fn main() {
    if true {
        out[0] = 1.0;
    } else {
        out[0] = 2.0;
    }
}
";
    let _ = compile(wgsl);
}

#[test]
fn coverage_phi_nodes_loop_carry() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(1)
fn main() {
    var sum: f32 = 0.0;
    var i: u32 = 0u;
    loop {
        if i >= 100u { break; }
        sum = sum + f32(i);
        i = i + 1u;
    }
    out[0] = sum;
}
";
    let _ = compile(wgsl);
}

// --- Complex control flow / repair_ssa ---

#[test]
fn coverage_nested_loops() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(1)
fn main() {
    var total: f32 = 0.0;
    var i: u32 = 0u;
    loop {
        if i >= 10u { break; }
        var j: u32 = 0u;
        loop {
            if j >= 10u { break; }
            total = total + f32(i * 10u + j);
            j = j + 1u;
        }
        i = i + 1u;
    }
    out[0] = total;
}
";
    let _ = compile(wgsl);
}

#[test]
fn coverage_continue_in_loop() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(1)
fn main() {
    var sum: f32 = 0.0;
    var i: u32 = 0u;
    loop {
        if i >= 50u { break; }
        i = i + 1u;
        if i % 2u == 0u { continue; }
        sum = sum + f32(i);
    }
    out[0] = sum;
}
";
    let _ = compile(wgsl);
}

// --- Uniform instructions ---

#[test]
fn coverage_uniform_builtins() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<u32>;
@compute @workgroup_size(64)
fn main(
    @builtin(global_invocation_id) gid: vec3<u32>,
    @builtin(local_invocation_id) lid: vec3<u32>,
    @builtin(workgroup_id) wid: vec3<u32>,
    @builtin(num_workgroups) nwg: vec3<u32>,
) {
    out[gid.x] = lid.x + wid.x * 64u + nwg.x;
}
";
    let _ = compile(wgsl);
}

// --- Logical operations ---

#[test]
fn coverage_logical_predicates() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let a = id.x > 5u;
    let b = id.x < 20u;
    let c = a && b;
    let d = a || !b;
    out[id.x] = select(0.0, 1.0, c);
}
";
    let _ = compile(wgsl);
}

// --- Liveness / long live ranges ---

#[test]
fn coverage_long_live_ranges() {
    let mut wgsl = String::from(
        "@group(0) @binding(0) var<storage, read_write> out: array<f32>;\n\
         @compute @workgroup_size(1) fn main() {\n\
           let a = 1.0;\n\
           let b = 2.0;\n\
           let c = 3.0;\n",
    );
    for i in 0..32 {
        let _ = writeln!(wgsl, "  var t{i} = a + f32({i});");
    }
    for i in 0..32 {
        let _ = writeln!(wgsl, "  t{i} = t{i} * b + c;");
    }
    wgsl.push_str("  out[0] = t0 + t31;\n}\n");
    let _ = compile(&wgsl);
}

// --- Instruction scheduling ---

#[test]
fn coverage_instruction_parallelism() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(1)
fn main() {
    let a = 1.0;
    let b = 2.0;
    let c = a + b;
    let d = a * b;
    let e = a - b;
    let f = c + d;
    let g = d * e;
    let h = f + g;
    out[0] = h;
}
";
    let _ = compile(wgsl);
}

// --- Memory operations ---

#[test]
fn coverage_shared_memory_barrier() {
    let wgsl = r"
var<workgroup> shared: array<f32, 256>;

@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(256)
fn main(@builtin(local_invocation_id) lid: vec3<u32>) {
    shared[lid.x] = f32(lid.x);
    workgroupBarrier();
    let idx = 255u - lid.x;
    out[lid.x] = shared[idx];
}
";
    let _ = compile(wgsl);
}

#[test]
fn coverage_atomic_operations() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> counter: atomic<u32>;
@group(0) @binding(1) var<storage, read_write> out: array<u32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let old = atomicAdd(&counter, 1u);
    out[gid.x] = old;
}
";
    let _ = compile(wgsl);
}

// --- f64 transcendentals ---

#[test]
fn coverage_f64_all_transcendentals() {
    let wgsl = r"
enable naga_ext_f64;
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(1)
fn main() {
    let x: f64 = 1.5;
    let sq = sqrt(x);
    let r = 1.0 / x;
    let e = exp2(x);
    let l = log2(x);
    let s = sin(x);
    let c = cos(x);
    out[0] = f32(s + c + e + l + sq + r);
}
";
    let _ = compile(wgsl);
}

// --- Optimization levels ---

#[test]
fn coverage_all_opt_levels() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(1)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    var x = f32(id.x);
    x = x * x + 2.0 * x + 1.0;
    if x > 100.0 { x = 100.0; }
    out[id.x] = x;
}
";
    for opt in 0..=3 {
        let _ = compile_with_opt(wgsl, opt);
    }
}

// --- Type conversions ---

#[test]
fn coverage_type_conversions() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(1)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let u = id.x;
    let i = i32(u);
    let f = f32(i);
    let u2 = u32(f);
    let b = u > 0u;
    out[0] = select(0.0, f, b);
}
";
    let _ = compile(wgsl);
}

// --- Multiple bindings ---

#[test]
fn coverage_multiple_bindings() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read> input_a: array<f32>;
@group(0) @binding(1) var<storage, read> input_b: array<f32>;
@group(0) @binding(2) var<storage, read_write> output: array<f32>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    output[id.x] = input_a[id.x] + input_b[id.x];
}
";
    let _ = compile(wgsl);
}

// --- Fixture-based integration tests (SM70 + AMD) ---

#[test]
fn fixture_control_flow_nested_if_switch() {
    let wgsl = include_str!("fixtures/wgsl/control_flow_nested_if_switch.wgsl");
    compile_fixture_sm70(wgsl);
}

#[test]
fn fixture_control_flow_for_while() {
    let wgsl = include_str!("fixtures/wgsl/control_flow_for_while.wgsl");
    compile_fixture_sm70(wgsl);
}

#[test]
fn fixture_control_flow_break_continue() {
    let wgsl = include_str!("fixtures/wgsl/control_flow_break_continue.wgsl");
    compile_fixture_sm70(wgsl);
}

#[test]
fn fixture_math_mix_clamp_step() {
    let wgsl = include_str!("fixtures/wgsl/math_mix_clamp_step.wgsl");
    compile_fixture_sm70(wgsl);
}

#[test]
fn fixture_math_fract_ceil_floor_round() {
    let wgsl = include_str!("fixtures/wgsl/math_fract_ceil_floor_round.wgsl");
    compile_fixture_sm70(wgsl);
}

#[test]
fn fixture_memory_atomics_multi() {
    let wgsl = include_str!("fixtures/wgsl/memory_atomics_multi.wgsl");
    compile_fixture_both(wgsl);
}

#[test]
fn fixture_memory_shared_storage_types() {
    let wgsl = include_str!("fixtures/wgsl/memory_shared_storage_types.wgsl");
    compile_fixture_both(wgsl);
}

#[test]
fn fixture_data_vec2_vec3_vec4() {
    let wgsl = include_str!("fixtures/wgsl/data_vec2_vec3_vec4.wgsl");
    compile_fixture_sm70(wgsl);
}

#[test]
fn fixture_data_matrices() {
    let wgsl = include_str!("fixtures/wgsl/data_matrices.wgsl");
    compile_fixture_sm70(wgsl);
}

#[test]
fn fixture_data_structs_arrays() {
    let wgsl = include_str!("fixtures/wgsl/data_structs_arrays.wgsl");
    compile_fixture_sm70(wgsl);
}

// --- Coverage-focused fixtures (10 new shaders) ---

#[test]
fn fixture_expr_binary_int_ops() {
    let wgsl = include_str!("fixtures/wgsl/expr_binary_int_ops.wgsl");
    compile_fixture_sm70(wgsl);
}

#[test]
fn fixture_func_math_transcendentals() {
    let wgsl = include_str!("fixtures/wgsl/func_math_transcendentals.wgsl");
    compile_fixture_sm70(wgsl);
}

#[test]
fn fixture_func_math_bit_ops() {
    let wgsl = include_str!("fixtures/wgsl/func_math_bit_ops.wgsl");
    compile_fixture_sm70(wgsl);
}

#[test]
fn fixture_op_conv_conversions() {
    let wgsl = include_str!("fixtures/wgsl/op_conv_conversions.wgsl");
    compile_fixture_sm70(wgsl);
}

#[test]
fn fixture_sm70_control_branches_loops_barrier() {
    let wgsl = include_str!("fixtures/wgsl/sm70_control_branches_loops_barrier.wgsl");
    compile_fixture_sm70(wgsl);
}

#[test]
fn fixture_builder_emit_complex() {
    let wgsl = include_str!("fixtures/wgsl/builder_emit_complex.wgsl");
    compile_fixture_sm70(wgsl);
}

#[test]
fn fixture_spill_register_pressure() {
    let wgsl = include_str!("fixtures/wgsl/spill_register_pressure.wgsl");
    compile_fixture_sm70(wgsl);
}

#[test]
fn fixture_lower_copy_swap() {
    let wgsl = include_str!("fixtures/wgsl/lower_copy_swap.wgsl");
    compile_fixture_sm70(wgsl);
}

#[test]
fn fixture_sm70_alu_int_signed() {
    let wgsl = include_str!("fixtures/wgsl/sm70_alu_int_signed.wgsl");
    compile_fixture_sm70(wgsl);
}

#[test]
fn fixture_sm70_alu_float_fma() {
    let wgsl = include_str!("fixtures/wgsl/sm70_alu_float_fma.wgsl");
    compile_fixture_sm70(wgsl);
}
