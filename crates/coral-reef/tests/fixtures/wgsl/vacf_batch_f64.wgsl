// SPDX-License-Identifier: AGPL-3.0-only
//
// Batched VACF — Compute C(lag) for one lag across all time origins.
//
// Each thread handles one particle: iterates over all valid time origins
// t0 = 0..n_frames-lag, accumulating v(t0) · v(t0+lag). Output is the
// per-particle sum across all origins, then reduced to one scalar.
//
// This replaces n_frames individual dispatches per lag with ONE dispatch.
// n_lag reductions total instead of n_frames × n_lag.
//
// Bindings:
//   0: vel_ring  [n_frames * N * 3]  f64, read  — flat ring of velocity snapshots
//   1: out       [N]                 f64, write — per-particle accumulated dot product
//   2: params    uniform             — { n: u32, n_frames: u32, lag: u32, stride: u32 }
//
// vel_ring layout: snapshot s, particle i, component d → vel_ring[s * stride + i * 3 + d]
// where stride = N * 3.

struct Params {
    n: u32,
    n_frames: u32,
    lag: u32,
    stride: u32,  // N * 3
}

@group(0) @binding(0) var<storage, read> vel_ring: array<f64>;
@group(0) @binding(1) var<storage, read_write> out: array<f64>;
@group(0) @binding(2) var<uniform> params: Params;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if (i >= params.n) { return; }

    let base_i = i * 3u;
    let n_origins = params.n_frames - params.lag;

    var acc = vel_ring[0] - vel_ring[0]; // 0.0 as f64

    for (var t0 = 0u; t0 < n_origins; t0 = t0 + 1u) {
        let off0 = t0 * params.stride + base_i;
        let off1 = (t0 + params.lag) * params.stride + base_i;

        acc += vel_ring[off0]      * vel_ring[off1]
             + vel_ring[off0 + 1u] * vel_ring[off1 + 1u]
             + vel_ring[off0 + 2u] * vel_ring[off1 + 2u];
    }

    out[i] = acc;
}
