// SPDX-License-Identifier: AGPL-3.0-only
// Link update: U = exp(dt * P) * U via Cayley + reunitarize.
// Cayley: exp(A) ≈ (I + A/2)(I - A/2)^{-1}, exact for anti-Hermitian.
// Self-contained: all SU(3) ops inline.

struct LinkParams {
    n_links: u32,
    pad0: u32,
    dt: f64,
}

@group(0) @binding(0) var<uniform> params: LinkParams;
@group(0) @binding(1) var<storage, read> momenta: array<f64>;
@group(0) @binding(2) var<storage, read_write> links: array<f64>;

fn mul33(a_in: array<f64, 18>, b_in: array<f64, 18>) -> array<f64, 18> {
    var a = a_in;
    var b = b_in;
    var r: array<f64, 18>;
    for (var i = 0u; i < 3u; i++) {
        for (var j = 0u; j < 3u; j++) {
            var re: f64 = f64(0.0);
            var im: f64 = f64(0.0);
            for (var k = 0u; k < 3u; k++) {
                let ik = (i*3u+k)*2u;
                let kj = (k*3u+j)*2u;
                re += a[ik]*b[kj] - a[ik+1u]*b[kj+1u];
                im += a[ik]*b[kj+1u] + a[ik+1u]*b[kj];
            }
            r[(i*3u+j)*2u] = re;
            r[(i*3u+j)*2u+1u] = im;
        }
    }
    return r;
}

fn inv33(a_in: array<f64, 18>) -> array<f64, 18> {
    var a = a_in;

    // Cofactors row 0
    let c00r = a[8]*a[16] - a[9]*a[17] - (a[10]*a[14] - a[11]*a[15]);
    let c00i = a[8]*a[17] + a[9]*a[16] - (a[10]*a[15] + a[11]*a[14]);
    let c01r = a[10]*a[12] - a[11]*a[13] - (a[6]*a[16] - a[7]*a[17]);
    let c01i = a[10]*a[13] + a[11]*a[12] - (a[6]*a[17] + a[7]*a[16]);
    let c02r = a[6]*a[14] - a[7]*a[15] - (a[8]*a[12] - a[9]*a[13]);
    let c02i = a[6]*a[15] + a[7]*a[14] - (a[8]*a[13] + a[9]*a[12]);

    let dr = a[0]*c00r - a[1]*c00i + a[2]*c01r - a[3]*c01i + a[4]*c02r - a[5]*c02i;
    let di = a[0]*c00i + a[1]*c00r + a[2]*c01i + a[3]*c01r + a[4]*c02i + a[5]*c02r;
    let d2 = dr*dr + di*di;
    let inv_dr =  dr / d2;
    let inv_di = -di / d2;

    let c10r = a[4]*a[14] - a[5]*a[15] - (a[2]*a[16] - a[3]*a[17]);
    let c10i = a[4]*a[15] + a[5]*a[14] - (a[2]*a[17] + a[3]*a[16]);
    let c11r = a[0]*a[16] - a[1]*a[17] - (a[4]*a[12] - a[5]*a[13]);
    let c11i = a[0]*a[17] + a[1]*a[16] - (a[4]*a[13] + a[5]*a[12]);
    let c12r = a[2]*a[12] - a[3]*a[13] - (a[0]*a[14] - a[1]*a[15]);
    let c12i = a[2]*a[13] + a[3]*a[12] - (a[0]*a[15] + a[1]*a[14]);
    let c20r = a[2]*a[10] - a[3]*a[11] - (a[4]*a[8] - a[5]*a[9]);
    let c20i = a[2]*a[11] + a[3]*a[10] - (a[4]*a[9] + a[5]*a[8]);
    let c21r = a[4]*a[6] - a[5]*a[7] - (a[0]*a[10] - a[1]*a[11]);
    let c21i = a[4]*a[7] + a[5]*a[6] - (a[0]*a[11] + a[1]*a[10]);
    let c22r = a[0]*a[8] - a[1]*a[9] - (a[2]*a[6] - a[3]*a[7]);
    let c22i = a[0]*a[9] + a[1]*a[8] - (a[2]*a[7] + a[3]*a[6]);

    var r: array<f64, 18>;
    r[0]  = c00r*inv_dr - c00i*inv_di; r[1]  = c00r*inv_di + c00i*inv_dr;
    r[2]  = c10r*inv_dr - c10i*inv_di; r[3]  = c10r*inv_di + c10i*inv_dr;
    r[4]  = c20r*inv_dr - c20i*inv_di; r[5]  = c20r*inv_di + c20i*inv_dr;
    r[6]  = c01r*inv_dr - c01i*inv_di; r[7]  = c01r*inv_di + c01i*inv_dr;
    r[8]  = c11r*inv_dr - c11i*inv_di; r[9]  = c11r*inv_di + c11i*inv_dr;
    r[10] = c21r*inv_dr - c21i*inv_di; r[11] = c21r*inv_di + c21i*inv_dr;
    r[12] = c02r*inv_dr - c02i*inv_di; r[13] = c02r*inv_di + c02i*inv_dr;
    r[14] = c12r*inv_dr - c12i*inv_di; r[15] = c12r*inv_di + c12i*inv_dr;
    r[16] = c22r*inv_dr - c22i*inv_di; r[17] = c22r*inv_di + c22i*inv_dr;
    return r;
}

