// SPDX-License-Identifier: AGPL-3.0-or-later
//
// layer_norm_f64.wgsl — Layer Normalization with df64 core streaming
//
// LayerNorm(x) = gamma * (x - mean) / sqrt(var + eps) + beta
//
// Three-zone core streaming: f64 buffer I/O, df64 compute, f64 output.
// df64 accumulation for mean/variance prevents catastrophic cancellation
// at large hidden dimensions.
//
// Cross-spring: benefits baseCamp (Sub-02 attention), WDM surrogates,
// coralForge (Evoformer), all transformer architectures.
//
// Absorption target: barracuda::ops::layer_norm_f64
// Requires: df64_core.wgsl + df64_transcendentals.wgsl (prepended via compile_shader_f64)

struct Params {
    seq_len: u32,
    hidden_dim: u32,
    eps_hi: f32,
    eps_lo: f32,
}

@group(0) @binding(0) var<storage, read> input: array<f64>;
@group(0) @binding(1) var<storage, read> gamma: array<f64>;
@group(0) @binding(2) var<storage, read> beta: array<f64>;
@group(0) @binding(3) var<storage, read_write> output: array<f64>;
@group(0) @binding(4) var<uniform> params: Params;

var<workgroup> shared_sum_hi: array<f32, 256>;
var<workgroup> shared_sum_lo: array<f32, 256>;

@compute @workgroup_size(256)
fn layer_norm(@builtin(global_invocation_id) gid: vec3<u32>,
              @builtin(local_invocation_id) lid: vec3<u32>,
              @builtin(workgroup_id) wid: vec3<u32>) {
    let row = wid.x;
    let tid = lid.x;
    let dim = params.hidden_dim;

    if row >= params.seq_len { return; }

    let base = row * dim;

    // Phase 1: Compute mean via df64 reduction (Zone 1+2: f64 → df64)
    var acc = df64_zero();
    var i = tid;
    while i < dim {
        acc = df64_add(acc, df64_from_f64(input[base + i]));
        i += 256u;
    }
    shared_sum_hi[tid] = acc.hi;
    shared_sum_lo[tid] = acc.lo;
    workgroupBarrier();

    var stride = 128u;
    while stride > 0u {
        if tid < stride {
            let r = df64_add(
                Df64(shared_sum_hi[tid], shared_sum_lo[tid]),
                Df64(shared_sum_hi[tid + stride], shared_sum_lo[tid + stride]),
            );
            shared_sum_hi[tid] = r.hi;
            shared_sum_lo[tid] = r.lo;
        }
        workgroupBarrier();
        stride >>= 1u;
    }

    let sum_df = Df64(shared_sum_hi[0], shared_sum_lo[0]);
    let dim_df = df64_from_f32(f32(dim));
    let mean_df = df64_div(sum_df, dim_df);
    workgroupBarrier();

    // Phase 2: Compute variance via df64 (Zone 1+2: f64 → df64)
    acc = df64_zero();
    i = tid;
    while i < dim {
        let val = df64_from_f64(input[base + i]);
        let diff = df64_sub(val, mean_df);
        let sq = df64_mul(diff, diff);
        acc = df64_add(acc, sq);
        i += 256u;
    }
    shared_sum_hi[tid] = acc.hi;
    shared_sum_lo[tid] = acc.lo;
    workgroupBarrier();

    stride = 128u;
    while stride > 0u {
        if tid < stride {
            let r = df64_add(
                Df64(shared_sum_hi[tid], shared_sum_lo[tid]),
                Df64(shared_sum_hi[tid + stride], shared_sum_lo[tid + stride]),
            );
            shared_sum_hi[tid] = r.hi;
            shared_sum_lo[tid] = r.lo;
        }
        workgroupBarrier();
        stride >>= 1u;
    }

    let var_sum = Df64(shared_sum_hi[0], shared_sum_lo[0]);
    let variance = df64_div(var_sum, dim_df);
    let eps = Df64(params.eps_hi, params.eps_lo);
    let inv_std = df64_div(df64_from_f32(1.0), sqrt_df64(df64_add(variance, eps)));
    workgroupBarrier();

    // Phase 3: Normalize + scale + shift (Zone 2+3: df64 → f64)
    i = tid;
    while i < dim {
        let val = df64_from_f64(input[base + i]);
        let normalized = df64_mul(df64_sub(val, mean_df), inv_std);
        let g = df64_from_f64(gamma[i]);
        let b = df64_from_f64(beta[i]);
        let result = df64_add(df64_mul(g, normalized), b);
        output[base + i] = df64_to_f64(result);
        i += 256u;
    }
}
