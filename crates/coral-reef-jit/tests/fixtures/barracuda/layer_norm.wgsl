// barraCuda-pattern shader — vendored from coralReef triple-path validation corpus.
// Origin: barraCuda WGSL compute shader patterns (elementwise / activation / reduction / norm / matmul).
// Validated through CoralIR interpreter + sovereign Cranelift JIT, March 2026.

var<workgroup> smem: array<f32, 4>;

@group(0) @binding(0) var<storage, read_write> data: array<f32>;

@compute @workgroup_size(4)
fn main(@builtin(local_invocation_id) lid: vec3<u32>) {
    let val = data[lid.x];
    smem[lid.x] = val;
    workgroupBarrier();

    // Compute mean via reduction
    if lid.x < 2u { smem[lid.x] = smem[lid.x] + smem[lid.x + 2u]; }
    workgroupBarrier();
    if lid.x == 0u { smem[0u] = smem[0u] + smem[1u]; }
    workgroupBarrier();

    let mean = smem[0u] / 4.0;

    // Compute variance: store (val - mean)^2 into shared
    let diff = val - mean;
    smem[lid.x] = diff * diff;
    workgroupBarrier();

    if lid.x < 2u { smem[lid.x] = smem[lid.x] + smem[lid.x + 2u]; }
    workgroupBarrier();
    if lid.x == 0u { smem[0u] = smem[0u] + smem[1u]; }
    workgroupBarrier();

    let variance = smem[0u] / 4.0;
    let std_dev = sqrt(variance + 1e-5);
    data[lid.x] = (val - mean) / std_dev;
}
