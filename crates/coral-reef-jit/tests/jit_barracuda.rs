// SPDX-License-Identifier: AGPL-3.0-only
//! barraCuda-pattern validation: activations, elementwise ops, unary ops.

mod common;

use common::{assert_triple_path_f32, make_request, ro_f32_binding, rw_f32_binding};

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
    let inputs = [-2.0_f32, -1.0, 0.0, 1.0, 2.0];
    #[expect(
        clippy::cast_possible_truncation,
        reason = "reference f64→f32 demotion"
    )]
    let expected: Vec<f32> = inputs
        .iter()
        .map(|&x| (1.0 / (1.0 + (-f64::from(x)).exp())) as f32)
        .collect();
    let request = make_request(
        wgsl,
        [5, 1, 1],
        vec![
            ro_f32_binding(0, 0, &inputs),
            rw_f32_binding(0, 1, &[0.0; 5]),
        ],
    );
    assert_triple_path_f32(&request, 1, &expected, "barracuda_sigmoid");
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
    let inputs = [-3.0_f32, -1.5, 0.0, 1.5, 3.0];
    let expected: Vec<f32> = inputs.iter().map(|&x| x.max(0.0)).collect();
    let request = make_request(
        wgsl,
        [5, 1, 1],
        vec![
            ro_f32_binding(0, 0, &inputs),
            rw_f32_binding(0, 1, &[0.0; 5]),
        ],
    );
    assert_triple_path_f32(&request, 1, &expected, "barracuda_relu");
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
    let inputs = [-4.0_f32, -1.0, 0.0, 1.0, 4.0];
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
    assert_triple_path_f32(&request, 1, &expected, "barracuda_leaky_relu");
}

#[test]
fn barracuda_elu_validation() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read> input: array<f32>;
@group(0) @binding(1) var<storage, read_write> output: array<f32>;

@compute @workgroup_size(1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let x = input[gid.x];
    if x > 0.0 {
        output[gid.x] = x;
    } else {
        output[gid.x] = exp2(x * 1.4426950408889634) - 1.0;
    }
}
";
    let inputs = [-2.0_f32, -1.0, -0.5, 0.0, 0.5, 1.0, 2.0];
    #[expect(
        clippy::cast_possible_truncation,
        reason = "reference f64→f32 demotion"
    )]
    let expected: Vec<f32> = inputs
        .iter()
        .map(|&x| {
            if x > 0.0 {
                x
            } else {
                (f64::from(x).exp() - 1.0) as f32
            }
        })
        .collect();
    let request = make_request(
        wgsl,
        [7, 1, 1],
        vec![
            ro_f32_binding(0, 0, &inputs),
            rw_f32_binding(0, 1, &[0.0; 7]),
        ],
    );
    assert_triple_path_f32(&request, 1, &expected, "barracuda_elu");
}

#[test]
fn barracuda_hardsigmoid_validation() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read> input: array<f32>;
@group(0) @binding(1) var<storage, read_write> output: array<f32>;

@compute @workgroup_size(1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let x = input[gid.x];
    output[gid.x] = max(min((x + 3.0) / 6.0, 1.0), 0.0);
}
";
    let inputs = [-5.0_f32, -3.0, -1.0, 0.0, 1.0, 3.0, 5.0];
    let expected: Vec<f32> = inputs
        .iter()
        .map(|&x| ((x + 3.0) / 6.0).clamp(0.0, 1.0))
        .collect();
    let request = make_request(
        wgsl,
        [7, 1, 1],
        vec![
            ro_f32_binding(0, 0, &inputs),
            rw_f32_binding(0, 1, &[0.0; 7]),
        ],
    );
    assert_triple_path_f32(&request, 1, &expected, "barracuda_hardsigmoid");
}

#[test]
fn barracuda_hardtanh_validation() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read> input: array<f32>;
@group(0) @binding(1) var<storage, read_write> output: array<f32>;

@compute @workgroup_size(1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let x = input[gid.x];
    output[gid.x] = max(min(x, 1.0), -1.0);
}
";
    let inputs = [-5.0_f32, -1.0, -0.5, 0.0, 0.5, 1.0, 5.0];
    let expected: Vec<f32> = inputs.iter().map(|&x| x.clamp(-1.0, 1.0)).collect();
    let request = make_request(
        wgsl,
        [7, 1, 1],
        vec![
            ro_f32_binding(0, 0, &inputs),
            rw_f32_binding(0, 1, &[0.0; 7]),
        ],
    );
    assert_triple_path_f32(&request, 1, &expected, "barracuda_hardtanh");
}

