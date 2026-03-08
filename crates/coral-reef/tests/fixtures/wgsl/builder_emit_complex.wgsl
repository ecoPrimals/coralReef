// SPDX-License-Identifier: AGPL-3.0-only
// builder/emit: complex shader with many instruction types
// Exercises: ALU, memory, control, conversions, transcendentals

@group(0) @binding(0) var<storage, read_write> out: array<f32>;
@group(0) @binding(1) var<storage, read> a: array<f32>;
@group(0) @binding(2) var<storage, read> b: array<f32>;
@group(0) @binding(3) var<storage, read> c: array<u32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let idx = gid.x;
    let x = a[idx];
    let y = b[idx];
    let z = f32(c[idx]);

    var v: f32 = x * y + z;
    v = v - sqrt(max(x, 0.01));
    v = v * inverseSqrt(max(y, 0.01));
    v = v + log(max(x + 1.0, 0.01)) * 0.1;
    v = v + exp(x * 0.01) * 0.01;
    v = clamp(v, -100.0, 100.0);

    let ui = u32(v);
    let back = f32(ui);
    v = v * 0.5 + back * 0.5;

    if idx % 4u == 0u {
        v = v + floor(x) + ceil(y) + round(z);
    }

    var i: u32 = 0u;
    loop {
        if i >= 4u { break; }
        v = v + f32(i) * 0.1;
        i = i + 1u;
    }

    out[idx] = v;
}