fn reunitarize(u_in: array<f64, 18>) -> array<f64, 18> {
    var u = u_in;
    var r: array<f64, 18>;

    var n0: f64 = f64(0.0);
    for (var j = 0u; j < 3u; j++) { n0 += u[j*2u]*u[j*2u] + u[j*2u+1u]*u[j*2u+1u]; }
    n0 = sqrt(n0);
    if n0 < f64(1e-30) { n0 = f64(1.0); }
    let inv0 = f64(1.0) / n0;
    for (var j = 0u; j < 3u; j++) { r[j*2u] = u[j*2u]*inv0; r[j*2u+1u] = u[j*2u+1u]*inv0; }

    var dot_re: f64 = f64(0.0);
    var dot_im: f64 = f64(0.0);
    for (var j = 0u; j < 3u; j++) {
        dot_re += r[j*2u]*u[6u+j*2u] + r[j*2u+1u]*u[6u+j*2u+1u];
        dot_im += r[j*2u]*u[6u+j*2u+1u] - r[j*2u+1u]*u[6u+j*2u];
    }
    var row1: array<f64, 6>;
    for (var j = 0u; j < 3u; j++) {
        row1[j*2u]   = u[6u+j*2u]   - (dot_re*r[j*2u] - dot_im*r[j*2u+1u]);
        row1[j*2u+1u] = u[6u+j*2u+1u] - (dot_re*r[j*2u+1u] + dot_im*r[j*2u]);
    }

    var n1: f64 = f64(0.0);
    for (var j = 0u; j < 3u; j++) { n1 += row1[j*2u]*row1[j*2u] + row1[j*2u+1u]*row1[j*2u+1u]; }
    n1 = sqrt(n1);
    if n1 < f64(1e-30) { n1 = f64(1.0); }
    let inv1 = f64(1.0) / n1;
    for (var j = 0u; j < 3u; j++) { r[6u+j*2u] = row1[j*2u]*inv1; r[6u+j*2u+1u] = row1[j*2u+1u]*inv1; }

    // Row 2 = conj(row0 × row1)
    let r0_0r = r[0]; let r0_0i = r[1];
    let r0_1r = r[2]; let r0_1i = r[3];
    let r0_2r = r[4]; let r0_2i = r[5];
    let r1_0r = r[6]; let r1_0i = r[7];
    let r1_1r = r[8]; let r1_1i = r[9];
    let r1_2r = r[10]; let r1_2i = r[11];

    let x0r = r0_1r*r1_2r - r0_1i*r1_2i - (r0_2r*r1_1r - r0_2i*r1_1i);
    let x0i = r0_1r*r1_2i + r0_1i*r1_2r - (r0_2r*r1_1i + r0_2i*r1_1r);
    r[12] = x0r; r[13] = -x0i;

    let x1r = r0_2r*r1_0r - r0_2i*r1_0i - (r0_0r*r1_2r - r0_0i*r1_2i);
    let x1i = r0_2r*r1_0i + r0_2i*r1_0r - (r0_0r*r1_2i + r0_0i*r1_2r);
    r[14] = x1r; r[15] = -x1i;

    let x2r = r0_0r*r1_1r - r0_0i*r1_1i - (r0_1r*r1_0r - r0_1i*r1_0i);
    let x2i = r0_0r*r1_1i + r0_0i*r1_1r - (r0_1r*r1_0i + r0_1i*r1_0r);
    r[16] = x2r; r[17] = -x2i;

    return r;
}

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>, @builtin(num_workgroups) nwg: vec3<u32>) {
    let idx = gid.x + gid.y * nwg.x * 64u;
    let link_idx = idx;
    if link_idx >= params.n_links { return; }

    let base_p = link_idx * 18u;
    let base_u = link_idx * 18u;

    var p: array<f64, 18>;
    for (var i = 0u; i < 18u; i++) { p[i] = momenta[base_p + i]; }
    var u: array<f64, 18>;
    for (var i = 0u; i < 18u; i++) { u[i] = links[base_u + i]; }

    let half_dt = params.dt * f64(0.5);

    var plus:  array<f64, 18>;
    var minus: array<f64, 18>;
    for (var i = 0u; i < 18u; i++) {
        let h = p[i] * half_dt;
        plus[i] = h;
        minus[i] = -h;
    }
    plus[0]  += f64(1.0); plus[8]  += f64(1.0); plus[16] += f64(1.0);
    minus[0] += f64(1.0); minus[8] += f64(1.0); minus[16]+= f64(1.0);

    var inv_m = inv33(minus);
    var exp_p = mul33(plus, inv_m);
    var new_u = reunitarize(mul33(exp_p, u));

    for (var i = 0u; i < 18u; i++) { links[base_u + i] = new_u[i]; }
}
