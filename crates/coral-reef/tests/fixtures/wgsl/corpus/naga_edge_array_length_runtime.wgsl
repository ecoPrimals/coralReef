// SPDX-License-Identifier: AGPL-3.0-only
// Exercises: naga_translate `translate_array_length` / `Expression::ArrayLength` on a
//            runtime-sized `array<u32>` in the storage address space.

@group(0) @binding(0) var<storage, read> data: array<u32>;
@group(0) @binding(1) var<storage, read_write> out: array<u32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let n = arrayLength(&data);
    out[gid.x] = n + gid.x;
}
