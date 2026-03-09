// SPDX-License-Identifier: AGPL-3.0-only
//
// Combined spin-orbit diagonal add + matrix packing for eigensolve (f64).
//
// Reads H matrices from per-group buffer (ns × ns layout),
// adds spin-orbit diagonal corrections, and writes to the packed
// eigensolve buffer (gns × gns layout) with padding.
//
// For padding rows/columns (r >= ns or c >= ns):
//   diagonal: 1e10 (large eigenvalue — sorted last, filtered by extraction)
//   off-diagonal: 0.0
//
// Dispatch: (ceil(gns/16), ceil(gns/16), n_active)

struct PackParams {
    ns: u32,           // source matrix dimension (per-group)
    gns: u32,          // destination matrix dimension (global_max_ns)
    n_active: u32,     // number of matrices to pack
    dst_start: u32,    // starting matrix index in destination buffer
    dst_stride: u32,   // stride between consecutive matrices in destination
    _pad1: u32,
    _pad2: u32,
    _pad3: u32,
}

@group(0) @binding(0) var<uniform> params: PackParams;
@group(0) @binding(1) var<storage, read> h_src: array<f64>;
@group(0) @binding(2) var<storage, read> so_diag: array<f64>;
@group(0) @binding(3) var<storage, read_write> h_dst: array<f64>;

@compute @workgroup_size(16, 16, 1)
fn pack_with_spinorbit(
    @builtin(global_invocation_id) gid: vec3<u32>,
) {
    let r = gid.x;
    let c = gid.y;
    let bi = gid.z;

    let ns = params.ns;
    let gns = params.gns;

    if r >= gns || c >= gns || bi >= params.n_active { return; }

    let dst_mat_idx = params.dst_start + bi * params.dst_stride;
    let dst_offset = dst_mat_idx * gns * gns;
    let dst_idx = dst_offset + r * gns + c;

    if r < ns && c < ns {
        let src_mat = bi * ns * ns;
        var val = h_src[src_mat + r * ns + c];
        if r == c {
            val = val + so_diag[bi * ns + r];
        }
        h_dst[dst_idx] = val;
    } else if r == c {
        h_dst[dst_idx] = f64(1e10);
    } else {
        h_dst[dst_idx] = f64(0.0);
    }
}
