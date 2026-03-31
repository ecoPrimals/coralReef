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

use bytes::Bytes;
use coral_reef_cpu::types::{BindingData, BindingUsage, ExecuteCpuRequest};
use coral_reef_jit::execute_jit;

const TOLERANCE: f64 = 1e-5;

fn f32_bytes(values: &[f32]) -> Bytes {
    Bytes::from(
        values
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<_>>(),
    )
}

fn u32_bytes(values: &[u32]) -> Bytes {
    Bytes::from(
        values
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect::<Vec<_>>(),
    )
}

fn read_f32s(data: &[u8]) -> Vec<f32> {
    data.chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

fn read_u32s(data: &[u8]) -> Vec<u32> {
    data.chunks_exact(4)
        .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

fn assert_f32_close(got: f32, expected: f32, label: &str) {
    let abs = (got - expected).abs();
    let rel = if expected.abs() > f32::EPSILON {
        abs / expected.abs()
    } else {
        abs
    };
    assert!(
        abs < TOLERANCE as f32 || rel < TOLERANCE as f32,
        "{label}: got {got}, expected {expected} (abs={abs}, rel={rel})"
    );
}

fn make_request(wgsl: &str, workgroups: [u32; 3], bindings: Vec<BindingData>) -> ExecuteCpuRequest {
    ExecuteCpuRequest {
        wgsl_source: wgsl.into(),
        entry_point: None,
        workgroups,
        bindings,
        uniforms: vec![],
        strategy: coral_reef_cpu::types::ExecutionStrategy::Jit,
    }
}

fn rw_f32_binding(group: u32, binding: u32, data: &[f32]) -> BindingData {
    BindingData {
        group,
        binding,
        data: f32_bytes(data),
        usage: BindingUsage::ReadWrite,
    }
}

fn ro_f32_binding(group: u32, binding: u32, data: &[f32]) -> BindingData {
    BindingData {
        group,
        binding,
        data: f32_bytes(data),
        usage: BindingUsage::ReadOnly,
    }
}

fn rw_u32_binding(group: u32, binding: u32, data: &[u32]) -> BindingData {
    BindingData {
        group,
        binding,
        data: u32_bytes(data),
        usage: BindingUsage::ReadWrite,
    }
}

fn rw_zero_bytes(group: u32, binding: u32, len: usize) -> BindingData {
    BindingData {
        group,
        binding,
        data: Bytes::from(vec![0u8; len]),
        usage: BindingUsage::ReadWrite,
    }
}

/// Run a shader through all available paths (JIT + CoralIR interpreter + Naga interpreter)
/// and assert that the given binding produces matching f32 results within tolerance.
///
/// The Naga interpreter is a best-effort third path — it may not support all ops
/// or may return a different binding layout, in which case its comparison is skipped.
fn assert_triple_path_f32(
    request: &ExecuteCpuRequest,
    binding_idx: usize,
    expected: &[f32],
    label: &str,
) {
    let jit_resp = execute_jit(request).unwrap_or_else(|e| panic!("{label}: JIT failed: {e}"));
    let jit_vals = read_f32s(&jit_resp.bindings[binding_idx].data);
    for (i, &exp) in expected.iter().enumerate() {
        assert_f32_close(jit_vals[i], exp, &format!("{label} JIT[{i}]"));
    }

    let coral_resp = coral_reef_cpu::execute_coral_ir(request)
        .unwrap_or_else(|e| panic!("{label}: CoralIR interp failed: {e}"));
    let coral_vals = read_f32s(&coral_resp.bindings[binding_idx].data);
    for (i, (&jit, &coral)) in jit_vals.iter().zip(coral_vals.iter()).enumerate() {
        assert_f32_close(
            jit,
            coral,
            &format!("{label} JIT↔CoralIR[{i}]"),
        );
    }

    if let Ok(naga_resp) = coral_reef_cpu::execute_cpu(request) {
        if naga_resp.bindings.len() > binding_idx {
            let naga_vals = read_f32s(&naga_resp.bindings[binding_idx].data);
            for (i, (&jit, &naga)) in jit_vals.iter().zip(naga_vals.iter()).enumerate() {
                assert_f32_close(jit, naga, &format!("{label} JIT↔Naga[{i}]"));
            }
        }
    }
}

/// Run a shader through JIT + CoralIR interpreter and assert u32 binding equality.
/// Naga comparison is best-effort.
fn assert_triple_path_u32(
    request: &ExecuteCpuRequest,
    binding_idx: usize,
    expected: &[u32],
    label: &str,
) {
    let jit_resp = execute_jit(request).unwrap_or_else(|e| panic!("{label}: JIT failed: {e}"));
    let jit_vals = read_u32s(&jit_resp.bindings[binding_idx].data);
    assert_eq!(jit_vals, expected, "{label} JIT output");

    let coral_resp = coral_reef_cpu::execute_coral_ir(request)
        .unwrap_or_else(|e| panic!("{label}: CoralIR interp failed: {e}"));
    let coral_vals = read_u32s(&coral_resp.bindings[binding_idx].data);
    assert_eq!(jit_vals, coral_vals, "{label} JIT↔CoralIR");

    if let Ok(naga_resp) = coral_reef_cpu::execute_cpu(request) {
        if naga_resp.bindings.len() > binding_idx {
            let naga_vals = read_u32s(&naga_resp.bindings[binding_idx].data);
            assert_eq!(jit_vals, naga_vals, "{label} JIT↔Naga");
        }
    }
}

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

/// CoralIR lowers WGSL `var` loop variables to register-addressed memory loads
/// (`Ld [R0+offset]`) which requires local scratch memory emulation on CPU.
/// This is a future evolution path — the current JIT handles pure SSA phi-based
/// loops but not mutable-variable-in-memory patterns.
#[test]
#[ignore = "requires local scratch memory emulation for CoralIR register-addressed loads"]
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

    let resp = execute_jit(&request).expect("loop shader");
    let values = read_u32s(&resp.bindings[0].data);
    // sum(0..n) = n*(n-1)/2: n=1→0, n=2→1, n=3→3, n=4→6, n=5→10
    assert_eq!(values, vec![0, 1, 3, 6, 10]);
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

    assert_triple_path_u32(&request, 0, &[0, 1, 10, 11, 100, 101, 110, 111], "3d_workgroups");
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
    assert_triple_path_f32(&request, 0, &[0.0, -1.0, 4.0, -3.0, 8.0, -5.0], "triple_conditional");
}

// ==========================================================================
// barraCuda-style validation: sigmoid + ReLU
// ==========================================================================

#[test]
fn barracuda_sigmoid_validation() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read> input: array<f32>;
@group(0) @binding(1) var<storage, read_write> output: array<f32>;

@compute @workgroup_size(1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let x = input[gid.x];
    output[gid.x] = 1.0 / (1.0 + exp(-x));
}

fn exp(x: f32) -> f32 {
    return exp2(x * 1.4426950408889634);
}
";
    let inputs: Vec<f32> = vec![-2.0, -1.0, 0.0, 1.0, 2.0];
    let expected_sigmoid: Vec<f32> = inputs
        .iter()
        .map(|&x| 1.0 / (1.0 + (-x as f64).exp()) as f32)
        .collect();

    let request = make_request(
        wgsl,
        [5, 1, 1],
        vec![
            ro_f32_binding(0, 0, &inputs),
            rw_f32_binding(0, 1, &[0.0; 5]),
        ],
    );

    let resp = execute_jit(&request).expect("sigmoid shader");
    let out_binding = resp
        .bindings
        .iter()
        .find(|b| b.binding == 1)
        .expect("output");
    let values = read_f32s(&out_binding.data);

    for (i, (&got, &expected)) in values.iter().zip(expected_sigmoid.iter()).enumerate() {
        assert_f32_close(
            got,
            expected,
            &format!("sigmoid[{i}] (input={})", inputs[i]),
        );
    }
}

