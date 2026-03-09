// SPDX-License-Identifier: AGPL-3.0-only
//
// VACF Dot Product (f64) — Per-particle v(t0) · v(t)
//
// For GPU-resident transport: computes per-particle dot product between
// a reference velocity snapshot and the current velocity buffer.
// Output is reduced via ReduceScalarPipeline to get C(lag) = <v(0)·v(t)>.
//
// Bindings:
//   0: v_ref    [N*3] f64, read  — reference velocity snapshot v(t0)
//   1: v_cur    [N*3] f64, read  — current velocity v(t0+lag)
//   2: out      [N]   f64, write — per-particle dot product
//   3: params   uniform          — { n: u32, pad: u32 }

struct Params {
    n: u32,
    pad0: u32,
}

@group(0) @binding(0) var<storage, read> v_ref: array<f64>;
@group(0) @binding(1) var<storage, read> v_cur: array<f64>;
@group(0) @binding(2) var<storage, read_write> out: array<f64>;
@group(0) @binding(3) var<uniform> params: Params;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if (i >= params.n) { return; }

    let base = i * 3u;
    let vx0 = v_ref[base];
    let vy0 = v_ref[base + 1u];
    let vz0 = v_ref[base + 2u];

    let vx1 = v_cur[base];
    let vy1 = v_cur[base + 1u];
    let vz1 = v_cur[base + 2u];

    out[i] = vx0 * vx1 + vy0 * vy1 + vz0 * vz1;
}
