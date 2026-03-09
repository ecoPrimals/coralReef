// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Per-Locus Allele Frequency Variance — GPU Parallel FST Decomposition
//
// Computes the variance of allele frequencies across populations for each
// locus independently.  Each thread handles one locus.  This is the core
// building block for Weir-Cockerham FST estimation.
//
// Input: allele_freqs[pop * n_loci + locus] — frequency of allele 1 at
//        each locus in each population.
//
// Output: per_locus_var[locus] — population variance of AF across pops.
//
// Absorption target: barracuda::ops::VarianceReduceF64 (per-row)
// Validates against: neuralSpring Paper 025 (Anderson — Meta-Population)

// Allele frequencies: af[pop * n_loci + locus]
@group(0) @binding(0) var<storage, read> allele_freqs: array<f32>;

// Output variance per locus
@group(0) @binding(1) var<storage, read_write> per_locus_var: array<f32>;

struct VarianceParams {
    n_pops: u32,
    n_loci: u32,
}
@group(0) @binding(2) var<uniform> params: VarianceParams;

@compute @workgroup_size(256)
fn locus_variance(@builtin(global_invocation_id) gid: vec3<u32>) {
    let locus = gid.x;
    if locus >= params.n_loci {
        return;
    }

    // Two-pass: mean then variance (numerically stable for small n_pops).
    var sum: f32 = 0.0;
    for (var p: u32 = 0u; p < params.n_pops; p = p + 1u) {
        sum = sum + allele_freqs[p * params.n_loci + locus];
    }
    let mean = sum / f32(params.n_pops);

    var var_sum: f32 = 0.0;
    for (var p: u32 = 0u; p < params.n_pops; p = p + 1u) {
        let diff = allele_freqs[p * params.n_loci + locus] - mean;
        var_sum = fma(diff, diff, var_sum);
    }

    // Population variance (ddof=0) — matches hand-rolled Rust FST formula.
    per_locus_var[locus] = var_sum / f32(params.n_pops);
}
