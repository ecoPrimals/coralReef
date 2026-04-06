// SPDX-License-Identifier: AGPL-3.0-or-later
// func_math: countOneBits, countLeadingZeros, reverseBits, firstLeadingBit
// Exercises: OpPopC, OpBRev, OpFlo

@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@group(0) @binding(1) var<storage, read> input: array<u32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let idx = gid.x;
    let v = input[idx];

    let pop = countOneBits(v);
    let clz = countLeadingZeros(v);
    let rev = reverseBits(v);
    let flb = firstLeadingBit(v);

    out[idx] = f32(pop) + f32(clz) * 0.01 + f32(rev) * 0.0001 + f32(flb) * 0.000001;
}
