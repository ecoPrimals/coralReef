// barraCuda-pattern shader — vendored from coralReef triple-path validation corpus.
// Origin: barraCuda WGSL compute shader patterns (elementwise / activation / reduction / norm / matmul).
// Validated through CoralIR interpreter + sovereign Cranelift JIT, March 2026.

@group(0) @binding(0) var<storage, read> a: array<f32>;
@group(0) @binding(1) var<storage, read> b: array<f32>;
@group(0) @binding(2) var<storage, read_write> out: array<f32>;

@compute @workgroup_size(1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    var acc: f32 = 0.0;
    let base = gid.x * 4u;
    for (var i: u32 = 0u; i < 4u; i = i + 1u) {
        acc = acc + a[base + i] * b[base + i];
    }
    out[gid.x] = acc;
}
