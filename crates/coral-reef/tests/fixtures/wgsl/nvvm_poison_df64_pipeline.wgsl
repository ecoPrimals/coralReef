// NVVM Poisoning Pattern: DF64 pipeline with transcendentals
//
// This shader combines the double-float (DF64) emulation pattern with
// f64 transcendentals — the exact combination that poisons NVVM on
// the NVIDIA proprietary driver. The f32-pair rewrite confuses NVVM's
// builtin resolution for exp/log, causing permanent device death.
//
// coralReef compiles this to native SASS without NVVM involvement.
//
// Source: hotSpring v0.6.25 Precision Brain + NVVM Poisoning Handoff

struct Params {
    n:    u32,
    beta: f64,
}

@group(0) @binding(0) var<uniform>             params: Params;
@group(0) @binding(1) var<storage, read>       x_arr:  array<f64>;
@group(0) @binding(2) var<storage, read_write> out:    array<f64>;

// Partition function accumulator — the classic physics pattern that
// requires f64 exp() and triggers NVVM poisoning
@compute @workgroup_size(256)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if (i >= params.n) { return; }

    let x = x_arr[i];
    let beta = params.beta;

    // Boltzmann weight: exp(-beta * E)
    // This is the EXACT pattern that poisons NVVM when compiled at
    // DF64 or F64Precise tier on the proprietary driver
    let energy = x * x + 0.5 * x;
    let weight = exp(-beta * energy);

    // Log-sum-exp accumulation (also triggers NVVM failure)
    let log_weight = log(weight + 1e-300);
    let log_partition = log(exp(log_weight) + exp(-log_weight));

    // Trigonometric + transcendental mix
    let phase = sin(x * 3.14159265358979) * cos(x * 1.57079632679490);
    let damped = phase * exp(-abs(x) * 0.1);

    out[i] = weight + log_partition + damped;
}
