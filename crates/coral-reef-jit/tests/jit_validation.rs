// SPDX-License-Identifier: AGPL-3.0-only
//! Integration tests for the Cranelift JIT backend and CoralIR interpreter.
//!
//! Validates shader correctness via up to three paths:
//! - **Path A**: Naga IR tree-walk interpreter (`execute_cpu`)
//! - **Path B**: Cranelift JIT backend (`execute_jit`)
//! - **Path C**: CoralIR reference executor (`execute_coral_ir`)
//!
//! Every shader that passes JIT is also validated against the CoralIR interpreter
//! to prove the optimization pipeline preserves semantics.
//!
//! Shared memory tests are in `jit_shared_memory.rs`.

mod common;

use common::{
    assert_f32_close, assert_triple_path_f32, assert_triple_path_u32, f32_bytes, make_request,
    read_f32s, ro_f32_binding, rw_f32_binding, rw_zero_bytes,
};
use coral_reef_jit::execute_jit;

// ==========================================================================
// Arithmetic shaders
// ==========================================================================

#[test]
fn add_two_buffers() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read> a: array<f32>;
@group(0) @binding(1) var<storage, read> b: array<f32>;
@group(0) @binding(2) var<storage, read_write> out: array<f32>;

@compute @workgroup_size(1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    out[gid.x] = a[gid.x] + b[gid.x];
}
";
    let request = make_request(
        wgsl,
        [4, 1, 1],
        vec![
            ro_f32_binding(0, 0, &[1.0, 2.0, 3.0, 4.0]),
            ro_f32_binding(0, 1, &[10.0, 20.0, 30.0, 40.0]),
            rw_f32_binding(0, 2, &[0.0; 4]),
        ],
    );

    assert_triple_path_f32(&request, 2, &[11.0, 22.0, 33.0, 44.0], "add_two_buffers");
}

#[test]
fn subtract_buffers() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read> a: array<f32>;
@group(0) @binding(1) var<storage, read> b: array<f32>;
@group(0) @binding(2) var<storage, read_write> out: array<f32>;

@compute @workgroup_size(1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    out[gid.x] = a[gid.x] - b[gid.x];
}
";
    let request = make_request(
        wgsl,
        [3, 1, 1],
        vec![
            ro_f32_binding(0, 0, &[10.0, 20.0, 30.0]),
            ro_f32_binding(0, 1, &[1.0, 5.0, 10.0]),
            rw_f32_binding(0, 2, &[0.0; 3]),
        ],
    );

    assert_triple_path_f32(&request, 2, &[9.0, 15.0, 20.0], "subtract_buffers");
}

#[test]
fn multiply_by_constant() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> data: array<f32>;

@compute @workgroup_size(1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    data[gid.x] = data[gid.x] * 2.5;
}
";
    let request = make_request(
        wgsl,
        [3, 1, 1],
        vec![rw_f32_binding(0, 0, &[2.0, 4.0, 6.0])],
    );

    assert_triple_path_f32(&request, 0, &[5.0, 10.0, 15.0], "multiply_by_constant");
}

#[test]
fn fused_multiply_add() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read> a: array<f32>;
@group(0) @binding(1) var<storage, read> b: array<f32>;
@group(0) @binding(2) var<storage, read_write> out: array<f32>;

@compute @workgroup_size(1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    out[gid.x] = fma(a[gid.x], b[gid.x], 1.0);
}
";
    let request = make_request(
        wgsl,
        [3, 1, 1],
        vec![
            ro_f32_binding(0, 0, &[2.0, 3.0, 4.0]),
            ro_f32_binding(0, 1, &[5.0, 6.0, 7.0]),
            rw_f32_binding(0, 2, &[0.0; 3]),
        ],
    );

    assert_triple_path_f32(&request, 2, &[11.0, 19.0, 29.0], "fma");
}

#[test]
fn integer_arithmetic() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<u32>;

@compute @workgroup_size(1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let idx = gid.x;
    out[idx] = idx * idx + 1u;
}
";
    let request = make_request(wgsl, [4, 1, 1], vec![rw_zero_bytes(0, 0, 16)]);

    assert_triple_path_u32(&request, 0, &[1, 2, 5, 10], "integer_arithmetic");
}

#[test]
fn negative_float_values() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> data: array<f32>;

@compute @workgroup_size(1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    data[gid.x] = -data[gid.x];
}
";
    let request = make_request(
        wgsl,
        [4, 1, 1],
        vec![rw_f32_binding(0, 0, &[-3.0, 0.0, 2.5, -7.5])],
    );

    assert_triple_path_f32(&request, 0, &[3.0, 0.0, -2.5, 7.5], "negate");
}

