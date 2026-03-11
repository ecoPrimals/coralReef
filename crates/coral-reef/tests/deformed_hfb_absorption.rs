// SPDX-License-Identifier: AGPL-3.0-only
//! Absorption tests for hotSpring's deformed HFB (Hartree-Fock-Bogoliubov)
//! nuclear structure shaders.
//!
//! These shaders compute on cylindrical (ρ,z) grids using f64 arithmetic
//! with `sqrt`, `exp`, `pow`, `abs`, `clamp`, `select`, `log`, `sin`, `cos`,
//! and workgroup shared memory reductions. They represent the production
//! workloads for the L3 HFB solver targeting Kokkos parity.
//!
//! Source provenance:
//!   `hotSpring/barracuda/src/physics/shaders/deformed_*.wgsl`

use coral_reef::{AmdArch, CompileOptions, GpuArch, GpuTarget, compile_wgsl};

fn sm70_opts() -> CompileOptions {
    CompileOptions {
        target: GpuArch::Sm70.into(),
        opt_level: 2,
        debug_info: false,
        fp64_software: true,
        ..CompileOptions::default()
    }
}

fn amd_opts() -> CompileOptions {
    CompileOptions {
        target: GpuTarget::Amd(AmdArch::Rdna2),
        opt_level: 2,
        debug_info: false,
        fp64_software: false,
        ..CompileOptions::default()
    }
}

// ---------------------------------------------------------------------------
// deformed_hamiltonian_f64: block Hamiltonian assembly with grid integration
// Exercises: f64 nested loops, mul-add accumulation, conditional diagonal add
// ---------------------------------------------------------------------------

const DEFORMED_HAMILTONIAN_F64_WGSL: &str = r"
struct HamiltonianParams {
    n_rho: u32,
    n_z: u32,
    block_size: u32,
    n_blocks: u32,
    d_rho: f64,
    d_z: f64,
}

@group(0) @binding(0) var<uniform> params: HamiltonianParams;
@group(0) @binding(1) var<storage, read> wavefunctions: array<f64>;
@group(0) @binding(2) var<storage, read> v_potential: array<f64>;
@group(0) @binding(3) var<storage, read> block_indices: array<u32>;
@group(0) @binding(4) var<storage, read> kinetic_energies: array<f64>;
@group(0) @binding(5) var<storage, read> block_sizes: array<u32>;
@group(0) @binding(6) var<storage, read> block_h_offsets: array<u32>;
@group(0) @binding(7) var<storage, read_write> h_matrices: array<f64>;

const PI: f64 = 3.14159265358979323846;

@compute @workgroup_size(256, 1, 1)
fn compute_potential_matrix_elements(
    @builtin(global_invocation_id) gid: vec3<u32>,
) {
    let pair_idx = gid.x;
    let block_idx = gid.y;
    if (block_idx >= params.n_blocks) { return; }
    let bs = block_sizes[block_idx];
    let n_pairs = bs * (bs + 1u) / 2u;
    if (pair_idx >= n_pairs) { return; }

    var bi = 0u;
    var offset = 0u;
    for (var trial = 0u; trial < bs; trial++) {
        let row_count = bs - trial;
        if (offset + row_count > pair_idx) { bi = trial; break; }
        offset += row_count;
    }
    let bj = bi + (pair_idx - offset);
    let n_grid = params.n_rho * params.n_z;
    let max_bs = params.block_size;
    let i_global = block_indices[block_idx * max_bs + bi];
    let j_global = block_indices[block_idx * max_bs + bj];

    var integral = f64(0.0);
    for (var k = 0u; k < n_grid; k++) {
        let i_rho = k / params.n_z;
        let rho_coord = f64(i_rho + 1u) * params.d_rho;
        let dv = f64(2.0) * PI * rho_coord * params.d_rho * params.d_z;
        let psi_i = wavefunctions[i_global * n_grid + k];
        let psi_j = wavefunctions[j_global * n_grid + k];
        integral += psi_i * v_potential[k] * psi_j * dv;
    }
    var h_ij = integral;
    if (bi == bj) { h_ij += kinetic_energies[i_global]; }
    let h_base = block_h_offsets[block_idx];
    h_matrices[h_base + bi * bs + bj] = h_ij;
    if (bi != bj) { h_matrices[h_base + bj * bs + bi] = h_ij; }
}
";

