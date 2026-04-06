// SPDX-License-Identifier: AGPL-3.0-or-later
// Data types: vec2, vec3, vec4 operations (manual dot expansion)
// Exercises: vector component access, swizzles, vector math

@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@group(0) @binding(1) var<storage, read> v2_in: array<vec2<f32>>;
@group(0) @binding(2) var<storage, read> v3_in: array<vec3<f32>>;
@group(0) @binding(3) var<storage, read> v4_in: array<vec4<f32>>;

@compute @workgroup_size(64)
fn main(@builtin(local_invocation_id) lid: vec3<u32>) {
    let idx = lid.x;
    let a2 = v2_in[idx];
    let a3 = v3_in[idx];
    let a4 = v4_in[idx];
    let dot2 = a2.x * a2.y;
    let dot3 = a3.x * a3.x + a3.y * a3.y + a3.z * a3.z;
    let dot4 = a4.x * a4.x + a4.y * a4.y + a4.z * a4.z + a4.w * a4.w;
    let sum = dot2 + dot3 + dot4;
    let swizzle = a4.z + a3.y;
    out[idx] = sum + swizzle;
}
