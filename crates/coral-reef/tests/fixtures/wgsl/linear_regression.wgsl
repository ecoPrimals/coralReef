// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Simple linear regression via normal equations: y = a*x + b.
// Parallel reduction for the five sufficient statistics (Sx, Sy, Sxx, Sxy, N).
// Host computes final a, b from partials.
//
// a = (N*Sxy - Sx*Sy) / (N*Sxx - Sx*Sx)
// b = (Sxx*Sy - Sx*Sxy) / (N*Sxx - Sx*Sx)
//
// Absorption target: barracuda::stats::linear_regression_gpu
//
// Binding layout:
//   0: storage, read — x: array<f32>
//   1: storage, read — y: array<f32>
//   2: storage, read_write — partials: 5 * n_workgroups (Sx, Sy, Sxx, Sxy, N)
//   3: uniform — params { n }

struct Params {
    n: u32,
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
}

@group(0) @binding(0) var<storage, read> x_data: array<f32>;
@group(0) @binding(1) var<storage, read> y_data: array<f32>;
@group(0) @binding(2) var<storage, read_write> partials: array<f32>;
@group(0) @binding(3) var<uniform> params: Params;

var<workgroup> sh_sx:  array<f32, 256>;
var<workgroup> sh_sy:  array<f32, 256>;
var<workgroup> sh_sxx: array<f32, 256>;
var<workgroup> sh_sxy: array<f32, 256>;
var<workgroup> sh_n:   array<f32, 256>;

@compute @workgroup_size(256)
fn linear_regression(
    @builtin(global_invocation_id) gid: vec3<u32>,
    @builtin(local_invocation_id) lid: vec3<u32>,
    @builtin(workgroup_id) wgid: vec3<u32>,
) {
    let idx = gid.x;
    var xv = 0.0;
    var yv = 0.0;
    var valid = 0.0;

    if (idx < params.n) {
        xv = x_data[idx];
        yv = y_data[idx];
        valid = 1.0;
    }

    sh_sx[lid.x]  = xv;
    sh_sy[lid.x]  = yv;
    sh_sxx[lid.x] = xv * xv;
    sh_sxy[lid.x] = xv * yv;
    sh_n[lid.x]   = valid;
    workgroupBarrier();

    for (var stride = 128u; stride > 0u; stride >>= 1u) {
        if (lid.x < stride) {
            sh_sx[lid.x]  += sh_sx[lid.x + stride];
            sh_sy[lid.x]  += sh_sy[lid.x + stride];
            sh_sxx[lid.x] += sh_sxx[lid.x + stride];
            sh_sxy[lid.x] += sh_sxy[lid.x + stride];
            sh_n[lid.x]   += sh_n[lid.x + stride];
        }
        workgroupBarrier();
    }

    if (lid.x == 0u) {
        let base = wgid.x * 5u;
        partials[base + 0u] = sh_sx[0];
        partials[base + 1u] = sh_sy[0];
        partials[base + 2u] = sh_sxx[0];
        partials[base + 3u] = sh_sxy[0];
        partials[base + 4u] = sh_n[0];
    }
}
