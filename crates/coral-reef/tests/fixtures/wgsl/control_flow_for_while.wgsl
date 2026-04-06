// SPDX-License-Identifier: AGPL-3.0-or-later
// Control flow: loop/break (matches coverage_nested_loops pattern)
// Exercises: loop lowering, back edges, liveness

@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@group(0) @binding(1) var<storage, read> input: array<f32>;

@compute @workgroup_size(1)
fn main() {
    var sum: f32 = 0.0;
    var i: u32 = 0u;
    loop {
        if i >= 8u { break; }
        sum = sum + input[i];
        i = i + 1u;
    }
    out[0] = sum;
}
