// SPDX-License-Identifier: AGPL-3.0-only
//
// CG scalar: beta = rz_new / rz_old, then rz_old ← rz_new.
// Single-thread GPU kernel — no readback for CG internals.

@group(0) @binding(0) var<storage, read> rz_new: array<f64>;
@group(0) @binding(1) var<storage, read_write> rz_old: array<f64>;
@group(0) @binding(2) var<storage, read_write> beta: array<f64>;

@compute @workgroup_size(1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let rz_new_val = rz_new[0];
    let rz_old_val = rz_old[0];
    if (abs(rz_old_val) > f64(1e-30)) {
        beta[0] = rz_new_val / rz_old_val;
    } else {
        beta[0] = f64(0.0);
    }
    rz_old[0] = rz_new_val;
}
