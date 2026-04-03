// SPDX-License-Identifier: AGPL-3.0-only
//! Tier 2-3 shared memory tests: barriers, reductions, matmul, layer norm.
//!
//! These tests exercise `var<workgroup>`, `workgroupBarrier()`, and
//! cooperative scheduling in the CoralIR interpreter.

mod common;

use common::{
    assert_f32_close, f32_bytes, read_f32s, read_u32s, rw_f32_binding, rw_zero_bytes,
};
use coral_reef_cpu::types::{BindingData, BindingUsage, ExecuteCpuRequest};

#[test]
fn shared_memory_swap_via_barrier() {
    let wgsl = r"
var<workgroup> tile: array<u32, 4>;

@group(0) @binding(0) var<storage, read_write> out: array<u32>;

@compute @workgroup_size(4)
fn main(@builtin(local_invocation_id) lid: vec3<u32>) {
    tile[lid.x] = lid.x + 1u;
    workgroupBarrier();
    let neighbor = (lid.x + 1u) % 4u;
    out[lid.x] = tile[neighbor];
}
";
    let request = ExecuteCpuRequest {
        wgsl_source: wgsl.into(),
        entry_point: None,
        workgroups: [1, 1, 1],
        bindings: vec![rw_zero_bytes(0, 0, 16)],
        uniforms: vec![],
        strategy: coral_reef_cpu::types::ExecutionStrategy::Interpret,
    };
    let resp =
        coral_reef_cpu::execute_coral_ir(&request).expect("shared_memory_swap: CoralIR interp");
    let vals = read_u32s(&resp.bindings[0].data);
    assert_eq!(vals, vec![2, 3, 4, 1], "shared_memory_swap output");
}

#[test]
fn shared_memory_tree_reduce_sum() {
    let wgsl = r"
var<workgroup> shared_data: array<f32, 8>;

@group(0) @binding(0) var<storage, read_write> input: array<f32>;
@group(0) @binding(1) var<storage, read_write> output: array<f32>;

@compute @workgroup_size(8)
fn main(@builtin(local_invocation_id) lid: vec3<u32>) {
    shared_data[lid.x] = input[lid.x];
    workgroupBarrier();

    if lid.x < 4u {
        shared_data[lid.x] = shared_data[lid.x] + shared_data[lid.x + 4u];
    }
    workgroupBarrier();

    if lid.x < 2u {
        shared_data[lid.x] = shared_data[lid.x] + shared_data[lid.x + 2u];
    }
    workgroupBarrier();

    if lid.x == 0u {
        shared_data[0u] = shared_data[0u] + shared_data[1u];
        output[0u] = shared_data[0u];
    }
}
";
    let input: Vec<f32> = (1..=8).map(|x| x as f32).collect();
    let request = ExecuteCpuRequest {
        wgsl_source: wgsl.into(),
        entry_point: None,
        workgroups: [1, 1, 1],
        bindings: vec![rw_f32_binding(0, 0, &input), rw_zero_bytes(0, 1, 4)],
        uniforms: vec![],
        strategy: coral_reef_cpu::types::ExecutionStrategy::Interpret,
    };
    let resp = coral_reef_cpu::execute_coral_ir(&request).expect("tree_reduce: CoralIR interp");
    let result = read_f32s(&resp.bindings[1].data);
    let expected_sum: f32 = (1..=8).map(|x| x as f32).sum();
    assert_f32_close(result[0], expected_sum, "tree_reduce sum");
}

