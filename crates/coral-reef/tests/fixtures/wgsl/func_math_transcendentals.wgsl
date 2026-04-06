// SPDX-License-Identifier: AGPL-3.0-or-later
// func_math: pow, exp, log, sqrt, inverseSqrt (f32)
// Exercises: OpTranscendental, log2/exp2 chains

@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@group(0) @binding(1) var<storage, read> input: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let idx = gid.x;
    let x = max(input[idx], 0.001);
    let y = max(input[(idx + 1u) % 64u], 0.001);

    let s = sqrt(x);
    let r = inverseSqrt(x);
    let e = exp(x);
    let l = log(x);
    let p = pow(x, y);

    out[idx] = s + r + e * 0.001 + l * 0.001 + p * 0.0001;
}
