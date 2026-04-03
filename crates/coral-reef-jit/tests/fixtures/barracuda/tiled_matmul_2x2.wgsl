// barraCuda-pattern shader — vendored from coralReef triple-path validation corpus.
// Origin: barraCuda WGSL compute shader patterns (elementwise / activation / reduction / norm / matmul).
// Validated through CoralIR interpreter + sovereign Cranelift JIT, March 2026.

var<workgroup> tile_a: array<f32, 4>;
var<workgroup> tile_b: array<f32, 4>;

@group(0) @binding(0) var<storage, read> a: array<f32>;
@group(0) @binding(1) var<storage, read> b: array<f32>;
@group(0) @binding(2) var<storage, read_write> c: array<f32>;

@compute @workgroup_size(2, 2)
fn main(@builtin(local_invocation_id) lid: vec3<u32>) {
    let row = lid.y;
    let col = lid.x;
    let linear = row * 2u + col;

    // Load A and B tiles into shared memory
    tile_a[linear] = a[linear];
    tile_b[linear] = b[linear];
    workgroupBarrier();

    // Compute C[row][col] = sum_k A[row][k] * B[k][col]
    var sum = 0.0;
    for (var k = 0u; k < 2u; k = k + 1u) {
        sum = sum + tile_a[row * 2u + k] * tile_b[k * 2u + col];
    }
    c[linear] = sum;
}