#[test]
fn barracuda_silu_validation() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read> input: array<f32>;
@group(0) @binding(1) var<storage, read_write> output: array<f32>;

@compute @workgroup_size(1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let x = input[gid.x];
    let sig = 1.0 / (1.0 + exp2(-x * 1.4426950408889634));
    output[gid.x] = x * sig;
}
";
    let inputs = [-2.0_f32, -1.0, 0.0, 1.0, 2.0];
    #[expect(
        clippy::cast_possible_truncation,
        reason = "reference f64→f32 demotion"
    )]
    let expected: Vec<f32> = inputs
        .iter()
        .map(|&x| {
            let xd = f64::from(x);
            let sig = 1.0 / (1.0 + (-xd).exp());
            (xd * sig) as f32
        })
        .collect();
    let request = make_request(
        wgsl,
        [5, 1, 1],
        vec![
            ro_f32_binding(0, 0, &inputs),
            rw_f32_binding(0, 1, &[0.0; 5]),
        ],
    );
    assert_triple_path_f32(&request, 1, &expected, "barracuda_silu");
}

// ==========================================================================
// barraCuda-style validation: elementwise math (Tier 0)
// ==========================================================================

#[test]
fn barracuda_elementwise_add_validation() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read> a: array<f32>;
@group(0) @binding(1) var<storage, read> b: array<f32>;
@group(0) @binding(2) var<storage, read_write> output: array<f32>;

@compute @workgroup_size(1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    output[gid.x] = a[gid.x] + b[gid.x];
}
";
    let a = [1.5_f32, -2.3, 0.0, 100.0, -50.0, 0.001];
    let b = [0.5_f32, 2.3, -0.0, -100.0, 50.0, -0.001];
    let expected: Vec<f32> = a.iter().zip(b.iter()).map(|(&x, &y)| x + y).collect();
    let request = make_request(
        wgsl,
        [6, 1, 1],
        vec![
            ro_f32_binding(0, 0, &a),
            ro_f32_binding(0, 1, &b),
            rw_f32_binding(0, 2, &[0.0; 6]),
        ],
    );
    assert_triple_path_f32(&request, 2, &expected, "barracuda_add");
}

#[test]
fn barracuda_elementwise_sub_validation() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read> a: array<f32>;
@group(0) @binding(1) var<storage, read> b: array<f32>;
@group(0) @binding(2) var<storage, read_write> output: array<f32>;

@compute @workgroup_size(1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    output[gid.x] = a[gid.x] - b[gid.x];
}
";
    let a = [10.0_f32, 0.0, -5.0, 3.14, 1e6];
    let b = [3.0_f32, 0.0, 5.0, 3.14, 1e6];
    let expected: Vec<f32> = a.iter().zip(b.iter()).map(|(&x, &y)| x - y).collect();
    let request = make_request(
        wgsl,
        [5, 1, 1],
        vec![
            ro_f32_binding(0, 0, &a),
            ro_f32_binding(0, 1, &b),
            rw_f32_binding(0, 2, &[0.0; 5]),
        ],
    );
    assert_triple_path_f32(&request, 2, &expected, "barracuda_sub");
}

#[test]
fn barracuda_elementwise_mul_validation() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read> a: array<f32>;
@group(0) @binding(1) var<storage, read> b: array<f32>;
@group(0) @binding(2) var<storage, read_write> output: array<f32>;

@compute @workgroup_size(1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    output[gid.x] = a[gid.x] * b[gid.x];
}
";
    let a = [2.0_f32, -3.0, 0.0, 0.5, 1e3];
    let b = [5.0_f32, -2.0, 100.0, 0.5, 1e-3];
    let expected: Vec<f32> = a.iter().zip(b.iter()).map(|(&x, &y)| x * y).collect();
    let request = make_request(
        wgsl,
        [5, 1, 1],
        vec![
            ro_f32_binding(0, 0, &a),
            ro_f32_binding(0, 1, &b),
            rw_f32_binding(0, 2, &[0.0; 5]),
        ],
    );
    assert_triple_path_f32(&request, 2, &expected, "barracuda_mul");
}

