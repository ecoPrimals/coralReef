// SPDX-License-Identifier: AGPL-3.0-or-later
//
// ipa_scores_f64.wgsl — Invariant Point Attention score computation
//
// Algorithm 22 from Jumper et al. (2021). Computes the three-term IPA logit:
//
//   a[h,i,j] = w_L * sum_d Q[i,h,d]*K[j,h,d] / sqrt(c)
//            + w_C * pair_bias[h,i,j]
//            + w_P * (-gamma/2) * sum_p ||T_i(q_p) - T_j(k_p)||^2
//
// The point distance term projects query/key points through backbone frames,
// making the attention score depend on 3D proximity — the key SE(3)-equivariant
// property that distinguishes IPA from standard attention.
//
// Three-zone core streaming: f64 buffer I/O, df64 compute, f64 output.
//
// Layouts:
//   q_scalar/k_scalar: [N, H, D]  (residue, head, dim)
//   pair_bias:         [H, N, N]
//   q_points/k_points: [N, H, P, 3] (local frame coordinates)
//   frames:            [N, 12]  (rotation 3x3 row-major + translation 3)
//   output:            [H, N, N]
//
// Absorption target: barracuda::ops::ipa_scores_f64
// Requires: df64_core.wgsl + df64_transcendentals.wgsl (prepended via compile_shader_f64)

struct Params {
    n_res:    u32,
    n_heads:  u32,
    head_dim: u32,
    n_points: u32,
    w_l:      f32,
    w_c:      f32,
    w_p:      f32,
    gamma:    f32,
}

@group(0) @binding(0) var<storage, read>       q_scalar:  array<f64>;
@group(0) @binding(1) var<storage, read>       k_scalar:  array<f64>;
@group(0) @binding(2) var<storage, read>       pair_bias: array<f64>;
@group(0) @binding(3) var<storage, read>       q_points:  array<f64>;
@group(0) @binding(4) var<storage, read>       k_points:  array<f64>;
@group(0) @binding(5) var<storage, read>       frames:    array<f64>;
@group(0) @binding(6) var<storage, read_write> scores:    array<f64>;
@group(0) @binding(7) var<uniform>             params:    Params;

fn apply_frame_df64(frame_idx: u32, px: Df64, py: Df64, pz: Df64) -> array<Df64, 3> {
    let b = frame_idx * 12u;
    let r00 = df64_from_f64(frames[b + 0u]);
    let r01 = df64_from_f64(frames[b + 1u]);
    let r02 = df64_from_f64(frames[b + 2u]);
    let r10 = df64_from_f64(frames[b + 3u]);
    let r11 = df64_from_f64(frames[b + 4u]);
    let r12 = df64_from_f64(frames[b + 5u]);
    let r20 = df64_from_f64(frames[b + 6u]);
    let r21 = df64_from_f64(frames[b + 7u]);
    let r22 = df64_from_f64(frames[b + 8u]);
    let tx  = df64_from_f64(frames[b + 9u]);
    let ty  = df64_from_f64(frames[b + 10u]);
    let tz  = df64_from_f64(frames[b + 11u]);

    let rx = df64_add(df64_add(df64_mul(r00, px), df64_mul(r01, py)),
                      df64_add(df64_mul(r02, pz), tx));
    let ry = df64_add(df64_add(df64_mul(r10, px), df64_mul(r11, py)),
                      df64_add(df64_mul(r12, pz), ty));
    let rz = df64_add(df64_add(df64_mul(r20, px), df64_mul(r21, py)),
                      df64_add(df64_mul(r22, pz), tz));
    return array<Df64, 3>(rx, ry, rz);
}

@compute @workgroup_size(256)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let N = params.n_res;
    let H = params.n_heads;
    let D = params.head_dim;
    let P = params.n_points;

    let total = H * N * N;
    let idx = gid.x;
    if idx >= total { return; }

    let h    = idx / (N * N);
    let rem  = idx % (N * N);
    let i    = rem / N;
    let j    = rem % N;

    // Term 1: scalar attention with df64 dot product (Zone 1+2)
    var scalar_acc = df64_zero();
    for (var d = 0u; d < D; d++) {
        let qi = df64_from_f64(q_scalar[(i * H + h) * D + d]);
        let ki = df64_from_f64(k_scalar[(j * H + h) * D + d]);
        scalar_acc = df64_add(scalar_acc, df64_mul(qi, ki));
    }
    let scale = sqrt_df64(df64_from_f32(f32(D)));
    let w_l = df64_from_f32(params.w_l);
    let scalar_term = df64_mul(w_l, df64_div(scalar_acc, scale));

    // Term 2: pair bias (Zone 1)
    let w_c = df64_from_f32(params.w_c);
    let bias = df64_from_f64(pair_bias[(h * N + i) * N + j]);
    let pair_term = df64_mul(w_c, bias);

    // Term 3: point distance with df64 frame application (Zone 1+2)
    var dist_acc = df64_zero();
    for (var p = 0u; p < P; p++) {
        let qp_base = ((i * H + h) * P + p) * 3u;
        let kp_base = ((j * H + h) * P + p) * 3u;

        let qpx = df64_from_f64(q_points[qp_base]);
        let qpy = df64_from_f64(q_points[qp_base + 1u]);
        let qpz = df64_from_f64(q_points[qp_base + 2u]);
        let qp_global = apply_frame_df64(i, qpx, qpy, qpz);

        let kpx = df64_from_f64(k_points[kp_base]);
        let kpy = df64_from_f64(k_points[kp_base + 1u]);
        let kpz = df64_from_f64(k_points[kp_base + 2u]);
        let kp_global = apply_frame_df64(j, kpx, kpy, kpz);

        let dx = df64_sub(qp_global[0], kp_global[0]);
        let dy = df64_sub(qp_global[1], kp_global[1]);
        let dz = df64_sub(qp_global[2], kp_global[2]);
        dist_acc = df64_add(dist_acc, df64_mul(dx, dx));
        dist_acc = df64_add(dist_acc, df64_mul(dy, dy));
        dist_acc = df64_add(dist_acc, df64_mul(dz, dz));
    }
    let w_p = df64_from_f32(params.w_p);
    let gamma_half = df64_from_f32(-params.gamma / 2.0);
    let point_term = df64_mul(df64_mul(w_p, gamma_half), dist_acc);

    // Zone 3: combine terms → f64
    let total_score = df64_add(df64_add(scalar_term, pair_term), point_term);
    scores[idx] = df64_to_f64(total_score);
}
