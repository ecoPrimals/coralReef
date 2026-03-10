// NVVM Poisoning Pattern: F64Precise (no-FMA) with transcendentals
//
// On the NVIDIA proprietary driver, F64Precise mode (no FMA fusion)
// breaks NVVM's transcendental implementation. The no-FMA compilation
// flags prevent NVVM from resolving exp/log builtins, causing the
// wgpu device to enter a permanent error state.
//
// coralReef honors FmaPolicy::NoContraction through its own codegen,
// producing correct SASS without touching NVVM.
//
// Source: hotSpring v0.6.25 NVVM Poisoning Handoff
//         groundSpring V97 f64 transcendental root cause analysis

@group(0) @binding(0) var<storage, read>       a: array<f64>;
@group(0) @binding(1) var<storage, read>       b: array<f64>;
@group(0) @binding(2) var<storage, read_write> c: array<f64>;

// Kahan-compensated summation with transcendentals — requires
// precise (no-FMA) arithmetic to maintain numerical stability.
// FMA fusion changes rounding and breaks CG convergence (hotSpring finding).
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    let n = arrayLength(&a);
    if (i >= n) { return; }

    let x = a[i];
    let y = b[i];

    // Precise multiply-add (no FMA fusion allowed for numerical stability)
    let prod = x * y;
    let sum = prod + 1.0;

    // f64 transcendentals that require no-FMA precision
    let r1 = exp(sum) - 1.0;
    let r2 = log(1.0 + abs(prod));

    // Compensated accumulation (Kahan pattern)
    let s = r1 + r2;
    let t = s - r1;
    let compensation = r2 - t;

    c[i] = s + compensation;
}
