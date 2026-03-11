// SPDX-License-Identifier: AGPL-3.0-only
//! NVVM Poisoning Validation — DF64 Yukawa Force (`exp_df64` + `sqrt_df64`)
//!
//! hotSpring Exp 053: NVIDIA proprietary NVVM chokes on DF64 transcendentals
//! (`exp_df64`, `sqrt_df64`) causing permanent device poisoning. This forces a
//! fallback to native f64 at 1:32 throughput on Ampere, producing a 12.4x
//! Kokkos parity gap (212 steps/s vs 2,630 steps/s).
//!
//! coralReef sovereign path (WGSL -> naga -> IR -> SASS) bypasses NVVM entirely.
//! The DF64 preamble uses only f32 pair arithmetic (Knuth two-sum, Dekker mul,
//! Horner exp2), so it compiles to FADD/FMUL/FFMA/MUFU.EX2 — all safe.
//!
//! These tests validate the fix: sovereign compilation of the exact shader
//! that poisons NVVM, targeting SM70/SM86/RDNA2.

use coral_reef::{AmdArch, CompileOptions, Fp64Strategy, GpuArch, GpuTarget, compile_wgsl};

fn sm70_df64_opts() -> CompileOptions {
    CompileOptions {
        target: GpuArch::Sm70.into(),
        opt_level: 2,
        debug_info: false,
        fp64_software: true,
        fp64_strategy: Fp64Strategy::DoubleFloat,
        ..CompileOptions::default()
    }
}

fn sm86_df64_opts() -> CompileOptions {
    CompileOptions {
        target: GpuArch::Sm86.into(),
        opt_level: 2,
        debug_info: false,
        fp64_software: true,
        fp64_strategy: Fp64Strategy::DoubleFloat,
        ..CompileOptions::default()
    }
}

fn sm89_df64_opts() -> CompileOptions {
    CompileOptions {
        target: GpuArch::Sm89.into(),
        opt_level: 2,
        debug_info: false,
        fp64_software: true,
        fp64_strategy: Fp64Strategy::DoubleFloat,
        ..CompileOptions::default()
    }
}

fn amd_df64_opts() -> CompileOptions {
    CompileOptions {
        target: GpuTarget::Amd(AmdArch::Rdna2),
        opt_level: 2,
        debug_info: false,
        fp64_software: false,
        fp64_strategy: Fp64Strategy::DoubleFloat,
        ..CompileOptions::default()
    }
}

// Adapted from hotSpring/barracuda/src/md/shaders/yukawa_force_df64.wgsl
// Uses round() (WGSL builtin) instead of barraCuda's round_f64 polyfill.
// The df64 preamble (exp_df64, sqrt_df64, df64_add/sub/mul/div etc) is
// auto-prepended by coralReef's prepare_wgsl when it sees Df64/df64_ usage.
const YUKAWA_FORCE_DF64_WGSL: &str = r"
@group(0) @binding(0) var<storage, read> positions: array<f64>;
@group(0) @binding(1) var<storage, read_write> forces: array<f64>;
@group(0) @binding(2) var<storage, read_write> pe_buf: array<f64>;
@group(0) @binding(3) var<storage, read> params: array<f64>;

