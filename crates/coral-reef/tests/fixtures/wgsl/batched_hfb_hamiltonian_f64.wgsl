// SPDX-License-Identifier: AGPL-3.0-only
// Batched HFB Hamiltonian Construction (f64) — GPU Shader
//
// Builds the full Hamiltonian matrix H = T_eff + V for all nuclei in a batch.
// Each thread computes one matrix element H[i,j] for one nucleus by performing
// a radial numerical integration (trapezoidal rule).
//
// This replaces the CPU build_hamiltonian() + build_t_eff() in hfb.rs.
//
// Memory layout:
//   wf: [n_states × nr] f64 — basis wavefunctions (shared across batch)
//   dwf: [n_states × nr] f64 — wavefunction derivatives (shared across batch)
//   u_total_batch: [batch_size × nr] f64 — total potential per nucleus
//   f_q_batch: [batch_size × nr] f64 — effective mass function per nucleus
//   r_grid: [nr] f64 — radial grid
//   lj_same: [n_states × n_states] u32 — 1 if states i,j share (l,j), 0 otherwise
//   H_batch: [batch_size × n_states × n_states] f64 — output Hamiltonians
//
// Algorithm:
//   For T_eff matrix element (same (l,j) block only):
//     T_eff[i,j] = integral( f_q(r) * (dwf_i * dwf_j * r² + l(l+1) * wf_i * wf_j) dr )
//
//   For potential matrix element:
//     Diagonal: V[i,i] = integral( wf_i² * U_total * r² dr )
//     Off-diagonal (same (l,j)): V[i,j] = integral( wf_i * wf_j * U_total * r² dr )
//
//   H[i,j] = T_eff[i,j] + V[i,j]

struct HamiltonianDims {
    n_states: u32,
    nr: u32,
    batch_size: u32,
    _pad: u32,
}

@group(0) @binding(0) var<uniform> dims: HamiltonianDims;
@group(0) @binding(1) var<storage, read> wf_batch: array<f64>;      // [batch × n_states × nr]
@group(0) @binding(2) var<storage, read> dwf_batch: array<f64>;     // [batch × n_states × nr]
@group(0) @binding(3) var<storage, read> u_total_batch: array<f64>; // [batch × nr]
@group(0) @binding(4) var<storage, read> f_q_batch: array<f64>;     // [batch × nr]
@group(0) @binding(5) var<storage, read> r_grid_batch: array<f64>;  // [batch × nr]
@group(0) @binding(6) var<storage, read> lj_same_batch: array<u32>; // [batch × ns × ns]
@group(0) @binding(7) var<storage, read> ll1_batch: array<f64>;     // [batch × ns]
@group(0) @binding(8) var<storage, read_write> H_batch: array<f64>; // [batch × ns × ns]
@group(0) @binding(9) var<storage, read> ham_params: array<f64>;    // [0]=dr

// Compute one matrix element H[batch_idx][i][j]
// Thread index encodes (i, j) pair; batch_idx from z dimension
// Dispatch: (ceil(n_states/16), ceil(n_states/16), batch_size)
@compute @workgroup_size(16, 16, 1)
fn build_hamiltonian(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(workgroup_id) wg_id: vec3<u32>,
) {
    let j_idx = global_id.x;
    let i_idx = global_id.y;
    let batch_idx = wg_id.z;

    let ns = dims.n_states;
    let nr = dims.nr;
    let dr = ham_params[0];

    if (i_idx >= ns || j_idx >= ns || batch_idx >= dims.batch_size) {
        return;
    }

    // Only compute upper triangle + diagonal (symmetric)
    if (j_idx < i_idx) {
        return;
    }

    let pot_base = batch_idx * nr;
    let wf_base = batch_idx * ns * nr;
    let lj_base = batch_idx * ns * ns;
    let ll1_base_idx = batch_idx * ns;
    let h_base = batch_idx * ns * ns;
    let same_lj = lj_same_batch[lj_base + i_idx * ns + j_idx];

    var h_val: f64 = f64(0.0);

    // T_eff contribution (only within same (l,j) block)
    // Uses trapezoidal rule: half-weight first and last grid points
    if (same_lj == 1u) {
        let ll1 = ll1_batch[ll1_base_idx + i_idx];
        var t_eff_sum: f64 = f64(0.0);
        for (var k = 0u; k < nr; k++) {
            let rk = r_grid_batch[pot_base + k];
            let fq = f_q_batch[pot_base + k];
            let wf_i = wf_batch[wf_base + i_idx * nr + k];
            let wf_j = wf_batch[wf_base + j_idx * nr + k];
            let dwf_i = dwf_batch[wf_base + i_idx * nr + k];
            let dwf_j = dwf_batch[wf_base + j_idx * nr + k];

            var val = fq * (dwf_i * dwf_j * rk * rk + ll1 * wf_i * wf_j);
            if (k == 0u || k == nr - 1u) {
                val = val * f64(0.5);
            }
            t_eff_sum = t_eff_sum + val;
        }
        h_val = h_val + t_eff_sum * dr;
    }

    // Potential contribution (trapezoidal rule)
    if (i_idx == j_idx || same_lj == 1u) {
        var v_sum: f64 = f64(0.0);
        for (var k = 0u; k < nr; k++) {
            let rk = r_grid_batch[pot_base + k];
            let u_k = u_total_batch[pot_base + k];
            let wf_i = wf_batch[wf_base + i_idx * nr + k];
            let wf_j = wf_batch[wf_base + j_idx * nr + k];
            var val = wf_i * wf_j * u_k * rk * rk;
            if (k == 0u || k == nr - 1u) {
                val = val * f64(0.5);
            }
            v_sum = v_sum + val;
        }
        h_val = h_val + v_sum * dr;
    }

    H_batch[h_base + i_idx * ns + j_idx] = h_val;
    if (i_idx != j_idx) {
        H_batch[h_base + j_idx * ns + i_idx] = h_val;
    }
}