#[test]
fn barracuda_sum_reduce_f32_workgroup() {
    let wgsl = r"
var<workgroup> smem: array<f32, 16>;

@group(0) @binding(0) var<storage, read> input: array<f32>;
@group(0) @binding(1) var<storage, read_write> output: array<f32>;

@compute @workgroup_size(16)
fn main(
    @builtin(local_invocation_id) lid: vec3<u32>,
    @builtin(global_invocation_id) gid: vec3<u32>,
) {
    smem[lid.x] = input[gid.x];
    workgroupBarrier();

    for (var stride = 8u; stride > 0u; stride = stride >> 1u) {
        if lid.x < stride {
            smem[lid.x] = smem[lid.x] + smem[lid.x + stride];
        }
        workgroupBarrier();
    }

    if lid.x == 0u {
        output[0u] = smem[0u];
    }
}
";
    let input: Vec<f32> = (1..=16).map(|x| x as f32).collect();
    let expected_sum: f32 = input.iter().sum();
    let request = ExecuteCpuRequest {
        wgsl_source: wgsl.into(),
        entry_point: None,
        workgroups: [1, 1, 1],
        bindings: vec![
            BindingData {
                group: 0,
                binding: 0,
                data: f32_bytes(&input),
                usage: BindingUsage::ReadOnly,
            },
            rw_zero_bytes(0, 1, 4),
        ],
        uniforms: vec![],
        strategy: coral_reef_cpu::types::ExecutionStrategy::Interpret,
    };
    let resp =
        coral_reef_cpu::execute_coral_ir(&request).expect("barracuda_sum_reduce: CoralIR interp");
    let result = read_f32s(&resp.bindings[1].data);
    assert_f32_close(result[0], expected_sum, "barracuda_sum_reduce");
}

#[test]
fn barracuda_max_reduce_f32_workgroup() {
    let wgsl = r"
var<workgroup> smem: array<f32, 16>;

@group(0) @binding(0) var<storage, read> input: array<f32>;
@group(0) @binding(1) var<storage, read_write> output: array<f32>;

@compute @workgroup_size(16)
fn main(
    @builtin(local_invocation_id) lid: vec3<u32>,
    @builtin(global_invocation_id) gid: vec3<u32>,
) {
    smem[lid.x] = input[gid.x];
    workgroupBarrier();

    for (var stride = 8u; stride > 0u; stride = stride >> 1u) {
        if lid.x < stride {
            let a = smem[lid.x];
            let b = smem[lid.x + stride];
            if b > a { smem[lid.x] = b; }
        }
        workgroupBarrier();
    }

    if lid.x == 0u {
        output[0u] = smem[0u];
    }
}
";
    let input: Vec<f32> = vec![
        3.0, 7.0, 1.0, 15.0, 2.0, 9.0, 4.0, 11.0, 5.0, 13.0, 6.0, 8.0, 14.0, 10.0, 12.0, 16.0,
    ];
    let request = ExecuteCpuRequest {
        wgsl_source: wgsl.into(),
        entry_point: None,
        workgroups: [1, 1, 1],
        bindings: vec![
            BindingData {
                group: 0,
                binding: 0,
                data: f32_bytes(&input),
                usage: BindingUsage::ReadOnly,
            },
            rw_zero_bytes(0, 1, 4),
        ],
        uniforms: vec![],
        strategy: coral_reef_cpu::types::ExecutionStrategy::Interpret,
    };
    let resp =
        coral_reef_cpu::execute_coral_ir(&request).expect("barracuda_max_reduce: CoralIR interp");
    let result = read_f32s(&resp.bindings[1].data);
    assert_f32_close(result[0], 16.0, "barracuda_max_reduce");
}

