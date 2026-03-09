// SPDX-License-Identifier: AGPL-3.0-only
// Kinetic energy: T_link = -0.5 * Re Tr(P^2) per link.
// Self-contained: SU(3) multiply inline.

struct KeParams {
    n_links: u32,
    pad0: u32,
    pad1: u32,
    pad2: u32,
}

@group(0) @binding(0) var<uniform> params: KeParams;
@group(0) @binding(1) var<storage, read> momenta: array<f64>;
@group(0) @binding(2) var<storage, read_write> out: array<f64>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>, @builtin(num_workgroups) nwg: vec3<u32>) {
    let idx = gid.x + gid.y * nwg.x * 64u;
    let link_idx = idx;
    if link_idx >= params.n_links { return; }

    let base = link_idx * 18u;

    // Compute Re Tr(P²) = Σ_i Re( Σ_k P[i][k] * P[k][i] )
    var trace_re: f64 = f64(0.0);
    for (var i = 0u; i < 3u; i++) {
        for (var k = 0u; k < 3u; k++) {
            let p_ik_re = momenta[base + (i*3u+k)*2u];
            let p_ik_im = momenta[base + (i*3u+k)*2u + 1u];
            let p_ki_re = momenta[base + (k*3u+i)*2u];
            let p_ki_im = momenta[base + (k*3u+i)*2u + 1u];
            trace_re += p_ik_re * p_ki_re - p_ik_im * p_ki_im;
        }
    }

    out[link_idx] = f64(-0.5) * trace_re;
}