// ==========================================================================
// Write constant to output (simplest possible)
// ==========================================================================

#[test]
fn write_constant() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;

@compute @workgroup_size(1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    out[gid.x] = 42.0;
}
";
    let request = make_request(wgsl, [2, 1, 1], vec![rw_f32_binding(0, 0, &[0.0, 0.0])]);

    assert_triple_path_f32(&request, 0, &[42.0, 42.0], "write_constant");
}

// ==========================================================================
// Control flow: conditionals, loops
// ==========================================================================

#[test]
fn conditional_select() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;

@compute @workgroup_size(1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    if gid.x % 2u == 0u {
        out[gid.x] = 1.0;
    } else {
        out[gid.x] = -1.0;
    }
}
";
    let request = make_request(wgsl, [4, 1, 1], vec![rw_f32_binding(0, 0, &[0.0; 4])]);

    assert_triple_path_f32(&request, 0, &[1.0, -1.0, 1.0, -1.0], "conditional_select");
}

#[test]
fn min_max_clamp() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> data: array<f32>;

@compute @workgroup_size(1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let v = data[gid.x];
    data[gid.x] = max(min(v, 1.0), -1.0);
}
";
    let request = make_request(
        wgsl,
        [5, 1, 1],
        vec![rw_f32_binding(0, 0, &[-5.0, -0.5, 0.0, 0.5, 5.0])],
    );

    assert_triple_path_f32(&request, 0, &[-1.0, -0.5, 0.0, 0.5, 1.0], "clamp");
}

#[test]
fn for_loop_accumulation() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<u32>;

@compute @workgroup_size(1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    var sum: u32 = 0u;
    let n = gid.x + 1u;
    for (var i: u32 = 0u; i < n; i = i + 1u) {
        sum = sum + i;
    }
    out[gid.x] = sum;
}
";
    let request = make_request(wgsl, [5, 1, 1], vec![rw_zero_bytes(0, 0, 20)]);
    // sum(0..n) = n*(n-1)/2: n=1→0, n=2→1, n=3→3, n=4→6, n=5→10
    assert_triple_path_u32(&request, 0, &[0, 1, 3, 6, 10], "for_loop_accumulation");
}

#[test]
fn barracuda_scalar_dot_product() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read> a: array<f32>;
@group(0) @binding(1) var<storage, read> b: array<f32>;
@group(0) @binding(2) var<storage, read_write> out: array<f32>;

@compute @workgroup_size(1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    var acc: f32 = 0.0;
    let base = gid.x * 4u;
    for (var i: u32 = 0u; i < 4u; i = i + 1u) {
        acc = acc + a[base + i] * b[base + i];
    }
    out[gid.x] = acc;
}
";
    let a = [1.0_f32, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0];
    let b = [2.0_f32, 3.0, 4.0, 5.0, 1.0, 1.0, 1.0, 1.0];
    let expected = [
        1.0 * 2.0 + 2.0 * 3.0 + 3.0 * 4.0 + 4.0 * 5.0,
        5.0 * 1.0 + 6.0 * 1.0 + 7.0 * 1.0 + 8.0 * 1.0,
    ];
    let request = make_request(
        wgsl,
        [2, 1, 1],
        vec![
            ro_f32_binding(0, 0, &a),
            ro_f32_binding(0, 1, &b),
            rw_f32_binding(0, 2, &[0.0; 2]),
        ],
    );
    assert_triple_path_f32(&request, 2, &expected, "barracuda_scalar_dot");
}

#[test]
fn barracuda_scalar_sum_reduce() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read> input: array<f32>;
@group(0) @binding(1) var<storage, read_write> output: array<f32>;

@compute @workgroup_size(1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    var acc: f32 = 0.0;
    let base = gid.x * 4u;
    for (var i: u32 = 0u; i < 4u; i = i + 1u) {
        acc = acc + input[base + i];
    }
    output[gid.x] = acc;
}
";
    let input = [1.0_f32, 2.0, 3.0, 4.0, 10.0, 20.0, 30.0, 40.0];
    let expected = [10.0_f32, 100.0];
    let request = make_request(
        wgsl,
        [2, 1, 1],
        vec![
            ro_f32_binding(0, 0, &input),
            rw_f32_binding(0, 1, &[0.0; 2]),
        ],
    );
    assert_triple_path_f32(&request, 1, &expected, "barracuda_scalar_sum");
}

