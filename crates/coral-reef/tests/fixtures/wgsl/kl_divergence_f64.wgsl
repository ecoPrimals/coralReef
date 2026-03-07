// SPDX-License-Identifier: AGPL-3.0-or-later
//
// KL divergence: sum(p * ln(p / q)) (f64).
// Single-pass fused — replaces CPU log-ratio + GPU sum pipeline.
// Input distributions are assumed pre-normalized.
//
// Binding layout:
//   0: uniform { n: u32, pad: u32 }
//   1: storage, read — p: array<f64>
//   2: storage, read — q: array<f64>
//   3: storage, read_write — partials: array<f64> (one per workgroup)

struct Params {
    n: u32,
    pad0: u32,
}

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> p: array<f64>;
@group(0) @binding(2) var<storage, read> q: array<f64>;
@group(0) @binding(3) var<storage, read_write> partials: array<f64>;

var<workgroup> shared: array<f64, 256>;

@compute @workgroup_size(256)
fn main(
    @builtin(global_invocation_id) gid: vec3<u32>,
    @builtin(local_invocation_id) lid: vec3<u32>,
    @builtin(workgroup_id) wgid: vec3<u32>,
) {
    let idx = gid.x;
    var val = f64(0.0);

    if (idx < params.n) {
        let guard = f64(1e-30);
        let pi = max(p[idx], guard);
        let qi = max(q[idx], guard);
        val = pi * log(pi / qi);
    }

    shared[lid.x] = val;
    workgroupBarrier();

    for (var stride = 128u; stride > 0u; stride >>= 1u) {
        if (lid.x < stride) {
            shared[lid.x] += shared[lid.x + stride];
        }
        workgroupBarrier();
    }

    if (lid.x == 0u) {
        partials[wgid.x] = shared[0];
    }
}
