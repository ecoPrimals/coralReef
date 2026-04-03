// barraCuda-pattern shader — vendored from coralReef triple-path validation corpus.
// Origin: barraCuda WGSL compute shader patterns (elementwise / activation / reduction / norm / matmul).
// Validated through CoralIR interpreter + sovereign Cranelift JIT, March 2026.

var<workgroup> smem: array<f32, 16>;

@group(0) @binding(0) var<storage, read> input: array<f32>;
@group(0) @binding(1) var<storage, read_write> output: array<f32>;

@compute @workgroup_size(16)
fn main(
    @builtin(local_invocation_id) lid: vec3<u32>,
    @builtin(global_invocation_id) gid: vec3<u32>,
) {
    smem[lid.x] = input[gid.x];
    workgroupBarrier();

    for (var stride = 8u; stride > 0u; stride = stride >> 1u) {
        if lid.x < stride {
            let a = smem[lid.x];
            let b = smem[lid.x + stride];
            if b > a { smem[lid.x] = b; }
        }
        workgroupBarrier();
    }

    if lid.x == 0u {
        output[0u] = smem[0u];
    }
}
