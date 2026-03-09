// SPDX-License-Identifier: AGPL-3.0-or-later
//
// backbone_update_f64.wgsl — Backbone frame composition with df64 core streaming
//
// Updates backbone frames by composing current frames with predicted
// delta transforms (quaternion + translation). Each residue gets an
// independent frame update:
//
//   T_new[i] = T_current[i] compose T_delta[i]
//   R_new = R_cur @ quat_to_rot(delta_q)
//   t_new = R_cur @ delta_t + t_cur
//
// One thread per residue. Three-zone core streaming: f64 buffer I/O,
// df64 compute, f64 output.
//
// Layouts:
//   delta_quats:    [N, 4]  (w, x, y, z — will be normalized)
//   delta_trans:    [N, 3]
//   current_frames: [N, 12] (rot 3x3 row-major + trans 3)
//   output_frames:  [N, 12]
//
// Absorption target: barracuda::ops::backbone_update_f64
// Requires: df64_core.wgsl + df64_transcendentals.wgsl (prepended via compile_shader_f64)

struct Params {
    n_res: u32,
    _p0:   u32,
    _p1:   u32,
    _p2:   u32,
}

@group(0) @binding(0) var<storage, read>       delta_quats:    array<f64>;
@group(0) @binding(1) var<storage, read>       delta_trans:    array<f64>;
@group(0) @binding(2) var<storage, read>       current_frames: array<f64>;
@group(0) @binding(3) var<storage, read_write> output_frames:  array<f64>;
@group(0) @binding(4) var<uniform>             params:         Params;

@compute @workgroup_size(256)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if i >= params.n_res { return; }

    // Zone 1: Load quaternion from f64 → df64
    let qb = i * 4u;
    let qw = df64_from_f64(delta_quats[qb]);
    let qx = df64_from_f64(delta_quats[qb + 1u]);
    let qy = df64_from_f64(delta_quats[qb + 2u]);
    let qz = df64_from_f64(delta_quats[qb + 3u]);

    // Zone 2: Normalize quaternion with sqrt_df64
    let norm_sq = df64_add(df64_add(df64_mul(qw, qw), df64_mul(qx, qx)),
                           df64_add(df64_mul(qy, qy), df64_mul(qz, qz)));
    let norm = sqrt_df64(norm_sq);
    let inv_n = df64_div(df64_from_f32(1.0), norm);
    let w = df64_mul(qw, inv_n);
    let x = df64_mul(qx, inv_n);
    let y = df64_mul(qy, inv_n);
    let z = df64_mul(qz, inv_n);

    // Quaternion to rotation (delta) in df64
    let two = df64_from_f32(2.0);
    let one = df64_from_f32(1.0);
    var dr: array<Df64, 9>;
    dr[0] = df64_sub(one, df64_mul(two, df64_add(df64_mul(y, y), df64_mul(z, z))));
    dr[1] = df64_mul(two, df64_sub(df64_mul(x, y), df64_mul(w, z)));
    dr[2] = df64_mul(two, df64_add(df64_mul(x, z), df64_mul(w, y)));
    dr[3] = df64_mul(two, df64_add(df64_mul(x, y), df64_mul(w, z)));
    dr[4] = df64_sub(one, df64_mul(two, df64_add(df64_mul(x, x), df64_mul(z, z))));
    dr[5] = df64_mul(two, df64_sub(df64_mul(y, z), df64_mul(w, x)));
    dr[6] = df64_mul(two, df64_sub(df64_mul(x, z), df64_mul(w, y)));
    dr[7] = df64_mul(two, df64_add(df64_mul(y, z), df64_mul(w, x)));
    dr[8] = df64_sub(one, df64_mul(two, df64_add(df64_mul(x, x), df64_mul(y, y))));

    // Load current frame from f64 → df64
    let fb = i * 12u;
    var cr: array<Df64, 9>;
    for (var k = 0u; k < 9u; k++) {
        cr[k] = df64_from_f64(current_frames[fb + k]);
    }
    let ctx = df64_from_f64(current_frames[fb + 9u]);
    let cty = df64_from_f64(current_frames[fb + 10u]);
    let ctz = df64_from_f64(current_frames[fb + 11u]);

    // Compose rotation: R_new = R_cur @ R_delta (df64 matrix multiply)
    var nr: array<Df64, 9>;
    for (var row = 0u; row < 3u; row++) {
        for (var col = 0u; col < 3u; col++) {
            var acc = df64_zero();
            for (var k = 0u; k < 3u; k++) {
                acc = df64_add(acc, df64_mul(cr[row * 3u + k], dr[k * 3u + col]));
            }
            nr[row * 3u + col] = acc;
        }
    }

    // Compose translation: t_new = R_cur @ delta_t + t_cur (df64)
    let tb = i * 3u;
    let dtx = df64_from_f64(delta_trans[tb]);
    let dty = df64_from_f64(delta_trans[tb + 1u]);
    let dtz = df64_from_f64(delta_trans[tb + 2u]);

    let ntx = df64_add(df64_add(df64_mul(cr[0], dtx), df64_add(df64_mul(cr[1], dty), df64_mul(cr[2], dtz))), ctx);
    let nty = df64_add(df64_add(df64_mul(cr[3], dtx), df64_add(df64_mul(cr[4], dty), df64_mul(cr[5], dtz))), cty);
    let ntz = df64_add(df64_add(df64_mul(cr[6], dtx), df64_add(df64_mul(cr[7], dty), df64_mul(cr[8], dtz))), ctz);

    // Zone 3: df64 → f64
    let ob = i * 12u;
    for (var k = 0u; k < 9u; k++) {
        output_frames[ob + k] = df64_to_f64(nr[k]);
    }
    output_frames[ob + 9u] = df64_to_f64(ntx);
    output_frames[ob + 10u] = df64_to_f64(nty);
    output_frames[ob + 11u] = df64_to_f64(ntz);
}
