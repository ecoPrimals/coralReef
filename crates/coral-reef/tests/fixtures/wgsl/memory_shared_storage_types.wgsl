// SPDX-License-Identifier: AGPL-3.0-or-later
// Memory: workgroup shared + storage buffers with i32/f32/u32
// Exercises: shared memory layout, barrier, typed loads/stores

var<workgroup> shared_f32: array<f32, 64>;
var<workgroup> shared_u32: array<u32, 64>;

@group(0) @binding(0) var<storage, read> input_f32: array<f32>;
@group(0) @binding(1) var<storage, read> input_u32: array<u32>;
@group(0) @binding(2) var<storage, read_write> output: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(local_invocation_id) lid: vec3<u32>) {
    shared_f32[lid.x] = input_f32[lid.x];
    shared_u32[lid.x] = input_u32[lid.x];
    workgroupBarrier();
    let rev = 63u - lid.x;
    output[lid.x] = f32(shared_u32[rev]) + shared_f32[rev];
}
