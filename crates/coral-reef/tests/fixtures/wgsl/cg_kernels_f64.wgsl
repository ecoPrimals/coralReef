// cg_kernels_f64.wgsl — BLAS-like kernels for lattice CG solver
//
// Three kernels for the Conjugate Gradient algorithm on complex fermion fields:
//   1. complex_dot_re: Re(<a|b>) per element (reduce externally)
//   2. axpy: y += alpha * x
//   3. xpay: p = x + beta * p
//
// All operate on flat f64 arrays where complex values are interleaved re/im.
//
// Absorbed from hotSpring v0.6.1 lattice/cg.rs (Feb 2026)

// ── Entry point 1: complex_dot_re ────────────────────────────────────────────

struct DotParams {
    n_pairs: u32,
    pad0: u32,
    pad1: u32,
    pad2: u32,
}

@group(0) @binding(0) var<uniform> dot_params: DotParams;
@group(0) @binding(1) var<storage, read> a: array<f64>;
@group(0) @binding(2) var<storage, read> b: array<f64>;
@group(0) @binding(3) var<storage, read_write> dot_out: array<f64>;

@compute @workgroup_size(64)
fn complex_dot_re(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if i >= dot_params.n_pairs { return; }
    dot_out[i] = a[i * 2u] * b[i * 2u] + a[i * 2u + 1u] * b[i * 2u + 1u];
}

// ── Entry point 2: axpy ──────────────────────────────────────────────────────

struct AxpyParams {
    n: u32,
    pad0: u32,
    alpha: f64,
}

@group(0) @binding(0) var<uniform> axpy_params: AxpyParams;
@group(0) @binding(1) var<storage, read> axpy_x: array<f64>;
@group(0) @binding(2) var<storage, read_write> axpy_y: array<f64>;

@compute @workgroup_size(64)
fn axpy(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if i >= axpy_params.n { return; }
    axpy_y[i] = axpy_y[i] + axpy_params.alpha * axpy_x[i];
}

// ── Entry point 3: xpay ──────────────────────────────────────────────────────

struct XpayParams {
    n: u32,
    pad0: u32,
    beta: f64,
}

@group(0) @binding(0) var<uniform> xpay_params: XpayParams;
@group(0) @binding(1) var<storage, read> xpay_x: array<f64>;
@group(0) @binding(2) var<storage, read_write> xpay_p: array<f64>;

@compute @workgroup_size(64)
fn xpay(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if i >= xpay_params.n { return; }
    xpay_p[i] = xpay_x[i] + xpay_params.beta * xpay_p[i];
}
