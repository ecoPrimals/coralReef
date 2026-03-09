// SPDX-License-Identifier: AGPL-3.0-only

struct Params {
    volume: u32,
    pad0: u32,
    mass_re: f64,
    hop_sign: f64,
}

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> links: array<f64>;
@group(0) @binding(2) var<storage, read> psi_in: array<f64>;
@group(0) @binding(3) var<storage, read_write> psi_out: array<f64>;
@group(0) @binding(4) var<storage, read> nbr: array<u32>;
@group(0) @binding(5) var<storage, read> phases: array<f64>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>, @builtin(num_workgroups) nwg: vec3<u32>) {
    let idx = gid.x + gid.y * nwg.x * 64u;
    let site = idx;
    if site >= params.volume { return; }

    let psi_base = site * 6u;

    // Mass term: out_c = mass * psi_c
    var or0: f64 = params.mass_re * psi_in[psi_base + 0u];
    var oi0: f64 = params.mass_re * psi_in[psi_base + 1u];
    var or1: f64 = params.mass_re * psi_in[psi_base + 2u];
    var oi1: f64 = params.mass_re * psi_in[psi_base + 3u];
    var or2: f64 = params.mass_re * psi_in[psi_base + 4u];
    var oi2: f64 = params.mass_re * psi_in[psi_base + 5u];

    // 4 hopping directions
    for (var mu: u32 = 0u; mu < 4u; mu = mu + 1u) {
        let half_eta = params.hop_sign * f64(0.5) * phases[site * 4u + mu];

        // ── Forward: U_mu(x) * psi(x+mu) ──
        let fwd = nbr[site * 8u + mu * 2u];
        let fp = fwd * 6u;
        let fl = (site * 4u + mu) * 18u;

        // SU(3) matrix * color vector  (U * psi_fwd)
        // row 0
        var fr0 = links[fl+0u]*psi_in[fp+0u] - links[fl+1u]*psi_in[fp+1u]
                + links[fl+2u]*psi_in[fp+2u] - links[fl+3u]*psi_in[fp+3u]
                + links[fl+4u]*psi_in[fp+4u] - links[fl+5u]*psi_in[fp+5u];
        var fi0 = links[fl+0u]*psi_in[fp+1u] + links[fl+1u]*psi_in[fp+0u]
                + links[fl+2u]*psi_in[fp+3u] + links[fl+3u]*psi_in[fp+2u]
                + links[fl+4u]*psi_in[fp+5u] + links[fl+5u]*psi_in[fp+4u];
        // row 1
        var fr1 = links[fl+6u]*psi_in[fp+0u] - links[fl+7u]*psi_in[fp+1u]
                + links[fl+8u]*psi_in[fp+2u] - links[fl+9u]*psi_in[fp+3u]
                + links[fl+10u]*psi_in[fp+4u] - links[fl+11u]*psi_in[fp+5u];
        var fi1 = links[fl+6u]*psi_in[fp+1u] + links[fl+7u]*psi_in[fp+0u]
                + links[fl+8u]*psi_in[fp+3u] + links[fl+9u]*psi_in[fp+2u]
                + links[fl+10u]*psi_in[fp+5u] + links[fl+11u]*psi_in[fp+4u];
        // row 2
        var fr2 = links[fl+12u]*psi_in[fp+0u] - links[fl+13u]*psi_in[fp+1u]
                + links[fl+14u]*psi_in[fp+2u] - links[fl+15u]*psi_in[fp+3u]
                + links[fl+16u]*psi_in[fp+4u] - links[fl+17u]*psi_in[fp+5u];
        var fi2 = links[fl+12u]*psi_in[fp+1u] + links[fl+13u]*psi_in[fp+0u]
                + links[fl+14u]*psi_in[fp+3u] + links[fl+15u]*psi_in[fp+2u]
                + links[fl+16u]*psi_in[fp+5u] + links[fl+17u]*psi_in[fp+4u];

        // ── Backward: U_mu(x-mu)† * psi(x-mu) ──
        let bwd = nbr[site * 8u + mu * 2u + 1u];
        let bp = bwd * 6u;
        let bl = (bwd * 4u + mu) * 18u;

        // U† * v : result_c = Σ_cp conj(U[cp][c]) * v[cp]
        // row 0: sum over cp of conj(U[cp][0]) * psi_bwd[cp]
        var br0 = links[bl+0u]*psi_in[bp+0u] + links[bl+1u]*psi_in[bp+1u]
                + links[bl+6u]*psi_in[bp+2u] + links[bl+7u]*psi_in[bp+3u]
                + links[bl+12u]*psi_in[bp+4u] + links[bl+13u]*psi_in[bp+5u];
        var bi0 = links[bl+0u]*psi_in[bp+1u] - links[bl+1u]*psi_in[bp+0u]
                + links[bl+6u]*psi_in[bp+3u] - links[bl+7u]*psi_in[bp+2u]
                + links[bl+12u]*psi_in[bp+5u] - links[bl+13u]*psi_in[bp+4u];
        // row 1
        var br1 = links[bl+2u]*psi_in[bp+0u] + links[bl+3u]*psi_in[bp+1u]
                + links[bl+8u]*psi_in[bp+2u] + links[bl+9u]*psi_in[bp+3u]
                + links[bl+14u]*psi_in[bp+4u] + links[bl+15u]*psi_in[bp+5u];
        var bi1 = links[bl+2u]*psi_in[bp+1u] - links[bl+3u]*psi_in[bp+0u]
                + links[bl+8u]*psi_in[bp+3u] - links[bl+9u]*psi_in[bp+2u]
                + links[bl+14u]*psi_in[bp+5u] - links[bl+15u]*psi_in[bp+4u];
        // row 2
        var br2 = links[bl+4u]*psi_in[bp+0u] + links[bl+5u]*psi_in[bp+1u]
                + links[bl+10u]*psi_in[bp+2u] + links[bl+11u]*psi_in[bp+3u]
                + links[bl+16u]*psi_in[bp+4u] + links[bl+17u]*psi_in[bp+5u];
        var bi2 = links[bl+4u]*psi_in[bp+1u] - links[bl+5u]*psi_in[bp+0u]
                + links[bl+10u]*psi_in[bp+3u] - links[bl+11u]*psi_in[bp+2u]
                + links[bl+16u]*psi_in[bp+5u] - links[bl+17u]*psi_in[bp+4u];

        // Accumulate: out += half_eta * (fwd - bwd)
        or0 = or0 + half_eta * (fr0 - br0);
        oi0 = oi0 + half_eta * (fi0 - bi0);
        or1 = or1 + half_eta * (fr1 - br1);
        oi1 = oi1 + half_eta * (fi1 - bi1);
        or2 = or2 + half_eta * (fr2 - br2);
        oi2 = oi2 + half_eta * (fi2 - bi2);
    }

    psi_out[psi_base + 0u] = or0;
    psi_out[psi_base + 1u] = oi0;
    psi_out[psi_base + 2u] = or1;
    psi_out[psi_base + 3u] = oi1;
    psi_out[psi_base + 4u] = or2;
    psi_out[psi_base + 5u] = oi2;
}
