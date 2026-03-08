// SPDX-License-Identifier: AGPL-3.0-only

struct Params {
    n_pairs: u32,
    pad0: u32,
    pad1: u32,
    pad2: u32,
}

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> a: array<f64>;
@group(0) @binding(2) var<storage, read> b: array<f64>;
@group(0) @binding(3) var<storage, read_write> out: array<f64>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>, @builtin(num_workgroups) nwg: vec3<u32>) {
    let idx = gid.x + gid.y * nwg.x * 64u;
    let i = idx;
    if i >= params.n_pairs { return; }
    out[i] = a[i * 2u] * b[i * 2u] + a[i * 2u + 1u] * b[i * 2u + 1u];
}
