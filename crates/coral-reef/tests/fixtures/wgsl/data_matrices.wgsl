// SPDX-License-Identifier: AGPL-3.0-or-later
// Data types: mat2x2, mat3x3, mat4x4
// Exercises: matrix loads, matrix multiply, vector-matrix
// Uses local_invocation_id for AMD compatibility (no SR_NTID)

@group(0) @binding(0) var<storage, read_write> out: array<vec4<f32>>;
@group(0) @binding(1) var<storage, read> m2: array<mat2x2<f32>>;
@group(0) @binding(2) var<storage, read> m3: array<mat3x3<f32>>;
@group(0) @binding(3) var<storage, read> m4: array<mat4x4<f32>>;
@group(0) @binding(4) var<storage, read> v4: array<vec4<f32>>;

@compute @workgroup_size(64)
fn main(@builtin(local_invocation_id) lid: vec3<u32>) {
    let idx = lid.x;
    let a2 = m2[idx];
    let a3 = m3[idx];
    let a4 = m4[idx];
    let v = v4[idx];
    let r2 = a2 * vec2<f32>(1.0, 0.0);
    let r3 = a3 * vec3<f32>(1.0, 0.0, 0.0);
    let r4 = a4 * v;
    out[idx] = vec4<f32>(r2.x, r2.y, r3.x, r4.w);
}