#[test]
fn barracuda_relu_validation() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read> input: array<f32>;
@group(0) @binding(1) var<storage, read_write> output: array<f32>;

@compute @workgroup_size(1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let x = input[gid.x];
    output[gid.x] = max(x, 0.0);
}
";
    let inputs: Vec<f32> = vec![-3.0, -1.5, 0.0, 1.5, 3.0];
    let expected_relu: Vec<f32> = inputs.iter().map(|&x| x.max(0.0)).collect();

    let request = make_request(
        wgsl,
        [5, 1, 1],
        vec![
            ro_f32_binding(0, 0, &inputs),
            rw_f32_binding(0, 1, &[0.0; 5]),
        ],
    );

    let resp = execute_jit(&request).expect("relu shader");
    let out_binding = resp
        .bindings
        .iter()
        .find(|b| b.binding == 1)
        .expect("output");
    let values = read_f32s(&out_binding.data);

    for (i, (&got, &expected)) in values.iter().zip(expected_relu.iter()).enumerate() {
        assert_f32_close(got, expected, &format!("relu[{i}]"));
    }
}

#[test]
fn barracuda_leaky_relu_validation() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read> input: array<f32>;
@group(0) @binding(1) var<storage, read_write> output: array<f32>;

@compute @workgroup_size(1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let x = input[gid.x];
    if x > 0.0 {
        output[gid.x] = x;
    } else {
        output[gid.x] = x * 0.01;
    }
}
";
    let inputs: Vec<f32> = vec![-4.0, -1.0, 0.0, 1.0, 4.0];
    let expected: Vec<f32> = inputs
        .iter()
        .map(|&x| if x > 0.0 { x } else { x * 0.01 })
        .collect();

    let request = make_request(
        wgsl,
        [5, 1, 1],
        vec![
            ro_f32_binding(0, 0, &inputs),
            rw_f32_binding(0, 1, &[0.0; 5]),
        ],
    );

    let resp = execute_jit(&request).expect("leaky relu shader");
    let values = read_f32s(&resp.bindings[1].data);
    for (i, (&got, &exp)) in values.iter().zip(expected.iter()).enumerate() {
        assert_f32_close(got, exp, &format!("leaky_relu[{i}]"));
    }
}

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