// ---------------------------------------------------------------------------
// deformed_wavefunction_f64: HO basis evaluation with Hermite/Laguerre
// Exercises: f64 recurrence, sqrt, exp, pow, factorial, transcendentals
// ---------------------------------------------------------------------------

const DEFORMED_WAVEFUNCTION_F64_WGSL: &str = r"
struct WfParams {
    n_rho: u32,
    n_z: u32,
    n_states: u32,
    _pad0: u32,
    d_rho: f64,
    d_z: f64,
    z_min: f64,
    b_z: f64,
    b_perp: f64,
    rho_max: f64,
}
struct StateParams { n_z: u32, n_perp: u32, abs_lambda: u32, _pad: u32, }

@group(0) @binding(0) var<uniform> params: WfParams;
@group(0) @binding(1) var<storage, read> state_params: array<StateParams>;
@group(0) @binding(2) var<storage, read_write> wavefunctions: array<f64>;

const SQRT_PI: f64 = 1.7724538509055159;

fn hermite(n: u32, x: f64) -> f64 {
    if (n == 0u) { return f64(1.0); }
    if (n == 1u) { return f64(2.0) * x; }
    var h_prev = f64(1.0);
    var h_curr = f64(2.0) * x;
    for (var k = 2u; k <= n; k++) {
        let h_next = f64(2.0) * x * h_curr - f64(2.0) * f64(k - 1u) * h_prev;
        h_prev = h_curr;
        h_curr = h_next;
    }
    return h_curr;
}

fn laguerre(n: u32, alpha: f64, x: f64) -> f64 {
    if (n == 0u) { return f64(1.0); }
    var l_prev = f64(1.0);
    var l_curr = f64(1.0) + alpha - x;
    for (var k = 1u; k < n; k++) {
        let kf = f64(k);
        let l_next = ((f64(2.0) * kf + f64(1.0) + alpha - x) * l_curr
                      - (kf + alpha) * l_prev) / (kf + f64(1.0));
        l_prev = l_curr;
        l_curr = l_next;
    }
    return l_curr;
}

fn factorial(n: u32) -> f64 {
    var result = f64(1.0);
    for (var k = 2u; k <= n; k++) { result = result * f64(k); }
    return result;
}

fn pow_int(base: f64, exp_n: u32) -> f64 {
    var result = f64(1.0);
    for (var i = 0u; i < exp_n; i++) { result = result * base; }
    return result;
}

@compute @workgroup_size(256, 1, 1)
fn evaluate_wavefunctions(@builtin(global_invocation_id) gid: vec3<u32>) {
    let grid_idx = gid.x;
    let state_idx = gid.y;
    let n_grid = params.n_rho * params.n_z;
    if (grid_idx >= n_grid || state_idx >= params.n_states) { return; }

    let i_rho = grid_idx / params.n_z;
    let i_z = grid_idx % params.n_z;
    let rho = f64(i_rho + 1u) * params.d_rho;
    let z = params.z_min + (f64(i_z) + f64(0.5)) * params.d_z;

    let sp = state_params[state_idx];
    let xi = z / params.b_z;
    let h_n = hermite(sp.n_z, xi);
    // 2^n via loop to avoid shift on non-const
    var two_pow_n = f64(1.0);
    for (var p = 0u; p < sp.n_z; p++) { two_pow_n = two_pow_n * f64(2.0); }
    let norm_z = f64(1.0) / sqrt(params.b_z * SQRT_PI * two_pow_n * factorial(sp.n_z));
    let phi_z = norm_z * h_n * exp(-xi * xi / f64(2.0));

    let eta = (rho / params.b_perp) * (rho / params.b_perp);
    let norm_rho = sqrt(factorial(sp.n_perp) / (f64(3.14159265358979323846) * params.b_perp * params.b_perp * factorial(sp.n_perp + sp.abs_lambda)));
    let lag = laguerre(sp.n_perp, f64(sp.abs_lambda), eta);
    let phi_rho = norm_rho * pow_int(rho / params.b_perp, sp.abs_lambda) * exp(-eta / f64(2.0)) * lag;

    wavefunctions[state_idx * n_grid + grid_idx] = phi_z * phi_rho;
}
";