#[test]
fn barracuda_scalar_mean() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read> input: array<f32>;
@group(0) @binding(1) var<storage, read_write> output: array<f32>;

@compute @workgroup_size(1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    var acc: f32 = 0.0;
    let n: u32 = 4u;
    let base = gid.x * n;
    for (var i: u32 = 0u; i < n; i = i + 1u) {
        acc = acc + input[base + i];
    }
    output[gid.x] = acc / f32(n);
}
";
    let input = [2.0_f32, 4.0, 6.0, 8.0, 10.0, 20.0, 30.0, 40.0];
    let expected = [5.0_f32, 25.0];
    let request = make_request(
        wgsl,
        [2, 1, 1],
        vec![
            ro_f32_binding(0, 0, &input),
            rw_f32_binding(0, 1, &[0.0; 2]),
        ],
    );
    assert_triple_path_f32(&request, 1, &expected, "barracuda_scalar_mean");
}

#[test]
fn barracuda_scalar_variance() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read> input: array<f32>;
@group(0) @binding(1) var<storage, read_write> output: array<f32>;

@compute @workgroup_size(1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let n: u32 = 4u;
    let base = gid.x * n;
    var mean_acc: f32 = 0.0;
    for (var i: u32 = 0u; i < n; i = i + 1u) {
        mean_acc = mean_acc + input[base + i];
    }
    let mean = mean_acc / f32(n);
    var var_acc: f32 = 0.0;
    for (var j: u32 = 0u; j < n; j = j + 1u) {
        let diff = input[base + j] - mean;
        var_acc = var_acc + diff * diff;
    }
    output[gid.x] = var_acc / f32(n);
}
";
    let input = [2.0_f32, 4.0, 6.0, 8.0];
    let mean = 5.0_f32;
    let var =
        ((2.0 - mean).powi(2) + (4.0 - mean).powi(2) + (6.0 - mean).powi(2) + (8.0 - mean).powi(2))
            / 4.0;
    let request = make_request(
        wgsl,
        [1, 1, 1],
        vec![
            ro_f32_binding(0, 0, &input),
            rw_f32_binding(0, 1, &[0.0; 1]),
        ],
    );
    assert_triple_path_f32(&request, 1, &[var], "barracuda_scalar_variance");
}

// ==========================================================================
// Multi-dimensional workgroups
// ==========================================================================

#[test]
fn multi_workgroup_dispatch() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<u32>;

@compute @workgroup_size(2)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    out[gid.x] = gid.x;
}
";
    let request = make_request(wgsl, [2, 1, 1], vec![rw_zero_bytes(0, 0, 16)]);

    assert_triple_path_u32(&request, 0, &[0, 1, 2, 3], "multi_workgroup");
}

#[test]
fn two_dimensional_workgroups() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<u32>;

@compute @workgroup_size(1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let cols = 3u;
    let idx = gid.y * cols + gid.x;
    out[idx] = gid.x + gid.y * 10u;
}
";
    // 3x2 grid → 6 invocations
    let request = make_request(wgsl, [3, 2, 1], vec![rw_zero_bytes(0, 0, 24)]);

    assert_triple_path_u32(&request, 0, &[0, 1, 2, 10, 11, 12], "2d_workgroups");
}

#[test]
fn three_dimensional_workgroups() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<u32>;

@compute @workgroup_size(1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let cols = 2u;
    let rows = 2u;
    let idx = gid.z * rows * cols + gid.y * cols + gid.x;
    out[idx] = gid.x + gid.y * 10u + gid.z * 100u;
}
";
    // 2x2x2 → 8 invocations
    let request = make_request(wgsl, [2, 2, 2], vec![rw_zero_bytes(0, 0, 32)]);

    assert_triple_path_u32(
        &request,
        0,
        &[0, 1, 10, 11, 100, 101, 110, 111],
        "3d_workgroups",
    );
}

#[test]
fn larger_buffer_64_elements() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> data: array<f32>;

@compute @workgroup_size(8)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    data[gid.x] = f32(gid.x) * 0.5;
}
";
    let request = make_request(wgsl, [8, 1, 1], vec![rw_f32_binding(0, 0, &[0.0; 64])]);

    let resp = execute_jit(&request).expect("large buffer shader");
    let values = read_f32s(&resp.bindings[0].data);
    assert_eq!(values.len(), 64);
    for (i, &v) in values.iter().enumerate() {
        assert_f32_close(v, i as f32 * 0.5, &format!("data[{i}]"));
    }
}

// ==========================================================================
// In-place operations
// ==========================================================================

#[test]
fn in_place_square() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> data: array<f32>;

