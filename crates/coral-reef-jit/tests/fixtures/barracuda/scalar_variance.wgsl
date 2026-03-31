// barraCuda-pattern shader — vendored from coralReef triple-path validation corpus.
// Origin: barraCuda WGSL compute shader patterns (elementwise / activation / reduction / norm / matmul).
// Validated through CoralIR interpreter + sovereign Cranelift JIT, March 2026.

@group(0) @binding(0) var<storage, read> input: array<f32>;
@group(0) @binding(1) var<storage, read_write> output: array<f32>;

@compute @workgroup_size(1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let n: u32 = 4u;
    let base = gid.x * n;
    var mean_acc: f32 = 0.0;
    for (var i: u32 = 0u; i < n; i = i + 1u) {
        mean_acc = mean_acc + input[base + i];
    }
    let mean = mean_acc / f32(n);
    var var_acc: f32 = 0.0;
    for (var j: u32 = 0u; j < n; j = j + 1u) {
        let diff = input[base + j] - mean;
        var_acc = var_acc + diff * diff;
    }
    output[gid.x] = var_acc / f32(n);
}