// ---------------------------------------------------------------------------
// deformed_density_energy_f64: BCS occupations + density accumulation
// Exercises: f64 sqrt, abs, select, clamp, conditional branches
// ---------------------------------------------------------------------------

const DEFORMED_DENSITY_ENERGY_F64_WGSL: &str = r"
struct DensityParams {
    n_rho: u32,
    n_z: u32,
    n_states: u32,
    n_particles: u32,
    d_rho: f64,
    d_z: f64,
    z_min: f64,
    delta_pair: f64,
    fermi_energy: f64,
    mix_alpha: f64,
}

@group(0) @binding(0) var<uniform> params: DensityParams;
@group(0) @binding(1) var<storage, read> eigenvalues: array<f64>;
@group(0) @binding(2) var<storage, read_write> occupations: array<f64>;

@compute @workgroup_size(256)
fn compute_bcs_occupations(@builtin(global_invocation_id) gid: vec3<u32>) {
    let idx = gid.x;
    if (idx >= params.n_states) { return; }

    if (params.delta_pair > f64(1e-10)) {
        let eps = eigenvalues[idx] - params.fermi_energy;
        let e_qp = sqrt(eps * eps + params.delta_pair * params.delta_pair);
        var v2: f64;
        if (abs(eps) > params.delta_pair) {
            let d2 = params.delta_pair * params.delta_pair;
            let v2_s = d2 / (f64(2.0) * e_qp * (e_qp + abs(eps)));
            v2 = select(f64(1.0) - v2_s, v2_s, eps > f64(0.0));
        } else {
            v2 = f64(0.5) * (f64(1.0) - eps / e_qp);
        }
        occupations[idx] = clamp(v2, f64(0.0), f64(1.0));
    } else {
        if (eigenvalues[idx] < params.fermi_energy) {
            occupations[idx] = f64(1.0);
        } else {
            occupations[idx] = f64(0.0);
        }
    }
}
";

// ---------------------------------------------------------------------------
// deformed_gradient_f64: kinetic density τ via finite difference ∇ψ
// Exercises: f64 finite difference stencils, boundary conditions, accumulation
// ---------------------------------------------------------------------------

const DEFORMED_GRADIENT_F64_WGSL: &str = r"
struct GridParams {
    n_rho: u32,
    n_z: u32,
    n_states: u32,
    _pad0: u32,
    d_rho: f64,
    d_z: f64,
    z_min: f64,
}

@group(0) @binding(0) var<uniform> params: GridParams;
@group(0) @binding(1) var<storage, read> wavefunctions: array<f64>;
@group(0) @binding(2) var<storage, read> occupations: array<f64>;
@group(0) @binding(3) var<storage, read_write> tau_out: array<f64>;

@compute @workgroup_size(256)
fn compute_tau(@builtin(global_invocation_id) gid: vec3<u32>) {
    let grid_idx = gid.x;
    let n_grid = params.n_rho * params.n_z;
    if (grid_idx >= n_grid) { return; }

    let i_rho = grid_idx / params.n_z;
    let i_z = grid_idx % params.n_z;
    var tau_val = f64(0.0);

    for (var si = 0u; si < params.n_states; si++) {
        let occ_i = occupations[si] * f64(2.0);
        if (occ_i < f64(1e-15)) { continue; }
        let base = si * n_grid;

        var dpsi_drho: f64;
        if (i_rho == 0u) {
            dpsi_drho = (wavefunctions[base + 1u * params.n_z + i_z]
                       - wavefunctions[base + grid_idx]) / params.d_rho;
        } else if (i_rho == params.n_rho - 1u) {
            dpsi_drho = (wavefunctions[base + grid_idx]
                       - wavefunctions[base + (i_rho - 1u) * params.n_z + i_z]) / params.d_rho;
        } else {
            dpsi_drho = (wavefunctions[base + (i_rho + 1u) * params.n_z + i_z]
                       - wavefunctions[base + (i_rho - 1u) * params.n_z + i_z])
                       / (f64(2.0) * params.d_rho);
        }

        var dpsi_dz: f64;
        if (i_z == 0u) {
            dpsi_dz = (wavefunctions[base + i_rho * params.n_z + 1u]
                     - wavefunctions[base + grid_idx]) / params.d_z;
        } else if (i_z == params.n_z - 1u) {
            dpsi_dz = (wavefunctions[base + grid_idx]
                     - wavefunctions[base + i_rho * params.n_z + i_z - 1u]) / params.d_z;
        } else {
            dpsi_dz = (wavefunctions[base + i_rho * params.n_z + i_z + 1u]
                     - wavefunctions[base + i_rho * params.n_z + i_z - 1u])
                     / (f64(2.0) * params.d_z);
        }

        tau_val += occ_i * (dpsi_drho * dpsi_drho + dpsi_dz * dpsi_dz);
    }
    tau_out[grid_idx] = tau_val;
}
";