fn pbc_delta(delta: f64, box_size: f64) -> f64 {
    return delta - box_size * round(delta / box_size);
}

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    let n = u32(params[0]);
    if (i >= n) { return; }

    let xi = positions[i * 3u];
    let yi = positions[i * 3u + 1u];
    let zi = positions[i * 3u + 2u];

    let kappa     = params[1];
    let prefactor = params[2];
    let cutoff_sq = params[3];
    let box_x     = params[4];
    let box_y     = params[5];
    let box_z     = params[6];
    let eps       = params[7];

    var fx = df64_zero();
    var fy = df64_zero();
    var fz = df64_zero();
    var pe = df64_zero();

    let kappa_df = df64_from_f64(kappa);
    let prefactor_df = df64_from_f64(prefactor);
    let half = df64_from_f32(0.5);
    let one = df64_from_f32(1.0);

    for (var j = 0u; j < n; j = j + 1u) {
        if (i == j) { continue; }

        let xj = positions[j * 3u];
        let yj = positions[j * 3u + 1u];
        let zj = positions[j * 3u + 2u];

        let dx_f64 = pbc_delta(xj - xi, box_x);
        let dy_f64 = pbc_delta(yj - yi, box_y);
        let dz_f64 = pbc_delta(zj - zi, box_z);

        let r_sq_f64 = dx_f64 * dx_f64 + dy_f64 * dy_f64 + dz_f64 * dz_f64;
        if (r_sq_f64 > cutoff_sq) { continue; }

        let dx = df64_from_f64(dx_f64);
        let dy = df64_from_f64(dy_f64);
        let dz = df64_from_f64(dz_f64);
        let r_sq = df64_from_f64(r_sq_f64);
        let r = sqrt_df64(df64_add(r_sq, df64_from_f64(eps)));

        let screening = exp_df64(df64_neg(df64_mul(kappa_df, r)));
        let kappa_r = df64_mul(kappa_df, r);
        let force_mag = df64_div(
            df64_mul(prefactor_df, df64_mul(screening, df64_add(one, kappa_r))),
            r_sq
        );

        let inv_r = df64_div(one, r);
        fx = df64_sub(fx, df64_mul(force_mag, df64_mul(dx, inv_r)));
        fy = df64_sub(fy, df64_mul(force_mag, df64_mul(dy, inv_r)));
        fz = df64_sub(fz, df64_mul(force_mag, df64_mul(dz, inv_r)));

        pe = df64_add(pe, df64_mul(half, df64_mul(prefactor_df, df64_mul(screening, inv_r))));
    }

    forces[i * 3u]      = df64_to_f64(fx);
    forces[i * 3u + 1u] = df64_to_f64(fy);
    forces[i * 3u + 2u] = df64_to_f64(fz);
    pe_buf[i] = df64_to_f64(pe);
}
";

#[test]
fn nvvm_poisoning_yukawa_df64_sm70() {
    let r = compile_wgsl(YUKAWA_FORCE_DF64_WGSL, &sm70_df64_opts());
    assert!(
        r.is_ok(),
        "DF64 Yukawa force (NVVM-poisoning shader) should compile via sovereign path for SM70: {r:?}"
    );
    assert!(!r.unwrap().is_empty());
}

#[test]
fn nvvm_poisoning_yukawa_df64_sm86() {
    let r = compile_wgsl(YUKAWA_FORCE_DF64_WGSL, &sm86_df64_opts());
    assert!(
        r.is_ok(),
        "DF64 Yukawa force (NVVM-poisoning shader) should compile via sovereign path for SM86 (RTX 3090): {r:?}"
    );
    assert!(!r.unwrap().is_empty());
}

#[test]
fn nvvm_poisoning_yukawa_df64_rdna2() {
    let r = compile_wgsl(YUKAWA_FORCE_DF64_WGSL, &amd_df64_opts());
    assert!(
        r.is_ok(),
        "DF64 Yukawa force should compile via sovereign path for RDNA2: {r:?}"
    );
    assert!(!r.unwrap().is_empty());
}

#[test]
fn nvvm_poisoning_df64_transcendentals_isolated_sm86() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read> input: array<f32>;
@group(0) @binding(1) var<storage, read_write> output: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let idx = gid.x;
    let x = df64_from_f32(input[idx]);

    let e = exp_df64(x);
    let s = sqrt_df64(x);
    let t = tanh_df64(x);

    let result = df64_add(e, df64_add(s, t));
    output[idx] = result.hi;
}
";
    let r = compile_wgsl(wgsl, &sm86_df64_opts());
    assert!(
        r.is_ok(),
        "Isolated DF64 transcendentals (exp/sqrt/tanh) should compile for SM86: {r:?}"
    );
    assert!(!r.unwrap().is_empty());
}

#[test]
fn nvvm_poisoning_df64_transcendentals_isolated_sm70() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read> input: array<f32>;
@group(0) @binding(1) var<storage, read_write> output: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let idx = gid.x;
    let x = df64_from_f32(input[idx]);

    let e = exp_df64(x);
    let s = sqrt_df64(x);
    let t = tanh_df64(x);

    let result = df64_add(e, df64_add(s, t));
    output[idx] = result.hi;
}
";
    let r = compile_wgsl(wgsl, &sm70_df64_opts());
    assert!(
        r.is_ok(),
        "Isolated DF64 transcendentals (exp/sqrt/tanh) should compile for SM70: {r:?}"
    );
    assert!(!r.unwrap().is_empty());
}

