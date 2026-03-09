// SPDX-License-Identifier: AGPL-3.0-or-later
//
// softmax_f64.wgsl — Row-wise softmax with df64 core streaming
//
// Pass 2 of 3-pass f64 SDPA. Also usable standalone for any row-wise softmax.
// Three-zone core streaming: f64 buffer I/O, df64 compute, f64 output.
//
// Algorithm (numerically stable):
//   1. Find row max (for numerical stability)
//   2. Compute exp_df64(x_i - max) with df64 sum accumulation
//   3. Normalize: out_i = exp(x_i - max) / sum (all df64)
//
// Each workgroup processes one row. Row length = params.row_len.
// Total rows = params.num_rows.
//
// Absorption target: barracuda::ops::softmax_f64
// Requires: df64_core.wgsl + df64_transcendentals.wgsl (prepended via compile_shader_f64)

struct Params {
    num_rows: u32,
    row_len:  u32,
    _pad0:    u32,
    _pad1:    u32,
}

@group(0) @binding(0) var<storage, read>       input:  array<f64>;
@group(0) @binding(1) var<storage, read_write> output: array<f64>;
@group(0) @binding(2) var<uniform>             params: Params;

var<workgroup> shared_vals: array<f32, 256>;
var<workgroup> shared_lo:   array<f32, 256>;

@compute @workgroup_size(256)
fn main(@builtin(global_invocation_id) gid: vec3<u32>,
        @builtin(local_invocation_id)  lid: vec3<u32>,
        @builtin(workgroup_id)         wid: vec3<u32>) {
    let row = wid.x;
    let tid = lid.x;
    let len = params.row_len;

    if row >= params.num_rows { return; }
    let base = row * len;

    // Phase 1: Find row maximum (f32 cast sufficient for overflow prevention)
    var local_max = -1e38f;
    var i = tid;
    while i < len {
        local_max = max(local_max, f32(input[base + i]));
        i += 256u;
    }
    shared_vals[tid] = local_max;
    workgroupBarrier();

    var stride = 128u;
    while stride > 0u {
        if tid < stride {
            shared_vals[tid] = max(shared_vals[tid], shared_vals[tid + stride]);
        }
        workgroupBarrier();
        stride >>= 1u;
    }
    let row_max = shared_vals[0];
    workgroupBarrier();

    // Phase 2: exp_df64(x - max) with df64 sum (Zone 1+2: f64 → df64 compute)
    let max_df = df64_from_f32(row_max);
    var acc = df64_zero();
    i = tid;
    while i < len {
        let x = df64_from_f64(input[base + i]);
        let shifted = df64_sub(x, max_df);
        let e = exp_df64(shifted);
        output[base + i] = df64_to_f64(e);
        acc = df64_add(acc, e);
        i += 256u;
    }
    shared_vals[tid] = acc.hi;
    shared_lo[tid] = acc.lo;
    workgroupBarrier();

    stride = 128u;
    while stride > 0u {
        if tid < stride {
            let r = df64_add(
                Df64(shared_vals[tid], shared_lo[tid]),
                Df64(shared_vals[tid + stride], shared_lo[tid + stride]),
            );
            shared_vals[tid] = r.hi;
            shared_lo[tid] = r.lo;
        }
        workgroupBarrier();
        stride >>= 1u;
    }
    let sum_df = Df64(shared_vals[0], shared_lo[0]);
    workgroupBarrier();

    // Phase 3: Normalize (Zone 2+3: df64 divide → f64 store)
    i = tid;
    while i < len {
        let e = df64_from_f64(output[base + i]);
        let normalized = df64_div(e, sum_df);
        output[base + i] = df64_to_f64(normalized);
        i += 256u;
    }
}
