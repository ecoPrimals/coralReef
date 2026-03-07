// rk4_parallel.wgsl — Parallel Multi-System RK4 Integration (f32)
//
// Integrates N independent ODE systems simultaneously on GPU.
// Each thread handles one complete system, stepping from t=0 to t=T
// with fixed step size dt using classical RK4.
//
// The derivative function is encoded in the coeffs buffer as a
// coefficient matrix for Hill-function-based regulatory network ODEs.
//
// GPU dispatch: ceil(n_systems / 64) workgroups, 64 threads each.
//
// Provenance: neuralSpring metalForge (Feb 21, 2026) → ToadStool absorption
// Reference: Mhatre et al. (2020) PNAS, Srivastava et al. (2011) J Bact

@group(0) @binding(0) var<storage, read_write> state: array<f32>;
@group(0) @binding(1) var<storage, read> coeffs: array<f32>;
@group(0) @binding(2) var<storage, read_write> state_out: array<f32>;

struct OdeParams {
    n_systems: u32,
    dim: u32,
    n_steps: u32,
    dt: f32,
    n_coeffs: u32,
}
@group(0) @binding(3) var<uniform> params: OdeParams;

@group(0) @binding(4) var<storage, read_write> scratch: array<f32>;

fn hill(x: f32, k: f32, n: f32) -> f32 {
    let xn = pow(x, n);
    return xn / (pow(k, n) + xn);
}

fn read_state(sys: u32, d: u32) -> f32 {
    return state[sys * params.dim + d];
}

fn write_scratch(sys: u32, stage: u32, d: u32, val: f32) {
    scratch[sys * params.dim * 5u + stage * params.dim + d] = val;
}

fn read_scratch(sys: u32, stage: u32, d: u32) -> f32 {
    return scratch[sys * params.dim * 5u + stage * params.dim + d];
}

@compute @workgroup_size(64)
fn rk4_step(@builtin(global_invocation_id) gid: vec3<u32>) {
    let sys = gid.x;
    if sys >= params.n_systems {
        return;
    }

    let dim = params.dim;
    let dt = params.dt;

    for (var step: u32 = 0u; step < params.n_steps; step = step + 1u) {
        // k1 = f(y)
        for (var d: u32 = 0u; d < dim; d = d + 1u) {
            let y = read_state(sys, d);
            let c_base = sys * params.n_coeffs + d * 3u;
            let prod = coeffs[c_base];
            let deg = coeffs[c_base + 1u];
            let activator_idx = u32(coeffs[c_base + 2u]);
            let activator = read_state(sys, activator_idx);
            let deriv = prod * hill(activator, 0.5, 2.0) - deg * y;
            write_scratch(sys, 0u, d, deriv);
        }

        // k2 = f(y + dt/2 * k1)
        for (var d: u32 = 0u; d < dim; d = d + 1u) {
            let y_tmp = read_state(sys, d) + 0.5 * dt * read_scratch(sys, 0u, d);
            write_scratch(sys, 4u, d, y_tmp);
        }
        for (var d: u32 = 0u; d < dim; d = d + 1u) {
            let c_base = sys * params.n_coeffs + d * 3u;
            let prod = coeffs[c_base];
            let deg = coeffs[c_base + 1u];
            let activator_idx = u32(coeffs[c_base + 2u]);
            let activator = read_scratch(sys, 4u, activator_idx);
            let y_tmp = read_scratch(sys, 4u, d);
            let deriv = prod * hill(activator, 0.5, 2.0) - deg * y_tmp;
            write_scratch(sys, 1u, d, deriv);
        }

        // k3 = f(y + dt/2 * k2)
        for (var d: u32 = 0u; d < dim; d = d + 1u) {
            let y_tmp = read_state(sys, d) + 0.5 * dt * read_scratch(sys, 1u, d);
            write_scratch(sys, 4u, d, y_tmp);
        }
        for (var d: u32 = 0u; d < dim; d = d + 1u) {
            let c_base = sys * params.n_coeffs + d * 3u;
            let prod = coeffs[c_base];
            let deg = coeffs[c_base + 1u];
            let activator_idx = u32(coeffs[c_base + 2u]);
            let activator = read_scratch(sys, 4u, activator_idx);
            let y_tmp = read_scratch(sys, 4u, d);
            let deriv = prod * hill(activator, 0.5, 2.0) - deg * y_tmp;
            write_scratch(sys, 2u, d, deriv);
        }

        // k4 = f(y + dt * k3)
        for (var d: u32 = 0u; d < dim; d = d + 1u) {
            let y_tmp = read_state(sys, d) + dt * read_scratch(sys, 2u, d);
            write_scratch(sys, 4u, d, y_tmp);
        }
        for (var d: u32 = 0u; d < dim; d = d + 1u) {
            let c_base = sys * params.n_coeffs + d * 3u;
            let prod = coeffs[c_base];
            let deg = coeffs[c_base + 1u];
            let activator_idx = u32(coeffs[c_base + 2u]);
            let activator = read_scratch(sys, 4u, activator_idx);
            let y_tmp = read_scratch(sys, 4u, d);
            let deriv = prod * hill(activator, 0.5, 2.0) - deg * y_tmp;
            write_scratch(sys, 3u, d, deriv);
        }

        // y_new = y + (dt/6)(k1 + 2*k2 + 2*k3 + k4)
        for (var d: u32 = 0u; d < dim; d = d + 1u) {
            let k1 = read_scratch(sys, 0u, d);
            let k2 = read_scratch(sys, 1u, d);
            let k3 = read_scratch(sys, 2u, d);
            let k4 = read_scratch(sys, 3u, d);
            let y = read_state(sys, d);
            state[sys * dim + d] = y + (dt / 6.0) * (k1 + 2.0 * k2 + 2.0 * k3 + k4);
        }
    }

    for (var d: u32 = 0u; d < dim; d = d + 1u) {
        state_out[sys * dim + d] = state[sys * dim + d];
    }
}
