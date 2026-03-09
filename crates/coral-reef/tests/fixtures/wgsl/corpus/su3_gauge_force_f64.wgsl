// SPDX-License-Identifier: AGPL-3.0-only
// SU(3) gauge force: staple sum + traceless anti-Hermitian projection.
// F(x,mu) = -(beta/3) * proj_TA( U_mu(x) * staple(x,mu) )
// Self-contained: SU(3) matrix ops inline.

struct ForceParams {
    volume: u32,
    pad0: u32,
    beta: f64,
}

@group(0) @binding(0) var<uniform> params: ForceParams;
@group(0) @binding(1) var<storage, read> links: array<f64>;
@group(0) @binding(2) var<storage, read> nbr: array<u32>;
@group(0) @binding(3) var<storage, read_write> force: array<f64>;

fn load_su3(off: u32) -> array<f64, 18> {
    var m: array<f64, 18>;
    for (var i = 0u; i < 18u; i++) { m[i] = links[off + i]; }
    return m;
}

fn mul_su3(a_in: array<f64, 18>, b_in: array<f64, 18>) -> array<f64, 18> {
    var a = a_in;
    var b = b_in;
    var r: array<f64, 18>;
    for (var i = 0u; i < 3u; i++) {
        for (var j = 0u; j < 3u; j++) {
            var re: f64 = f64(0.0);
            var im: f64 = f64(0.0);
            for (var k = 0u; k < 3u; k++) {
                let idx_ik = (i*3u+k)*2u;
                let idx_kj = (k*3u+j)*2u;
                re += a[idx_ik]*b[idx_kj] - a[idx_ik+1u]*b[idx_kj+1u];
                im += a[idx_ik]*b[idx_kj+1u] + a[idx_ik+1u]*b[idx_kj];
            }
            r[(i*3u+j)*2u] = re;
            r[(i*3u+j)*2u+1u] = im;
        }
    }
    return r;
}

fn adj_su3(a_in: array<f64, 18>) -> array<f64, 18> {
    var a = a_in;
    var r: array<f64, 18>;
    for (var i = 0u; i < 3u; i++) {
        for (var j = 0u; j < 3u; j++) {
            r[(i*3u+j)*2u] = a[(j*3u+i)*2u];
            r[(i*3u+j)*2u+1u] = -a[(j*3u+i)*2u+1u];
        }
    }
    return r;
}

fn add_su3(a_in: array<f64, 18>, b_in: array<f64, 18>) -> array<f64, 18> {
    var a = a_in;
    var b = b_in;
    var r: array<f64, 18>;
    for (var i = 0u; i < 18u; i++) { r[i] = a[i] + b[i]; }
    return r;
}

fn zero_su3() -> array<f64, 18> {
    var r: array<f64, 18>;
    for (var i = 0u; i < 18u; i++) { r[i] = f64(0.0); }
    return r;
}

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>, @builtin(num_workgroups) nwg: vec3<u32>) {
    let idx = gid.x + gid.y * nwg.x * 64u;
    let link_idx = idx;
    let n_links = params.volume * 4u;
    if link_idx >= n_links { return; }

    let site = link_idx / 4u;
    let mu = link_idx % 4u;

    let u_mu_x = load_su3(link_idx * 18u);
    var staple = zero_su3();

    for (var nu = 0u; nu < 4u; nu++) {
        if nu == mu { continue; }

        let fwd_mu = nbr[site * 8u + mu * 2u];
        let fwd_nu = nbr[site * 8u + nu * 2u];
        let bwd_nu = nbr[site * 8u + nu * 2u + 1u];

        // Upper: U_nu(x+mu) * U_mu†(x+nu) * U_nu†(x)
        let u_nu_xmu = load_su3((fwd_mu * 4u + nu) * 18u);
        let u_mu_xnu = load_su3((fwd_nu * 4u + mu) * 18u);
        let u_nu_x   = load_su3((site * 4u + nu) * 18u);
        let upper = mul_su3(mul_su3(u_nu_xmu, adj_su3(u_mu_xnu)), adj_su3(u_nu_x));

        // Lower: U_nu†(x+mu-nu) * U_mu†(x-nu) * U_nu(x-nu)
        let fwd_mu_bwd_nu = nbr[fwd_mu * 8u + nu * 2u + 1u];
        let u_nu_xmubnu   = load_su3((fwd_mu_bwd_nu * 4u + nu) * 18u);
        let u_mu_xbnu     = load_su3((bwd_nu * 4u + mu) * 18u);
        let u_nu_xbnu     = load_su3((bwd_nu * 4u + nu) * 18u);
        let lower = mul_su3(mul_su3(adj_su3(u_nu_xmubnu), adj_su3(u_mu_xbnu)), u_nu_xbnu);

        staple = add_su3(staple, add_su3(upper, lower));
    }

    // W = U * staple
    var w = mul_su3(u_mu_x, staple);

    // Anti-Hermitian traceless projection
    var wdag = adj_su3(w);
    var diff: array<f64, 18>;
    for (var i = 0u; i < 18u; i++) { diff[i] = (w[i] - wdag[i]) * f64(0.5); }

    var tr_re: f64 = diff[0] + diff[8] + diff[16];
    var tr_im: f64 = diff[1] + diff[9] + diff[17];
    let tr3_re = tr_re / f64(3.0);
    let tr3_im = tr_im / f64(3.0);

    var proj: array<f64, 18>;
    for (var i = 0u; i < 18u; i++) { proj[i] = diff[i]; }
    proj[0]  -= tr3_re; proj[1]  -= tr3_im;
    proj[8]  -= tr3_re; proj[9]  -= tr3_im;
    proj[16] -= tr3_re; proj[17] -= tr3_im;

    let s = -params.beta / f64(3.0);
    for (var i = 0u; i < 18u; i++) {
        force[link_idx * 18u + i] = proj[i] * s;
    }
}
