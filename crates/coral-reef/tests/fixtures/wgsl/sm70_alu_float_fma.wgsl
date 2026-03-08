// SPDX-License-Identifier: AGPL-3.0-only
// sm70_encode/alu: FMA, float min/max, transcendental chains
// Exercises: OpFFma, OpFMnMx, MUFU, float ALU encoding

@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@group(0) @binding(1) var<storage, read> a: array<f32>;
@group(0) @binding(2) var<storage, read> b: array<f32>;
@group(0) @binding(3) var<storage, read> c: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let idx = gid.x;
    let x = a[idx];
    let y = b[idx];
    let z = c[idx];

    let fma_val = x * y + z;
    let fma2 = fma_val * 2.0 + x;
    let mn = min(x, y);
    let mx = max(x, y);
    let cl = clamp(z, 0.0, 1.0);

    let sq = sqrt(max(x, 0.01));
    let rsq = inverseSqrt(max(y, 0.01));
    let ex = exp(x * 0.01);
    let lg = log(max(z, 0.01));

    out[idx] = fma_val + fma2 * 0.1 + mn + mx * 0.1 + cl * 0.01 + sq + rsq * 0.1 + ex * 0.01 + lg * 0.001;
}
