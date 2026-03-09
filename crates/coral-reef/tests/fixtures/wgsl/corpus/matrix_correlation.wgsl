// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Pearson correlation between upper triangles of two N×N matrices.
// Produces a scalar result (via workgroup reduction).
//
// r = cov(a_ut, b_ut) / (std(a_ut) * std(b_ut))
//
// where a_ut, b_ut are the N*(N-1)/2 upper-triangle entries.
//
// Two-pass design:
//   Pass 1 (this shader): each thread accumulates partial sums for one
//     chunk of the upper triangle (sum_a, sum_b, sum_ab, sum_a2, sum_b2, count).
//     Workgroup reduces to per-workgroup partials.
//   Host finalizes the scalar from partials.
//
// Absorption target: barracuda::stats::matrix_correlation
//
// Binding layout:
//   0: storage, read — matrix_a: N*N flat row-major
//   1: storage, read — matrix_b: N*N flat row-major
//   2: storage, read_write — partials: 6 * n_workgroups (sum_a, sum_b, sum_ab, sum_a2, sum_b2, count)
//   3: uniform — params { n, total_pairs }

struct Params {
    n: u32,
    total_pairs: u32,
    _pad0: u32,
    _pad1: u32,
}

@group(0) @binding(0) var<storage, read> matrix_a: array<f32>;
@group(0) @binding(1) var<storage, read> matrix_b: array<f32>;
@group(0) @binding(2) var<storage, read_write> partials: array<f32>;
@group(0) @binding(3) var<uniform> params: Params;

var<workgroup> sh_sum_a:  array<f32, 256>;
var<workgroup> sh_sum_b:  array<f32, 256>;
var<workgroup> sh_sum_ab: array<f32, 256>;
var<workgroup> sh_sum_a2: array<f32, 256>;
var<workgroup> sh_sum_b2: array<f32, 256>;
var<workgroup> sh_count:  array<f32, 256>;

// Map flat pair index → (i, j) in upper triangle of N×N matrix.
fn pair_to_ij(idx: u32, n: u32) -> vec2<u32> {
    var k = idx;
    var row = 0u;
    var remaining = n - 1u;
    while (k >= remaining && remaining > 0u) {
        k -= remaining;
        row++;
        remaining = n - 1u - row;
    }
    return vec2<u32>(row, row + 1u + k);
}

@compute @workgroup_size(256)
fn matrix_correlation(
    @builtin(global_invocation_id) gid: vec3<u32>,
    @builtin(local_invocation_id) lid: vec3<u32>,
    @builtin(workgroup_id) wgid: vec3<u32>,
) {
    let idx = gid.x;
    let n = params.n;
    var a_val = 0.0;
    var b_val = 0.0;
    var valid = 0.0;

    if (idx < params.total_pairs) {
        let ij = pair_to_ij(idx, n);
        let flat = ij.x * n + ij.y;
        a_val = matrix_a[flat];
        b_val = matrix_b[flat];
        valid = 1.0;
    }

    sh_sum_a[lid.x]  = a_val;
    sh_sum_b[lid.x]  = b_val;
    sh_sum_ab[lid.x] = a_val * b_val;
    sh_sum_a2[lid.x] = a_val * a_val;
    sh_sum_b2[lid.x] = b_val * b_val;
    sh_count[lid.x]  = valid;
    workgroupBarrier();

    for (var stride = 128u; stride > 0u; stride >>= 1u) {
        if (lid.x < stride) {
            sh_sum_a[lid.x]  += sh_sum_a[lid.x + stride];
            sh_sum_b[lid.x]  += sh_sum_b[lid.x + stride];
            sh_sum_ab[lid.x] += sh_sum_ab[lid.x + stride];
            sh_sum_a2[lid.x] += sh_sum_a2[lid.x + stride];
            sh_sum_b2[lid.x] += sh_sum_b2[lid.x + stride];
            sh_count[lid.x]  += sh_count[lid.x + stride];
        }
        workgroupBarrier();
    }

    if (lid.x == 0u) {
        let base = wgid.x * 6u;
        partials[base + 0u] = sh_sum_a[0];
        partials[base + 1u] = sh_sum_b[0];
        partials[base + 2u] = sh_sum_ab[0];
        partials[base + 3u] = sh_sum_a2[0];
        partials[base + 4u] = sh_sum_b2[0];
        partials[base + 5u] = sh_count[0];
    }
}
