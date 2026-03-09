// SPDX-License-Identifier: AGPL-3.0-only
//
// DEPRECATED — Use barracuda::ops::md::observables::Rdf
// Retained for fossil record. barracuda canonical: ops/md/observables/rdf_histogram_f64.wgsl
//
// RDF pair distance histogram (f64 positions, u32 bins)
//
// Computes all-pairs distances with PBC and bins into a histogram.
// Uses atomicAdd on u32 bins, then normalized on CPU.
//
// Bindings:
//   0: positions   [N*3] f64, read
//   1: histogram   [n_bins] atomic<u32>, read-write
//   2: params      [8]   f64, read  — [n, n_bins, dr, _, box_x, box_y, box_z, _]

@group(0) @binding(0) var<storage, read> positions: array<f64>;
@group(0) @binding(1) var<storage, read_write> histogram: array<atomic<u32>>;
@group(0) @binding(2) var<storage, read> params: array<f64>;

fn pbc_delta_rdf(delta: f64, box_size: f64) -> f64 {
    return delta - box_size * round(delta / box_size);
}

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    let n      = u32(params[0]);
    let n_bins = u32(params[1]);
    let dr     = params[2];
    let box_x  = params[4];
    let box_y  = params[5];
    let box_z  = params[6];

    if (i >= n) { return; }

    let xi = positions[i * 3u];
    let yi = positions[i * 3u + 1u];
    let zi = positions[i * 3u + 2u];

    // Only count pairs where j > i to avoid double-counting
    for (var j = i + 1u; j < n; j = j + 1u) {
        let dx = pbc_delta_rdf(positions[j * 3u]      - xi, box_x);
        let dy = pbc_delta_rdf(positions[j * 3u + 1u] - yi, box_y);
        let dz = pbc_delta_rdf(positions[j * 3u + 2u] - zi, box_z);

        let r = sqrt(dx * dx + dy * dy + dz * dz);
        let bin = u32(r / dr);

        if (bin < n_bins) {
            atomicAdd(&histogram[bin], 1u);
        }
    }
}