@compute @workgroup_size(1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let v = data[gid.x];
    data[gid.x] = v * v;
}
";
    let request = make_request(
        wgsl,
        [4, 1, 1],
        vec![rw_f32_binding(0, 0, &[2.0, 3.0, 4.0, 5.0])],
    );

    assert_triple_path_f32(&request, 0, &[4.0, 9.0, 16.0, 25.0], "in_place_square");
}

// ==========================================================================
// Dual-path validation (compare JIT vs interpreter)
// ==========================================================================

#[test]
fn triple_path_fma_consistency() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;

@compute @workgroup_size(1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let x = f32(gid.x + 1u);
    out[gid.x] = x * x + x;
}
";
    let request = make_request(wgsl, [4, 1, 1], vec![rw_f32_binding(0, 0, &[0.0; 4])]);
    assert_triple_path_f32(&request, 0, &[2.0, 6.0, 12.0, 20.0], "triple_fma");
}

#[test]
fn triple_path_conditional_consistency() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;

@compute @workgroup_size(1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    if gid.x % 2u == 0u {
        out[gid.x] = f32(gid.x) * 2.0;
    } else {
        out[gid.x] = f32(gid.x) * -1.0;
    }
}
";
    let request = make_request(wgsl, [6, 1, 1], vec![rw_f32_binding(0, 0, &[0.0; 6])]);
    assert_triple_path_f32(
        &request,
        0,
        &[0.0, -1.0, 4.0, -3.0, 8.0, -5.0],
        "triple_conditional",
    );
}

// barraCuda activation, elementwise, and unary tests are in jit_barracuda.rs

// ==========================================================================
// Error handling / negative tests
// ==========================================================================

// This marker replaces the barraCuda tests extracted to jit_barracuda.rs.
// The section comment is kept so we don't orphan the error handling tests below.
//
// Tests moved: sigmoid, relu, leaky_relu, elu, hardsigmoid, hardtanh, silu,
// elementwise_add/sub/mul/fma, abs, sqrt, sign, workgroup_size_256_relu.

// ==========================================================================
// Error handling / negative tests
// ==========================================================================

#[test]
fn invalid_wgsl_returns_error() {
    let request = make_request("not valid wgsl {{{{", [1, 1, 1], vec![]);
    let result = execute_jit(&request);
    assert!(result.is_err(), "invalid WGSL should fail");
}

#[test]
fn empty_shader_no_bindings() {
    let wgsl = r"
@compute @workgroup_size(1)
fn main() {}
";
    let request = make_request(wgsl, [1, 1, 1], vec![]);
    let resp = execute_jit(&request).expect("empty shader should succeed");
    assert!(resp.bindings.is_empty());
    assert!(resp.execution_time_ns > 0);
}

// ==========================================================================
// Dual-path via coralreef-core handler
// ==========================================================================

#[test]
fn dual_path_validate_handler() {
    use coral_reef_cpu::types::{ExpectedBinding, Tolerance, ValidateRequest};

    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> output: array<f32>;
@compute @workgroup_size(1)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    output[id.x] = f32(id.x) * 2.0 + 1.0;
}
";
    let request = ValidateRequest {
        wgsl_source: wgsl.into(),
        entry_point: None,
        workgroups: [3, 1, 1],
        bindings: vec![rw_f32_binding(0, 0, &[0.0; 3])],
        uniforms: vec![],
        expected: vec![ExpectedBinding {
            group: 0,
            binding: 0,
            data: f32_bytes(&[1.0, 3.0, 5.0]),
            tolerance: Tolerance {
                abs: 1e-5,
                rel: 1e-5,
            },
        }],
    };

    let resp = coralreef_core::service::handle_validate(&request).expect("validate handler");
    assert!(resp.passed, "validation should pass: {:?}", resp.mismatches);

    if let Some(dual) = &resp.dual_path {
        assert!(
            dual.paths_agree || dual.note.is_some(),
            "dual-path should agree or have a note: {dual:?}"
        );
    }
}

// ==========================================================================
// Execution timing
// ==========================================================================

#[test]
fn execution_reports_nonzero_time() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@compute @workgroup_size(1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    out[gid.x] = f32(gid.x);
}
";
    let request = make_request(wgsl, [4, 1, 1], vec![rw_f32_binding(0, 0, &[0.0; 4])]);
    let resp = execute_jit(&request).expect("timing test");
    assert!(resp.execution_time_ns > 0, "should report nonzero time");
}

// Shared memory tests (Tier 2-3) are in jit_shared_memory.rs
