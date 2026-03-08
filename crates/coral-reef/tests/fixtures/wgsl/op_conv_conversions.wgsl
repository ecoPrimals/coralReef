// SPDX-License-Identifier: AGPL-3.0-only
// op_conv: f32<->i32, f32<->u32, bitcast
// Exercises: OpF2I, OpI2F, OpF2U, OpU2F, OpB32

@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@group(0) @binding(1) var<storage, read> input_f: array<f32>;
@group(0) @binding(2) var<storage, read> input_u: array<u32>;
@group(0) @binding(3) var<storage, read> input_i: array<i32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let idx = gid.x;
    let f = input_f[idx];
    let u = input_u[idx];
    let i = input_i[idx];

    let f_to_i = i32(f);
    let f_to_u = u32(f);
    let i_to_f = f32(i);
    let u_to_f = f32(u);

    let bc_f_to_u = bitcast<u32>(f);
    let bc_u_to_f = bitcast<f32>(u);

    out[idx] = f32(f_to_i) * 0.0001 + u_to_f * 0.001 + i_to_f * 0.01 + f + bitcast<f32>(bc_f_to_u) * 0.00001;
}
