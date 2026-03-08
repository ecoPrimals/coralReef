// SPDX-License-Identifier: AGPL-3.0-only
// Memory: atomic add (u32 and i32)
// Exercises: atomic code paths in naga_translate and memory lowering

@group(0) @binding(0) var<storage, read_write> counter_u: atomic<u32>;
@group(0) @binding(1) var<storage, read_write> out: array<u32>;
@group(0) @binding(2) var<storage, read_write> counter_i: atomic<i32>;

@compute @workgroup_size(64)
fn main(@builtin(local_invocation_id) lid: vec3<u32>) {
    let old_add = atomicAdd(&counter_u, 1u);
    let i32_old = atomicAdd(&counter_i, 1);
    out[lid.x] = old_add + u32(i32_old);
}