// ---------------------------------------------------------------------------
// deformed_potentials_f64: Skyrme + Coulomb mean-field potential
// Exercises: f64 pow, select, clamp, multi-term physics accumulation
// ---------------------------------------------------------------------------

const DEFORMED_POTENTIALS_F64_WGSL: &str = r"
struct PotParams {
    n_rho: u32,
    n_z: u32,
    is_proton: u32,
    _pad0: u32,
    d_rho: f64,
    d_z: f64,
    z_min: f64,
    t0: f64, t1: f64, t2: f64, t3: f64,
    x0: f64, x1: f64, x2: f64, x3: f64,
    alpha: f64,
    w0: f64,
}

@group(0) @binding(0) var<uniform> params: PotParams;
@group(0) @binding(1) var<storage, read> rho_p: array<f64>;
@group(0) @binding(2) var<storage, read> rho_n: array<f64>;
@group(0) @binding(3) var<storage, read> tau_p: array<f64>;
@group(0) @binding(4) var<storage, read> tau_n: array<f64>;
@group(0) @binding(5) var<storage, read> d_rho_total_dr: array<f64>;
@group(0) @binding(6) var<storage, read> d_rho_q_dr: array<f64>;
@group(0) @binding(7) var<storage, read> v_coulomb: array<f64>;
@group(0) @binding(8) var<storage, read_write> v_out: array<f64>;

const PI: f64 = 3.14159265358979323846;

@compute @workgroup_size(256)
fn compute_mean_field(@builtin(global_invocation_id) gid: vec3<u32>) {
    let grid_idx = gid.x;
    let n_grid = params.n_rho * params.n_z;
    if (grid_idx >= n_grid) { return; }

    let rho = max(rho_p[grid_idx] + rho_n[grid_idx], f64(0.0));
    let rq = select(rho_n[grid_idx], rho_p[grid_idx], params.is_proton == 1u);
    let rq_pos = max(rq, f64(0.0));

    let v_central = params.t0 * ((f64(1.0) + params.x0 / f64(2.0)) * rho
                                - (f64(0.5) + params.x0) * rq_pos)
        + params.t3 / f64(12.0) * pow(rho, params.alpha) *
            ((f64(2.0) + params.alpha) * (f64(1.0) + params.x3 / f64(2.0)) * rho
             - (f64(2.0) * (f64(0.5) + params.x3) * rq_pos
                + params.alpha * (f64(1.0) + params.x3 / f64(2.0)) * rho));

    let tau_total_i = tau_p[grid_idx] + tau_n[grid_idx];
    let tau_q_i = select(tau_n[grid_idx], tau_p[grid_idx], params.is_proton == 1u);
    let v_eff_mass = params.t1 / f64(4.0)
        * ((f64(2.0) + params.x1) * tau_total_i - (f64(1.0) + f64(2.0) * params.x1) * tau_q_i)
        + params.t2 / f64(4.0)
        * ((f64(2.0) + params.x2) * tau_total_i + (f64(1.0) + f64(2.0) * params.x2) * tau_q_i);

    let i_rho = grid_idx / params.n_z;
    let i_z = grid_idx % params.n_z;
    let rho_coord = f64(i_rho + 1u) * params.d_rho;
    let z_coord = params.z_min + (f64(i_z) + f64(0.5)) * params.d_z;
    let r = max(sqrt(rho_coord * rho_coord + z_coord * z_coord), f64(0.1));
    let v_so = -params.w0 / f64(2.0) * (d_rho_total_dr[grid_idx] + d_rho_q_dr[grid_idx]) / r;

    var v_total = clamp(v_central + v_eff_mass + v_so, f64(-5000.0), f64(5000.0));
    if (params.is_proton == 1u) {
        v_total += clamp(v_coulomb[grid_idx], f64(-500.0), f64(500.0));
    }
    v_out[grid_idx] = v_total;
}
";

