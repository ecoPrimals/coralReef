// SPDX-License-Identifier: AGPL-3.0-only
//
// Staggered fermion force on GPU (fp64).
//
// Given x = (D†D)⁻¹ φ (from CG) and y = D·x (from Dirac), computes:
//
//   F_f(x,μ) = TA[ U_μ(x) · M(x,μ) ]
//
// where M(x,μ) = η_μ(x)/2 × [X(x+μ̂) ⊗ Y†(x) − Y(x+μ̂) ⊗ X†(x)]
//
// TA = traceless anti-Hermitian projection.
//
// Layout: link_idx = site*4+mu, output force[link_idx*18 .. +18].

struct Params {
    volume: u32,
    pad0: u32,
    pad1: u32,
    pad2: u32,
}

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> links: array<f64>;
@group(0) @binding(2) var<storage, read> x_field: array<f64>;
@group(0) @binding(3) var<storage, read> y_field: array<f64>;
@group(0) @binding(4) var<storage, read> nbr: array<u32>;
@group(0) @binding(5) var<storage, read> phases: array<f64>;
@group(0) @binding(6) var<storage, read_write> force: array<f64>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>, @builtin(num_workgroups) nwg: vec3<u32>) {
    let idx = gid.x + gid.y * nwg.x * 64u;
    let site = idx;
    if site >= params.volume { return; }

    let xp = site * 6u;

    for (var mu: u32 = 0u; mu < 4u; mu = mu + 1u) {
        let link_idx = site * 4u + mu;
        let eta = phases[site * 4u + mu];
        let half_eta = f64(0.5) * eta;

        let fwd = nbr[site * 8u + mu * 2u];
        let fp = fwd * 6u;

        // Build 3×3 outer-product matrix M[a][b]:
        //   M[a][b] = half_eta * (x_fwd[a] * conj(y_here[b]) - y_fwd[a] * conj(x_here[b]))
        // We store M as 18 f64 (re/im interleaved, row-major).
        var m: array<f64, 18>;
        for (var a = 0u; a < 3u; a++) {
            let xa_fwd_re = x_field[fp + a*2u];
            let xa_fwd_im = x_field[fp + a*2u + 1u];
            let ya_fwd_re = y_field[fp + a*2u];
            let ya_fwd_im = y_field[fp + a*2u + 1u];

            for (var b = 0u; b < 3u; b++) {
                let yb_here_re = y_field[xp + b*2u];
                let yb_here_im = y_field[xp + b*2u + 1u];
                let xb_here_re = x_field[xp + b*2u];
                let xb_here_im = x_field[xp + b*2u + 1u];

                // x_fwd[a] * conj(y_here[b])
                let t1_re = xa_fwd_re * yb_here_re + xa_fwd_im * yb_here_im;
                let t1_im = xa_fwd_im * yb_here_re - xa_fwd_re * yb_here_im;

                // y_fwd[a] * conj(x_here[b])
                let t2_re = ya_fwd_re * xb_here_re + ya_fwd_im * xb_here_im;
                let t2_im = ya_fwd_im * xb_here_re - ya_fwd_re * xb_here_im;

                let moff = (a * 3u + b) * 2u;
                m[moff]     = half_eta * (t1_re - t2_re);
                m[moff + 1u] = half_eta * (t1_im - t2_im);
            }
        }

        // Compute W = U_mu(x) * M  (3×3 complex multiply)
        let ul = link_idx * 18u;
        var w: array<f64, 18>;
        for (var i = 0u; i < 3u; i++) {
            for (var j = 0u; j < 3u; j++) {
                var wr: f64 = f64(0.0);
                var wi: f64 = f64(0.0);
                for (var k = 0u; k < 3u; k++) {
                    let u_re = links[ul + (i*3u+k)*2u];
                    let u_im = links[ul + (i*3u+k)*2u + 1u];
                    let m_re = m[(k*3u+j)*2u];
                    let m_im = m[(k*3u+j)*2u + 1u];
                    wr += u_re * m_re - u_im * m_im;
                    wi += u_re * m_im + u_im * m_re;
                }
                w[(i*3u+j)*2u]     = wr;
                w[(i*3u+j)*2u + 1u] = wi;
            }
        }

        // TA projection: W_ta = (W - W†)/2 - Tr(W - W†)/(2·3) · I
        var ta: array<f64, 18>;
        // First compute (W - W†)/2
        for (var i = 0u; i < 3u; i++) {
            for (var j = 0u; j < 3u; j++) {
                let w_ij_re = w[(i*3u+j)*2u];
                let w_ij_im = w[(i*3u+j)*2u + 1u];
                let w_ji_re = w[(j*3u+i)*2u];
                let w_ji_im = w[(j*3u+i)*2u + 1u];
                // (W - W†)[i][j] = W[i][j] - conj(W[j][i])
                let ah_re = f64(0.5) * (w_ij_re - w_ji_re);
                let ah_im = f64(0.5) * (w_ij_im + w_ji_im);
                ta[(i*3u+j)*2u]     = ah_re;
                ta[(i*3u+j)*2u + 1u] = ah_im;
            }
        }

        // Subtract trace/3 from diagonal (traceless)
        let tr_re = (ta[0u] + ta[8u] + ta[16u]) / f64(3.0);
        let tr_im = (ta[1u] + ta[9u] + ta[17u]) / f64(3.0);
        ta[0u]  -= tr_re; ta[1u]  -= tr_im;
        ta[8u]  -= tr_re; ta[9u]  -= tr_im;
        ta[16u] -= tr_re; ta[17u] -= tr_im;

        // Write force output
        let out_base = link_idx * 18u;
        for (var i = 0u; i < 18u; i++) {
            force[out_base + i] = ta[i];
        }
    }
}
