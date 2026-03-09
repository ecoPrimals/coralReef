// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Multi-Objective Fitness — GPU Batch Evaluation
//
// Computes per-objective fitness for a batch of genotypes. Each genotype
// is split into equal chunks (one per objective); fitness[obj] = mean + 0.1*std
// for that chunk. Matches neuralSpring Paper 014 (Directed Evolution).
//
// Input: genotypes[individual * genome_len + locus] — flat [pop_size × genome_len]
// Output: fitness[individual * n_objectives + obj] — [pop_size × n_objectives]
//
// Validates against: neuralSpring directed_evolution::multi_objective_fitness

@group(0) @binding(0) var<storage, read> genotypes: array<f32>;

@group(0) @binding(1) var<storage, read_write> fitness: array<f32>;

struct Params {
    pop_size: u32,
    genome_len: u32,
    n_objectives: u32,
    _pad: u32,
}
@group(0) @binding(2) var<uniform> params: Params;

@compute @workgroup_size(256)
fn multi_obj_fitness(@builtin(global_invocation_id) gid: vec3<u32>) {
    let idx = gid.x;
    let total_outputs = params.pop_size * params.n_objectives;
    if idx >= total_outputs {
        return;
    }

    let individual = idx / params.n_objectives;
    let objective = idx % params.n_objectives;
    let chunk_size = params.genome_len / params.n_objectives;
    let start = individual * params.genome_len + objective * chunk_size;

    var sum: f32 = 0.0;
    for (var i: u32 = 0u; i < chunk_size; i = i + 1u) {
        sum = sum + genotypes[start + i];
    }
    let mean = sum / f32(chunk_size);

    var var_sum: f32 = 0.0;
    for (var i: u32 = 0u; i < chunk_size; i = i + 1u) {
        let diff = genotypes[start + i] - mean;
        var_sum = fma(diff, diff, var_sum);
    }
    let variance = var_sum / f32(chunk_size);
    let std_dev = sqrt(variance);

    fitness[idx] = mean + 0.1 * std_dev;
}
