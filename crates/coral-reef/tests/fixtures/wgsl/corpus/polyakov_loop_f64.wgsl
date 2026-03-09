// SPDX-License-Identifier: AGPL-3.0-only
// polyakov_loop_f64.wgsl — GPU Polyakov loop (temporal Wilson line)
//
// Prepend: complex_f64.wgsl + su3_f64.wgsl
//
// Cross-spring evolution:
//   toadStool polyakov_loop_f64.wgsl → hotSpring v0.6.13 absorption
//   Eliminates CPU readback; full GPU-resident observable.
//
// Computes L(x) = Tr(∏_{t=0}^{Nt-1} U_3(t,x,y,z)) / 3  per spatial site.
// Output: (Re, Im) per spatial site.
//
// Bindings:
//   @binding(0) uniform params  — lattice dimensions
//   @binding(1) storage links   — [V × 4 × 18] f64
//   @binding(2) storage poly    — [spatial_vol × 2] f64 (Re, Im)

struct PolyParams {
    nt:          u32,
    nx:          u32,
    ny:          u32,
    nz:          u32,
    volume:      u32,
    spatial_vol: u32,
    _pad0:       u32,
    _pad1:       u32,
}

@group(0) @binding(0) var<uniform>             params: PolyParams;
@group(0) @binding(1) var<storage, read>       links:  array<f64>;
@group(0) @binding(2) var<storage, read_write> poly:   array<f64>;

fn load_link(site: u32, mu: u32) -> array<Complex64, 9> {
    var m: array<Complex64, 9>;
    let base = (site * 4u + mu) * 18u;
    for (var i = 0u; i < 9u; i = i + 1u) {
        let off = base + i * 2u;
        m[i] = c64_new(links[off], links[off + 1u]);
    }
    return m;
}

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let spatial_idx = gid.x;
    if (spatial_idx >= params.spatial_vol) { return; }

    // Decompose spatial index → (ix, iy, iz) for t-major ordering:
    //   site = t * (nx*ny*nz) + ix * (ny*nz) + iy * nz + iz
    let nyz = params.ny * params.nz;
    let ix  = spatial_idx / nyz;
    let rem = spatial_idx % nyz;
    let iy  = rem / params.nz;
    let iz  = rem % params.nz;

    var prod = su3_identity();
    for (var t = 0u; t < params.nt; t = t + 1u) {
        let site = t * (params.nx * params.ny * params.nz)
                 + ix * (params.ny * params.nz)
                 + iy * params.nz
                 + iz;
        let u_t = load_link(site, 3u);
        prod = su3_mul(prod, u_t);
    }

    let tr = su3_trace(prod);
    let result = c64_scale(tr, f64(1.0) / f64(3.0));
    poly[spatial_idx * 2u]      = result.re;
    poly[spatial_idx * 2u + 1u] = result.im;
}