#[test]
fn barracuda_elementwise_fma_validation() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read> a: array<f32>;
@group(0) @binding(1) var<storage, read> b: array<f32>;
@group(0) @binding(2) var<storage, read> c: array<f32>;
@group(0) @binding(3) var<storage, read_write> output: array<f32>;

@compute @workgroup_size(1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    output[gid.x] = fma(a[gid.x], b[gid.x], c[gid.x]);
}
";
    let a = [2.0_f32, 3.0, -1.0, 0.5];
    let b = [5.0_f32, -2.0, 4.0, 8.0];
    let c = [1.0_f32, 1.0, 1.0, 1.0];
    let expected: Vec<f32> = a
        .iter()
        .zip(b.iter())
        .zip(c.iter())
        .map(|((&x, &y), &z)| x.mul_add(y, z))
        .collect();
    let request = make_request(
        wgsl,
        [4, 1, 1],
        vec![
            ro_f32_binding(0, 0, &a),
            ro_f32_binding(0, 1, &b),
            ro_f32_binding(0, 2, &c),
            rw_f32_binding(0, 3, &[0.0; 4]),
        ],
    );
    assert_triple_path_f32(&request, 3, &expected, "barracuda_fma");
}

#[test]
fn barracuda_abs_validation() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read> input: array<f32>;
@group(0) @binding(1) var<storage, read_write> output: array<f32>;

@compute @workgroup_size(1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    output[gid.x] = abs(input[gid.x]);
}
";
    let inputs = [-5.0_f32, -0.001, 0.0, 0.001, 5.0];
    let expected: Vec<f32> = inputs.iter().map(|x| x.abs()).collect();
    let request = make_request(
        wgsl,
        [5, 1, 1],
        vec![
            ro_f32_binding(0, 0, &inputs),
            rw_f32_binding(0, 1, &[0.0; 5]),
        ],
    );
    assert_triple_path_f32(&request, 1, &expected, "barracuda_abs");
}

#[test]
fn barracuda_sqrt_validation() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read> input: array<f32>;
@group(0) @binding(1) var<storage, read_write> output: array<f32>;

@compute @workgroup_size(1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    output[gid.x] = sqrt(input[gid.x]);
}
";
    let inputs = [0.0_f32, 1.0, 4.0, 9.0, 16.0, 25.0];
    let expected: Vec<f32> = inputs.iter().map(|x| x.sqrt()).collect();
    let request = make_request(
        wgsl,
        [6, 1, 1],
        vec![
            ro_f32_binding(0, 0, &inputs),
            rw_f32_binding(0, 1, &[0.0; 6]),
        ],
    );
    assert_triple_path_f32(&request, 1, &expected, "barracuda_sqrt");
}

#[test]
fn barracuda_sign_validation() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read> input: array<f32>;
@group(0) @binding(1) var<storage, read_write> output: array<f32>;

@compute @workgroup_size(1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let x = input[gid.x];
    output[gid.x] = sign(x);
}
";
    let inputs = [-5.0_f32, -0.001, 0.0, 0.001, 5.0];
    let expected: Vec<f32> = inputs
        .iter()
        .map(|&x| {
            if x > 0.0 {
                1.0
            } else if x < 0.0 {
                -1.0
            } else {
                0.0
            }
        })
        .collect();
    let request = make_request(
        wgsl,
        [5, 1, 1],
        vec![
            ro_f32_binding(0, 0, &inputs),
            rw_f32_binding(0, 1, &[0.0; 5]),
        ],
    );
    assert_triple_path_f32(&request, 1, &expected, "barracuda_sign");
}

#[test]
fn barracuda_workgroup_size_256_relu() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read> input: array<f32>;
@group(0) @binding(1) var<storage, read_write> output: array<f32>;

@compute @workgroup_size(256)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    output[gid.x] = max(input[gid.x], 0.0);
}
";
    let n = 256;
    let inputs: Vec<f32> = (0..n).map(|i| (i as f32) - 128.0).collect();
    let expected: Vec<f32> = inputs.iter().map(|&x| x.max(0.0)).collect();
    let request = make_request(
        wgsl,
        [1, 1, 1],
        vec![
            ro_f32_binding(0, 0, &inputs),
            rw_f32_binding(0, 1, &vec![0.0; n]),
        ],
    );
    assert_triple_path_f32(&request, 1, &expected, "barracuda_wg256_relu");
}
