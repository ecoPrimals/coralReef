// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Pairwise Jaccard Distance — GPU Parallel Binary Matrix Analysis
//
// Computes the upper-triangle Jaccard distance matrix for a pangenome
// presence/absence (PA) matrix.  Each thread handles one genome pair (i,j)
// and accumulates intersection/union counts from binary gene vectors.
//
// Jaccard(i,j) = 1 - |intersection(i,j)| / |union(i,j)|
//
// The PA matrix is stored column-major: pa[gene * n_genomes + genome].
//
// Absorption target: barracuda::ops::pairwise_distance or batch_elementwise
// Validates against: neuralSpring Paper 024 (Anderson — Pangenome Selection)

// PA matrix: pa[gene * n_genomes + genome] ∈ {0.0, 1.0}
@group(0) @binding(0) var<storage, read> pa: array<f32>;

// Output distances: upper-triangle, pair_index = i*(2*N-i-1)/2 + (j-i-1)
@group(0) @binding(1) var<storage, read_write> distances: array<f32>;

struct JaccardParams {
    n_genomes: u32,
    n_genes: u32,
}
@group(0) @binding(2) var<uniform> params: JaccardParams;

@compute @workgroup_size(256)
fn pairwise_jaccard(@builtin(global_invocation_id) gid: vec3<u32>) {
    let pair_idx = gid.x;
    let n = params.n_genomes;
    let n_pairs = n * (n - 1u) / 2u;
    if pair_idx >= n_pairs {
        return;
    }

    // Decode pair index to (i, j) where i < j.
    // Row i starts at cumulative offset i*(2N-i-1)/2.
    var i: u32 = 0u;
    var remaining = pair_idx;
    loop {
        let row_len = n - 1u - i;
        if remaining < row_len {
            break;
        }
        remaining = remaining - row_len;
        i = i + 1u;
    }
    let j = i + 1u + remaining;

    var intersection: f32 = 0.0;
    var union_count: f32 = 0.0;
    for (var g: u32 = 0u; g < params.n_genes; g = g + 1u) {
        let a = pa[g * n + i];
        let b = pa[g * n + j];
        intersection = intersection + a * b;
        union_count = union_count + max(a, b);
    }

    var dist: f32 = 1.0;
    if union_count > 0.0 {
        dist = 1.0 - intersection / union_count;
    }
    distances[pair_idx] = dist;
}
