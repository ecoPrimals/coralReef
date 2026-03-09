// SPDX-License-Identifier: AGPL-3.0-only
// Batched HFB Energy Functional (f64) — GPU Shader
//
// Computes the total energy E_total for each nucleus in the batch:
//   E = E_kin + E_t0 + E_t3 + E_coul_direct + E_coul_exchange + E_pair + E_cm
//
// This produces a per-grid-point integrand for each energy component,
// which is then summed via trapezoidal rule (SumReduceF64 or inline).
//
// For convergence checking: |E_total - E_prev| < tol
//
// Each thread computes the energy integrand at one grid point for one nucleus.
// Final sum is done per-batch via shared-memory reduction.

struct EnergyParams {
    n_states: u32,
    nr: u32,
    batch_size: u32,
    _pad: u32,
    t0_lo: u32, t0_hi: u32,
    t3_lo: u32, t3_hi: u32,
    x0_lo: u32, x0_hi: u32,
    x3_lo: u32, x3_hi: u32,
    alpha_lo: u32, alpha_hi: u32,
    dr_lo: u32, dr_hi: u32,
    hw_lo: u32, hw_hi: u32,  // hbar*omega for CM correction
}

fn decode_f64_e(lo: u32, hi: u32) -> f64 {
    return bitcast<f64>(vec2<u32>(lo, hi));
}

const E2_E: f64 = 1.4399764;
const PI_E: f64 = 3.141592653589793;

@group(0) @binding(0) var<uniform> params: EnergyParams;
@group(0) @binding(1) var<storage, read> rho_p: array<f64>;   // [batch × nr]
@group(0) @binding(2) var<storage, read> rho_n: array<f64>;   // [batch × nr]
@group(0) @binding(3) var<storage, read> r_grid: array<f64>;  // [nr]
@group(0) @binding(4) var<storage, read> charge_enclosed: array<f64>; // [batch × nr]
@group(0) @binding(5) var<storage, read_write> energy_integrands: array<f64>; // [batch × nr]
// Each grid point contributes:
// E_t0_integrand + E_t3_integrand + E_coul_direct_integrand + E_coul_exchange_integrand

// ═══════════════════════════════════════════════════════════════════
// Compute potential energy integrands at each grid point
// ═══════════════════════════════════════════════════════════════════
// Dispatch: (ceil(nr/256), batch_size, 1)
@compute @workgroup_size(256, 1, 1)
fn compute_energy_integrands(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let k = global_id.x;
    let batch_idx = global_id.y;
    let nr = params.nr;

    if (k >= nr || batch_idx >= params.batch_size) {
        return;
    }

    let t0 = decode_f64_e(params.t0_lo, params.t0_hi);
    let t3 = decode_f64_e(params.t3_lo, params.t3_hi);
    let x0 = decode_f64_e(params.x0_lo, params.x0_hi);
    let x3 = decode_f64_e(params.x3_lo, params.x3_hi);
    let alpha = decode_f64_e(params.alpha_lo, params.alpha_hi);
    let dr = decode_f64_e(params.dr_lo, params.dr_hi);

    let idx = batch_idx * nr + k;
    let rk = r_grid[k];
    let rp = rho_p[idx];
    let rn = rho_n[idx];
    let rho = rp + rn;
    let rho_safe = max(rho, f64(1e-20));
    let r2 = rk * rk;
    let vol = f64(4.0) * PI_E * r2;

    let sum_rho2 = rp * rp + rn * rn;

    // E_t0 integrand: (t0/2) * [(1+x0/2)*rho² - (1/2+x0)*sum_rho2] * 4π r²
    let e_t0 = (t0 / f64(2.0)) * ((f64(1.0) + x0 / f64(2.0)) * rho * rho
        - (f64(0.5) + x0) * sum_rho2) * vol;

    // E_t3 integrand: (t3/12) * rho^alpha * [(1+x3/2)*rho² - (1/2+x3)*sum_rho2] * 4π r²
    let rho_alpha = pow(rho_safe, alpha);
    let e_t3 = (t3 / f64(12.0)) * rho_alpha
        * ((f64(1.0) + x3 / f64(2.0)) * rho * rho
            - (f64(0.5) + x3) * sum_rho2) * vol;

    // E_coul_direct integrand: 0.5 * V_C(r) * rho_p(r) * 4π r²
    let v_c = E2_E * (charge_enclosed[idx] / max(rk, f64(1e-10)));
    // Note: phi_outer is not in this bind group — we approximate with charge_enclosed only
    // For full accuracy, include phi_outer in a separate pass or precompute V_C
    let e_coul_direct = f64(0.5) * v_c * rp * vol;

    // E_coul_exchange integrand: V_Cx(r) * rho_p(r) * 4π r²
    let coeff_cx = -E2_E * pow(f64(3.0) / PI_E, f64(1.0) / f64(3.0));
    let v_cx = coeff_cx * pow(max(rp, f64(0.0)), f64(1.0) / f64(3.0));
    let e_coul_exchange = v_cx * rp * vol;

    // Total integrand at this grid point (will be summed with dr weighting)
    energy_integrands[idx] = (e_t0 + e_t3 + e_coul_direct + e_coul_exchange) * dr;
}

// ═══════════════════════════════════════════════════════════════════
// Compute pairing energy per nucleus (sum over states, not grid)
// ═══════════════════════════════════════════════════════════════════
// E_pair = -sum_i delta_q * deg_i * sqrt(v²_i * (1 - v²_i))
//
// One thread per batch element
// Dispatch: (batch_size, 1, 1)

@group(1) @binding(0) var<storage, read> v2_p: array<f64>;     // [batch × ns]
@group(1) @binding(1) var<storage, read> v2_n: array<f64>;     // [batch × ns]
@group(1) @binding(2) var<storage, read> degs: array<f64>;     // [ns]
@group(1) @binding(3) var<storage, read> delta_p: array<f64>;  // [batch]
@group(1) @binding(4) var<storage, read> delta_n: array<f64>;  // [batch]
@group(1) @binding(5) var<storage, read_write> e_pair_batch: array<f64>; // [batch]

@compute @workgroup_size(256, 1, 1)
fn compute_pairing_energy(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let batch_idx = global_id.x;
    let ns = params.n_states;

    if (batch_idx >= params.batch_size) {
        return;
    }

    let base = batch_idx * ns;
    var e_pair: f64 = f64(0.0);

    // Proton pairing
    let dp = delta_p[batch_idx];
    for (var i = 0u; i < ns; i++) {
        let v2 = v2_p[base + i];
        let u2 = f64(1.0) - v2;
        let vu = max(v2 * u2, f64(0.0));
        e_pair = e_pair - dp * degs[i] * sqrt(vu);
    }

    // Neutron pairing
    let dn = delta_n[batch_idx];
    for (var i = 0u; i < ns; i++) {
        let v2 = v2_n[base + i];
        let u2 = f64(1.0) - v2;
        let vu = max(v2 * u2, f64(0.0));
        e_pair = e_pair - dn * degs[i] * sqrt(vu);
    }

    e_pair_batch[batch_idx] = e_pair;
}
