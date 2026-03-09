// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Batch Fitness Evaluation — Parallel Population GEMM via WGSL
//
// Evaluates fitness for an entire EA population in a single GPU dispatch.
// Each individual's genotype is a row in the population matrix; fitness
// is computed as a dot product with a trait-weight vector (linear fitness)
// or via a nonlinear landscape function.
//
// This replaces the CPU loop `for ind in population: fitness(ind)` with
// a single batched dispatch where each thread handles one individual.
//
// Absorption target: barracuda::ops::batch_gemm or batch_elementwise
// Validates against: neuralSpring Papers 011–015 (Dolson et al.)
// Reference: Dolson et al. (2020) Nature Physics, (2022) eLife

// Population matrix: pop[i * genome_len + g] = allele value
@group(0) @binding(0) var<storage, read> population: array<f32>;

// Trait weights: weights[g] (linear fitness landscape)
@group(0) @binding(1) var<storage, read> weights: array<f32>;

// Output fitness values: fitness[i]
@group(0) @binding(2) var<storage, read_write> fitness: array<f32>;

struct FitnessParams {
    pop_size: u32,
    genome_len: u32,
}
@group(0) @binding(3) var<uniform> params: FitnessParams;

@compute @workgroup_size(256)
fn batch_fitness_linear(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if i >= params.pop_size {
        return;
    }

    let base = i * params.genome_len;
    var acc: f32 = 0.0;
    for (var g: u32 = 0u; g < params.genome_len; g = g + 1u) {
        acc = fma(population[base + g], weights[g], acc);
    }
    fitness[i] = acc;
}

// NK landscape variant: epistatic interactions between K neighbors.
// interaction_table[g * (K+1) + k] encodes the fitness contribution
// of gene g given the state of its K neighbors.
//
// For now, this is a placeholder entry point; the full NK lookup
// requires a more complex binding layout that will be designed
// during validation against Paper 011 (counterdiabatic evolution).