#[test]
fn barracuda_layer_norm_f32() {
    let wgsl = r"
var<workgroup> smem: array<f32, 4>;

@group(0) @binding(0) var<storage, read_write> data: array<f32>;

@compute @workgroup_size(4)
fn main(@builtin(local_invocation_id) lid: vec3<u32>) {
    let val = data[lid.x];
    smem[lid.x] = val;
    workgroupBarrier();

    // Compute mean via reduction
    if lid.x < 2u { smem[lid.x] = smem[lid.x] + smem[lid.x + 2u]; }
    workgroupBarrier();
    if lid.x == 0u { smem[0u] = smem[0u] + smem[1u]; }
    workgroupBarrier();

    let mean = smem[0u] / 4.0;

    // Compute variance: store (val - mean)^2 into shared
    let diff = val - mean;
    smem[lid.x] = diff * diff;
    workgroupBarrier();

    if lid.x < 2u { smem[lid.x] = smem[lid.x] + smem[lid.x + 2u]; }
    workgroupBarrier();
    if lid.x == 0u { smem[0u] = smem[0u] + smem[1u]; }
    workgroupBarrier();

    let variance = smem[0u] / 4.0;
    let std_dev = sqrt(variance + 1e-5);
    data[lid.x] = (val - mean) / std_dev;
}
";
    let input = vec![1.0_f32, 2.0, 3.0, 4.0];
    let mean: f32 = input.iter().sum::<f32>() / 4.0;
    let var: f32 = input.iter().map(|x| (x - mean).powi(2)).sum::<f32>() / 4.0;
    let std = (var + 1e-5_f32).sqrt();
    let expected: Vec<f32> = input.iter().map(|x| (x - mean) / std).collect();

    let request = ExecuteCpuRequest {
        wgsl_source: wgsl.into(),
        entry_point: None,
        workgroups: [1, 1, 1],
        bindings: vec![rw_f32_binding(0, 0, &input)],
        uniforms: vec![],
        strategy: coral_reef_cpu::types::ExecutionStrategy::Interpret,
    };
    let resp =
        coral_reef_cpu::execute_coral_ir(&request).expect("barracuda_layer_norm: CoralIR interp");
    let result = read_f32s(&resp.bindings[0].data);
    for (i, (&got, &exp)) in result.iter().zip(expected.iter()).enumerate() {
        assert_f32_close(got, exp, &format!("layer_norm[{i}]"));
    }
}

#[test]
fn barracuda_tiled_matmul_2x2() {
    let wgsl = r"
var<workgroup> tile_a: array<f32, 4>;
var<workgroup> tile_b: array<f32, 4>;

@group(0) @binding(0) var<storage, read> a: array<f32>;
@group(0) @binding(1) var<storage, read> b: array<f32>;
@group(0) @binding(2) var<storage, read_write> c: array<f32>;

@compute @workgroup_size(2, 2)
fn main(@builtin(local_invocation_id) lid: vec3<u32>) {
    let row = lid.y;
    let col = lid.x;
    let linear = row * 2u + col;

    // Load A and B tiles into shared memory
    tile_a[linear] = a[linear];
    tile_b[linear] = b[linear];
    workgroupBarrier();

    // Compute C[row][col] = sum_k A[row][k] * B[k][col]
    var sum = 0.0;
    for (var k = 0u; k < 2u; k = k + 1u) {
        sum = sum + tile_a[row * 2u + k] * tile_b[k * 2u + col];
    }
    c[linear] = sum;
}
";
    // A = [[1, 2], [3, 4]], B = [[5, 6], [7, 8]]
    // C = [[1*5+2*7, 1*6+2*8], [3*5+4*7, 3*6+4*8]] = [[19, 22], [43, 50]]
    let a_data = vec![1.0_f32, 2.0, 3.0, 4.0];
    let b_data = vec![5.0_f32, 6.0, 7.0, 8.0];
    let expected = vec![19.0_f32, 22.0, 43.0, 50.0];

    let request = ExecuteCpuRequest {
        wgsl_source: wgsl.into(),
        entry_point: None,
        workgroups: [1, 1, 1],
        bindings: vec![
            BindingData {
                group: 0,
                binding: 0,
                data: f32_bytes(&a_data),
                usage: BindingUsage::ReadOnly,
            },
            BindingData {
                group: 0,
                binding: 1,
                data: f32_bytes(&b_data),
                usage: BindingUsage::ReadOnly,
            },
            rw_zero_bytes(0, 2, 16),
        ],
        uniforms: vec![],
        strategy: coral_reef_cpu::types::ExecutionStrategy::Interpret,
    };
    let resp =
        coral_reef_cpu::execute_coral_ir(&request).expect("barracuda_tiled_matmul: CoralIR interp");
    let result = read_f32s(&resp.bindings[2].data);
    for (i, (&got, &exp)) in result.iter().zip(expected.iter()).enumerate() {
        assert_f32_close(got, exp, &format!("tiled_matmul[{i}]"));
    }
}
