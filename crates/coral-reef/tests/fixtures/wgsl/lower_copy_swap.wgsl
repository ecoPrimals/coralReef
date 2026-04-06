// SPDX-License-Identifier: AGPL-3.0-or-later
// lower_copy_swap: copy/swap patterns, phi nodes with multiple predecessors
// Exercises: value swapping, copy elimination, SSA repair

@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@group(0) @binding(1) var<storage, read> input: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    var a = input[gid.x];
    var b = input[(gid.x + 1u) % 64u];

    var i: u32 = 0u;
    loop {
        if i >= 8u { break; }
        let tmp = a;
        a = b;
        b = tmp + f32(i);
        i = i + 1u;
    }

    var j: u32 = 0u;
    loop {
        if j >= 4u { break; }
        if a > b {
            let t = a;
            a = b;
            b = t;
        }
        j = j + 1u;
    }

    out[gid.x] = a + b;
}
