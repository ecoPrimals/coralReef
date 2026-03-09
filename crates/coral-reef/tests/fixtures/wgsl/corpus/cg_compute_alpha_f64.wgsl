// SPDX-License-Identifier: AGPL-3.0-only
//
// CG scalar: alpha = rz / pAp.  Single-thread GPU kernel.
// Keeps the scalar on GPU — no readback for CG internals.

@group(0) @binding(0) var<storage, read> rz: array<f64>;
@group(0) @binding(1) var<storage, read> pap: array<f64>;
@group(0) @binding(2) var<storage, read_write> alpha: array<f64>;

@compute @workgroup_size(1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let rz_val = rz[0];
    let pap_val = pap[0];
    if (abs(pap_val) > f64(1e-30)) {
        alpha[0] = rz_val / pap_val;
    } else {
        alpha[0] = f64(0.0);
    }
}
