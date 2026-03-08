// SPDX-License-Identifier: AGPL-3.0-only
// Math: fract (x-floor), ceil, floor, round, sign, abs
// Exercises: FRnd modes, naga_translate func_math

@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@group(0) @binding(1) var<storage, read> input: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(local_invocation_id) lid: vec3<u32>) {
    let x = input[lid.x];
    let fl = floor(x);
    let f = x - fl;
    let c = ceil(x);
    let r = round(x);
    let ab = abs(-x);
    out[lid.x] = f + c * 0.1 + fl * 0.01 + r * 0.001 + ab;
}
