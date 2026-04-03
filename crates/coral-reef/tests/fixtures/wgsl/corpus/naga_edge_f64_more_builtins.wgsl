// SPDX-License-Identifier: AGPL-3.0-only
// Exercises: f64 expression lowering — `fma`, rounding (`ceil`/`floor`/`trunc`/`fract`),
//            `sign`, `abs`, `min`/`max`, `clamp`, and transcendental pairs (`sin`/`cos`)
//            for additional df64 / ALU coverage (`degrees`/`radians` are not lowered yet).

enable f64;

@group(0) @binding(0) var<storage, read_write> out: array<f64>;
@group(0) @binding(1) var<storage, read> inp: array<f64>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let x = inp[gid.x];
    let y = inp[gid.x + 1u];
    let p = fma(x, y, 1.0);
    let q = ceil(x) + floor(y) + trunc(x) + fract(x);
    let r = sin(x) + cos(y);
    let s = sign(x) * abs(y);
    let t = min(x, y) + max(x, y);
    let u = clamp(x, 0.0, 1.0);
    out[gid.x] = p + q + r + s + t + u;
}
