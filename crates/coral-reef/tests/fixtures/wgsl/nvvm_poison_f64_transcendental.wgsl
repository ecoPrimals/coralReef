// NVVM Poisoning Pattern: f64 transcendentals (exp/log)
//
// On the NVIDIA proprietary driver, compiling this shader through
// wgpu (naga → SPIR-V → NVVM) permanently poisons the GPU device.
// Once poisoned, ALL subsequent buffer/dispatch/readback operations
// panic with "Buffer is invalid" — the only recovery is process restart.
//
// coralReef's sovereign WGSL → naga → codegen IR → native SASS path
// bypasses NVVM entirely, compiling this safely.
//
// Source: hotSpring v0.6.25 NVVM Device Poisoning Handoff (March 10, 2026)

@group(0) @binding(0) var<storage, read>       input:  array<f64>;
@group(0) @binding(1) var<storage, read_write>  output: array<f64>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    let n = arrayLength(&input);
    if (i >= n) { return; }

    let x = input[i];

    // f64 exp — triggers NVVM compilation failure on proprietary driver
    let exp_x = exp(x);

    // f64 log — triggers NVVM compilation failure on proprietary driver
    let log_x = log(x + 1.0);

    // f64 exp2/log2 — also problematic through NVVM
    let exp2_x = exp2(x * 0.5);
    let log2_x = log2(abs(x) + 1.0);

    // Combined: Boltzmann factor (physics use case from hotSpring)
    let beta = 1.0 / 0.025;  // 1/kT at room temperature
    let boltzmann = exp(-beta * x * x);

    output[i] = exp_x + log_x + exp2_x + log2_x + boltzmann;
}
