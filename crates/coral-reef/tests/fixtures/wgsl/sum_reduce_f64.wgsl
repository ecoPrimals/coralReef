// SPDX-License-Identifier: AGPL-3.0-only
//
// Tree reduction: sum N f64 values down to ceil(N/256) partial sums.
// Dispatch twice for full scalar output:
//   Pass 1:  input[N]          → output[ceil(N/256)]
//   Pass 2:  output[ceil(N/256)] → final[1]
//
// Ported from toadStool sum_reduce_f64.wgsl for hotSpring lattice QCD.

struct ReduceParams {
    size: u32,
    _pad1: u32,
    _pad2: u32,
    _pad3: u32,
}

@group(0) @binding(0) var<storage, read> input: array<f64>;
@group(0) @binding(1) var<storage, read_write> output: array<f64>;
@group(0) @binding(2) var<uniform> params: ReduceParams;

var<workgroup> shared_data: array<f64, 256>;

@compute @workgroup_size(256)
fn main(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(local_invocation_id) local_id: vec3<u32>,
    @builtin(workgroup_id) workgroup_id: vec3<u32>,
    @builtin(num_workgroups) nwg: vec3<u32>,
) {
    let tid = local_id.x;
    let gid = global_id.x + global_id.y * nwg.x * 256u;

    if (gid < params.size) {
        shared_data[tid] = input[gid];
    } else {
        shared_data[tid] = f64(0.0);
    }
    workgroupBarrier();

    for (var stride = 128u; stride > 0u; stride = stride >> 1u) {
        if (tid < stride) {
            shared_data[tid] = shared_data[tid] + shared_data[tid + stride];
        }
        workgroupBarrier();
    }

    if (tid == 0u) {
        let wg_linear = workgroup_id.x + workgroup_id.y * nwg.x;
        output[wg_linear] = shared_data[0];
    }
}