// Verlet integrator with DF64 force — the second shader hotSpring uses
// in the MD pipeline. Tests that velocity Verlet + DF64 force accumulation
// compiles through the sovereign path.
#[test]
fn nvvm_poisoning_yukawa_verlet_df64_sm86() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> pos: array<f64>;
@group(0) @binding(1) var<storage, read_write> vel: array<f64>;
@group(0) @binding(2) var<storage, read> force: array<f64>;
@group(0) @binding(3) var<storage, read> params: array<f64>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    let n = u32(params[0]);
    if (i >= n) { return; }

    let dt = params[1];
    let half_dt = df64_from_f32(0.5);
    let dt_df = df64_from_f64(dt);
    let half_dt_sq = df64_mul(df64_mul(half_dt, dt_df), dt_df);

    for (var c = 0u; c < 3u; c = c + 1u) {
        let idx = i * 3u + c;
        let f = df64_from_f64(force[idx]);
        let v = df64_from_f64(vel[idx]);
        let p = df64_from_f64(pos[idx]);

        let new_p = df64_add(p, df64_add(df64_mul(v, dt_df), df64_mul(f, half_dt_sq)));
        pos[idx] = df64_to_f64(new_p);

        let new_v = df64_add(v, df64_mul(f, dt_df));
        vel[idx] = df64_to_f64(new_v);
    }
}
";
    let r = compile_wgsl(wgsl, &sm86_df64_opts());
    assert!(
        r.is_ok(),
        "DF64 Verlet integrator should compile for SM86: {r:?}"
    );
    assert!(!r.unwrap().is_empty());
}

// ===========================================================================
// SM89 (Ada Lovelace) — RTX 40xx series
// neuralSpring found `enable f64;` regressions on Ada; sovereign path must
// bypass these. SM89 has 1:64 native f64 throughput, making DF64 essential.
// ===========================================================================

#[test]
fn nvvm_poisoning_yukawa_df64_sm89() {
    let r = compile_wgsl(YUKAWA_FORCE_DF64_WGSL, &sm89_df64_opts());
    assert!(
        r.is_ok(),
        "DF64 Yukawa force should compile via sovereign path for SM89 (Ada): {r:?}"
    );
    assert!(!r.unwrap().is_empty());
}

#[test]
fn nvvm_poisoning_df64_transcendentals_isolated_sm89() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read> input: array<f32>;
@group(0) @binding(1) var<storage, read_write> output: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let idx = gid.x;
    let x = df64_from_f32(input[idx]);

    let e = exp_df64(x);
    let s = sqrt_df64(x);
    let t = tanh_df64(x);

    let result = df64_add(e, df64_add(s, t));
    output[idx] = result.hi;
}
";
    let r = compile_wgsl(wgsl, &sm89_df64_opts());
    assert!(
        r.is_ok(),
        "Isolated DF64 transcendentals (exp/sqrt/tanh) should compile for SM89: {r:?}"
    );
    assert!(!r.unwrap().is_empty());
}

#[test]
fn nvvm_poisoning_yukawa_verlet_df64_sm89() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> pos: array<f64>;
@group(0) @binding(1) var<storage, read_write> vel: array<f64>;
@group(0) @binding(2) var<storage, read> force: array<f64>;
@group(0) @binding(3) var<storage, read> params: array<f64>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    let n = u32(params[0]);
    if (i >= n) { return; }

    let dt = params[1];
    let half_dt = df64_from_f32(0.5);
    let dt_df = df64_from_f64(dt);
    let half_dt_sq = df64_mul(df64_mul(half_dt, dt_df), dt_df);

    for (var c = 0u; c < 3u; c = c + 1u) {
        let idx = i * 3u + c;
        let f = df64_from_f64(force[idx]);
        let v = df64_from_f64(vel[idx]);
        let p = df64_from_f64(pos[idx]);

        let new_p = df64_add(p, df64_add(df64_mul(v, dt_df), df64_mul(f, half_dt_sq)));
        pos[idx] = df64_to_f64(new_p);

        let new_v = df64_add(v, df64_mul(f, dt_df));
        vel[idx] = df64_to_f64(new_v);
    }
}
";
    let r = compile_wgsl(wgsl, &sm89_df64_opts());
    assert!(
        r.is_ok(),
        "DF64 Verlet integrator should compile for SM89: {r:?}"
    );
    assert!(!r.unwrap().is_empty());
}
