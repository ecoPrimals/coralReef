// SPDX-License-Identifier: AGPL-3.0-only
// chi2 contribution per nucleus: (B_calc - B_exp)^2 / sigma^2
@group(0) @binding(0) var<storage, read> b_calc: array<f64>;
@group(0) @binding(1) var<storage, read> b_exp: array<f64>;
@group(0) @binding(2) var<storage, read> sigma: array<f64>;
@group(0) @binding(3) var<storage, read_write> chi2: array<f64>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let idx = gid.x;
    if (idx >= arrayLength(&chi2)) {
        return;
    }
    let diff = b_calc[idx] - b_exp[idx];
    let s = sigma[idx];
    chi2[idx] = diff * diff / (s * s);
}