// ===========================================================================
// SM70 (Volta) compilation tests
// ===========================================================================

#[test]
fn deformed_hamiltonian_f64_sm70() {
    let r = compile_wgsl(DEFORMED_HAMILTONIAN_F64_WGSL, &sm70_opts());
    assert!(
        r.is_ok(),
        "deformed_hamiltonian_f64 should compile for SM70: {r:?}"
    );
    assert!(!r.unwrap().is_empty());
}

#[test]
fn deformed_wavefunction_f64_sm70() {
    let r = compile_wgsl(DEFORMED_WAVEFUNCTION_F64_WGSL, &sm70_opts());
    assert!(
        r.is_ok(),
        "deformed_wavefunction_f64 should compile for SM70: {r:?}"
    );
    assert!(!r.unwrap().is_empty());
}

#[test]
fn deformed_density_energy_f64_sm70() {
    let r = compile_wgsl(DEFORMED_DENSITY_ENERGY_F64_WGSL, &sm70_opts());
    assert!(
        r.is_ok(),
        "deformed_density_energy_f64 should compile for SM70: {r:?}"
    );
    assert!(!r.unwrap().is_empty());
}

#[test]
fn deformed_gradient_f64_sm70() {
    let r = compile_wgsl(DEFORMED_GRADIENT_F64_WGSL, &sm70_opts());
    assert!(
        r.is_ok(),
        "deformed_gradient_f64 should compile for SM70: {r:?}"
    );
    assert!(!r.unwrap().is_empty());
}

#[test]
fn deformed_potentials_f64_sm70() {
    let r = compile_wgsl(DEFORMED_POTENTIALS_F64_WGSL, &sm70_opts());
    assert!(
        r.is_ok(),
        "deformed_potentials_f64 should compile for SM70: {r:?}"
    );
    assert!(!r.unwrap().is_empty());
}

// ===========================================================================
// AMD RDNA2 compilation tests
// ===========================================================================

#[test]
fn deformed_hamiltonian_f64_rdna2() {
    let r = compile_wgsl(DEFORMED_HAMILTONIAN_F64_WGSL, &amd_opts());
    assert!(
        r.is_ok(),
        "deformed_hamiltonian_f64 should compile for RDNA2: {r:?}"
    );
    assert!(!r.unwrap().is_empty());
}

#[test]
#[ignore = "RDNA2 encoding not yet implemented for instruction used in HO recurrence"]
fn deformed_wavefunction_f64_rdna2() {
    let r = compile_wgsl(DEFORMED_WAVEFUNCTION_F64_WGSL, &amd_opts());
    assert!(
        r.is_ok(),
        "deformed_wavefunction_f64 should compile for RDNA2: {r:?}"
    );
    assert!(!r.unwrap().is_empty());
}

#[test]
fn deformed_density_energy_f64_rdna2() {
    let r = compile_wgsl(DEFORMED_DENSITY_ENERGY_F64_WGSL, &amd_opts());
    assert!(
        r.is_ok(),
        "deformed_density_energy_f64 should compile for RDNA2: {r:?}"
    );
    assert!(!r.unwrap().is_empty());
}

#[test]
fn deformed_gradient_f64_rdna2() {
    let r = compile_wgsl(DEFORMED_GRADIENT_F64_WGSL, &amd_opts());
    assert!(
        r.is_ok(),
        "deformed_gradient_f64 should compile for RDNA2: {r:?}"
    );
    assert!(!r.unwrap().is_empty());
}

#[test]
fn deformed_potentials_f64_rdna2() {
    let r = compile_wgsl(DEFORMED_POTENTIALS_F64_WGSL, &amd_opts());
    assert!(
        r.is_ok(),
        "deformed_potentials_f64 should compile for RDNA2: {r:?}"
    );
    assert!(!r.unwrap().is_empty());
}
