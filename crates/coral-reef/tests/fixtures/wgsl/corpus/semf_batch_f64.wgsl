// SPDX-License-Identifier: AGPL-3.0-only
// GPU SEMF: B(Z,N) for N nuclei in parallel using Skyrme-derived coefficients
//
// Inputs:
//   nuclei[7*i+0..6] = Z, N, A^(2/3), A^(1/3), sqrt(A), z_even(0/1), n_even(0/1)
//   nmp[0..3] = a_v, r0, a_a, e2
// Output:
//   energies[i] = B(Z,N) in MeV

@group(0) @binding(0) var<storage, read> nuclei: array<f64>;
@group(0) @binding(1) var<storage, read> nmp: array<f64>;
@group(0) @binding(2) var<storage, read_write> energies: array<f64>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let idx = gid.x;
    if (idx >= arrayLength(&energies)) {
        return;
    }

    let base = 7u * idx;
    let z      = nuclei[base + 0u];
    let n      = nuclei[base + 1u];
    let a_23   = nuclei[base + 2u];  // A^(2/3), precomputed on CPU
    let a_13   = nuclei[base + 3u];  // A^(1/3), precomputed on CPU
    let sqrt_a = nuclei[base + 4u];  // sqrt(A), precomputed on CPU
    let z_even = nuclei[base + 5u];  // 1.0 if even, 0.0 if odd
    let n_even = nuclei[base + 6u];

    let a = z + n;

    let a_v = nmp[0];
    let r0  = nmp[1];
    let a_a = nmp[2];
    let e2  = nmp[3];

    let a_s = a_v * 1.1;
    let a_c = 3.0 * e2 / (5.0 * r0);
    let a_p = 12.0 / sqrt_a;

    // Bethe-Weizsacker mass formula (pure f64 arithmetic, no builtins)
    var b = a_v * a;
    b = b - a_s * a_23;
    b = b - a_c * z * (z - 1.0) / a_13;
    b = b - a_a * (n - z) * (n - z) / a;

    // Pairing: even-even +delta, odd-odd -delta
    if (z_even > 0.5 && n_even > 0.5) {
        b = b + a_p;
    } else if (z_even < 0.5 && n_even < 0.5) {
        b = b - a_p;
    }

    // max(b, 0) without builtin: clamp negative to zero
    if (b < 0.0) {
        b = b - b;  // sets to 0.0 as f64
    }

    energies[idx] = b;
}
