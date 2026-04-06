// SPDX-License-Identifier: AGPL-3.0-or-later
// sm70_encode/control: branches, loops, barriers
// Exercises: BRA, BRX, SSY/SYNC, back edges, merge blocks

var<workgroup> wg_data: array<f32, 128>;

@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@group(0) @binding(1) var<storage, read> input: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>, @builtin(local_invocation_id) lid: vec3<u32>) {
    wg_data[lid.x] = input[gid.x];
    workgroupBarrier();

    var acc: f32 = 0.0;
    var i: u32 = 0u;
    loop {
        if i >= 8u { break; }
        if i % 2u == 0u {
            acc = acc + wg_data[i * 8u + lid.x];
        } else {
            acc = acc - wg_data[i * 8u + lid.x] * 0.5;
        }
        i = i + 1u;
    }

    var j: u32 = 0u;
    loop {
        if j >= 4u { break; }
        if acc > 0.0 {
            acc = acc * 0.9;
        } else {
            acc = acc * 1.1;
        }
        j = j + 1u;
    }

    workgroupBarrier();
    out[gid.x] = acc + wg_data[63u - lid.x];
}
