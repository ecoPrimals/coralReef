// SPDX-License-Identifier: AGPL-3.0-only
// Wilson plaquette: per-site sum of 6 planes, Re Tr P_μν / 3.
// Self-contained: no library dependencies. SU(3) ops inline.

struct PlaqParams {
    volume: u32,
    pad0: u32,
    pad1: u32,
    pad2: u32,
}

@group(0) @binding(0) var<uniform> params: PlaqParams;
@group(0) @binding(1) var<storage, read> links: array<f64>;
@group(0) @binding(2) var<storage, read> nbr: array<u32>;
@group(0) @binding(3) var<storage, read_write> out: array<f64>;

// SU(3) as 9 complex entries (re,im pairs), row-major.
// M[row][col] at offset + (row*3 + col)*2

// Re Tr(A*B) where A,B start at offsets oa, ob in links[]
fn re_trace_product(oa: u32, ob: u32) -> f64 {
    var s: f64 = f64(0.0);
    for (var i = 0u; i < 3u; i++) {
        for (var k = 0u; k < 3u; k++) {
            let a_ik_re = links[oa + (i * 3u + k) * 2u];
            let a_ik_im = links[oa + (i * 3u + k) * 2u + 1u];
            let b_ki_re = links[ob + (k * 3u + i) * 2u];
            let b_ki_im = links[ob + (k * 3u + i) * 2u + 1u];
            s += a_ik_re * b_ki_re - a_ik_im * b_ki_im;
        }
    }
    return s;
}

// Compute Re Tr(P_μν(x)) / 3 for plaquette at site, directions mu, nu.
// P = U_mu(x) * U_nu(x+mu) * U_mu†(x+nu) * U_nu†(x)
// We compute this via four matrix multiplications in registers.
fn plaquette_re_tr(site: u32, mu: u32, nu: u32) -> f64 {
    let fwd_mu = nbr[site * 8u + mu * 2u];
    let fwd_nu = nbr[site * 8u + nu * 2u];

    let oa = (site * 4u + mu) * 18u;
    let ob = (fwd_mu * 4u + nu) * 18u;
    let oc = (fwd_nu * 4u + mu) * 18u;
    let od = (site * 4u + nu) * 18u;

    // step1 = A * B
    var s1: array<f64, 18>;
    for (var i = 0u; i < 3u; i++) {
        for (var j = 0u; j < 3u; j++) {
            var re: f64 = f64(0.0);
            var im: f64 = f64(0.0);
            for (var k = 0u; k < 3u; k++) {
                let ar = links[oa + (i*3u+k)*2u];
                let ai = links[oa + (i*3u+k)*2u + 1u];
                let br = links[ob + (k*3u+j)*2u];
                let bi = links[ob + (k*3u+j)*2u + 1u];
                re += ar*br - ai*bi;
                im += ar*bi + ai*br;
            }
            s1[(i*3u+j)*2u] = re;
            s1[(i*3u+j)*2u + 1u] = im;
        }
    }

    // step2 = step1 * C† (adjoint of C)
    var s2: array<f64, 18>;
    for (var i = 0u; i < 3u; i++) {
        for (var j = 0u; j < 3u; j++) {
            var re: f64 = f64(0.0);
            var im: f64 = f64(0.0);
            for (var k = 0u; k < 3u; k++) {
                let ar = s1[(i*3u+k)*2u];
                let ai = s1[(i*3u+k)*2u + 1u];
                // C†[k][j] = conj(C[j][k])
                let br = links[oc + (j*3u+k)*2u];
                let bi = -links[oc + (j*3u+k)*2u + 1u];
                re += ar*br - ai*bi;
                im += ar*bi + ai*br;
            }
            s2[(i*3u+j)*2u] = re;
            s2[(i*3u+j)*2u + 1u] = im;
        }
    }

    // step3 = step2 * D† (adjoint of D)
    var trace_re: f64 = f64(0.0);
    for (var i = 0u; i < 3u; i++) {
        var re: f64 = f64(0.0);
        for (var k = 0u; k < 3u; k++) {
            let ar = s2[(i*3u+k)*2u];
            let ai = s2[(i*3u+k)*2u + 1u];
            // D†[k][i] = conj(D[i][k])
            let br = links[od + (i*3u+k)*2u];
            let bi = -links[od + (i*3u+k)*2u + 1u];
            re += ar*br - ai*bi;
        }
        trace_re += re;
    }

    return trace_re / f64(3.0);
}

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>, @builtin(num_workgroups) nwg: vec3<u32>) {
    let idx = gid.x + gid.y * nwg.x * 64u;
    let site = idx;
    if site >= params.volume { return; }

    var plaq_sum: f64 = f64(0.0);
    for (var mu = 0u; mu < 4u; mu++) {
        for (var nu = mu + 1u; nu < 4u; nu++) {
            plaq_sum += plaquette_re_tr(site, mu, nu);
        }
    }
    out[site] = plaq_sum;
}
