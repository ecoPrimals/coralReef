// SPDX-License-Identifier: AGPL-3.0-only
//
// CG vector update: p = r + beta * p.
// Beta is read from a GPU buffer (no CPU upload needed).

struct Params {
    n: u32,
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
}

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read_write> p: array<f64>;
@group(0) @binding(2) var<storage, read> r: array<f64>;
@group(0) @binding(3) var<storage, read> beta: array<f64>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>, @builtin(num_workgroups) nwg: vec3<u32>) {
    let idx = gid.x + gid.y * nwg.x * 64u;
    let i = idx;
    if (i >= params.n) { return; }
    let b = beta[0];
    p[i] = r[i] + b * p[i];
}
