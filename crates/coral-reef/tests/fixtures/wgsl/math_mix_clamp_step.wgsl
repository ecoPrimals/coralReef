// SPDX-License-Identifier: AGPL-3.0-or-later
// Math: clamp, min, max, select (mix/step/smoothstep use unsupported builtins)
// Exercises: naga_translate func_math, FMnMx, select chains

@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@group(0) @binding(1) var<storage, read> a: array<f32>;
@group(0) @binding(2) var<storage, read> b: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(local_invocation_id) lid: vec3<u32>) {
    let x = a[lid.x];
    let y = b[lid.x];
    let mixed = x + (y - x) * 0.5;
    let clamped = clamp(mixed, 0.0, 1.0);
    let stepped = select(0.0, 1.0, clamped >= 0.5);
    let t = clamp(x, 0.0, 1.0);
    let smoothed = t * t * (2.0 - t);
    out[lid.x] = stepped + smoothed;
}
