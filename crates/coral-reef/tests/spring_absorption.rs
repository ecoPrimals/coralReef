// SPDX-License-Identifier: AGPL-3.0-only
//! Regression tests from Spring ecosystem shader absorption.
//!
//! These shaders are the production workloads that exposed coralReef bugs
//! and serve as the validation corpus for the Titan V sovereign pipeline.
//! Source provenance documented per test.

use coral_reef::{AmdArch, CompileOptions, GpuArch, GpuTarget, compile_wgsl};

fn sm70_opts() -> CompileOptions {
    CompileOptions {
        target: GpuArch::Sm70.into(),
        opt_level: 2,
        debug_info: false,
        fp64_software: true,
        ..CompileOptions::default()
    }
}

fn amd_opts() -> CompileOptions {
    CompileOptions {
        target: GpuTarget::Amd(AmdArch::Rdna2),
        opt_level: 2,
        debug_info: false,
        fp64_software: false,
        ..CompileOptions::default()
    }
}

// ---------------------------------------------------------------------------
// groundSpring — Anderson localization (f64 + uniform + PRNG + loop)
// Source: groundSpring/metalForge/shaders/anderson_lyapunov.wgsl
// Exercises: f64 arithmetic, var<uniform> struct, loop-carried values, PRNG
// ---------------------------------------------------------------------------

const ANDERSON_LYAPUNOV_WGSL: &str = r"
struct Params {
    n_sites:         u32,
    n_realizations:  u32,
    disorder_x1000:  i32,
    energy_x1000:    i32,
}

@group(0) @binding(0) var<uniform>             params: Params;
@group(0) @binding(1) var<storage, read_write> seeds:  array<u32>;
@group(0) @binding(2) var<storage, read_write> output: array<f64>;

fn rotl(x: u32, k: u32) -> u32 {
    return (x << k) | (x >> (32u - k));
}

fn xoshiro_next(s: ptr<function, vec4<u32>>) -> u32 {
    let result = rotl((*s).y * 5u, 7u) * 9u;
    let t = (*s).y << 9u;
    (*s).z ^= (*s).x;
    (*s).w ^= (*s).y;
    (*s).y ^= (*s).z;
    (*s).x ^= (*s).w;
    (*s).z ^= t;
    (*s).w = rotl((*s).w, 11u);
    return result;
}

fn xoshiro_uniform(s: ptr<function, vec4<u32>>) -> f64 {
    let hi = xoshiro_next(s);
    let lo = xoshiro_next(s);
    return (f64(hi) * 4294967296.0 + f64(lo)) / 18446744073709551616.0;
}

@compute @workgroup_size(64, 1, 1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let idx = gid.x;
    if idx >= params.n_realizations { return; }

    let disorder = f64(params.disorder_x1000) / 1000.0;
    let energy   = f64(params.energy_x1000) / 1000.0;

    let seed_base = idx * 4u;
    var state = vec4<u32>(
        seeds[seed_base],
        seeds[seed_base + 1u],
        seeds[seed_base + 2u],
        seeds[seed_base + 3u],
    );

    var log_growth: f64 = 0.0;
    var v0: f64 = 1.0;
    var v1: f64 = 0.0;

    for (var i = 0u; i < params.n_sites; i++) {
        let u = xoshiro_uniform(&state);
        let potential = disorder * (u - 0.5);
        let factor = energy - potential;

        let new_v0 = factor * v0 - v1;
        v1 = v0;
        v0 = new_v0;

        let norm = sqrt(v0 * v0 + v1 * v1);
        if norm > 0.0 {
            log_growth += log(norm);
            v0 /= norm;
            v1 /= norm;
        }
    }

    output[idx] = log_growth / f64(params.n_sites);

    seeds[seed_base]      = state.x;
    seeds[seed_base + 1u] = state.y;
    seeds[seed_base + 2u] = state.z;
    seeds[seed_base + 3u] = state.w;
}
";

// ---------------------------------------------------------------------------
// hotSpring/barraCuda — Yukawa force (f64, all-pairs, PBC)
// Source: hotSpring/barracuda/src/md/shaders/yukawa_force_f64.wgsl
// Exercises: f64 arithmetic, sqrt/exp/round transcendentals, nested loops
// ---------------------------------------------------------------------------

const YUKAWA_FORCE_F64_WGSL: &str = r"
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

    var fx = xi - xi;
    var fy = xi - xi;
    var fz = xi - xi;
    var pe = xi - xi;

    for (var j = 0u; j < n; j = j + 1u) {
        if (i == j) { continue; }

        let xj = positions[j * 3u];
        let yj = positions[j * 3u + 1u];
        let zj = positions[j * 3u + 2u];

        var dx = pbc_delta(xj - xi, box_x);
        var dy = pbc_delta(yj - yi, box_y);
        var dz = pbc_delta(zj - zi, box_z);

        let r_sq = dx * dx + dy * dy + dz * dz;

        if (r_sq > cutoff_sq) { continue; }

        let r = sqrt(r_sq + eps);
        let screening = exp(-kappa * r);
        let force_mag = prefactor * screening * (1.0 + kappa * r) / r_sq;

        let inv_r = 1.0 / r;
        fx = fx - force_mag * dx * inv_r;
        fy = fy - force_mag * dy * inv_r;
        fz = fz - force_mag * dz * inv_r;

        pe = pe + 0.5 * prefactor * screening * inv_r;
    }

    forces[i * 3u]      = fx;
    forces[i * 3u + 1u] = fy;
    forces[i * 3u + 2u] = fz;
    pe_buf[i] = pe;
}
";

// ---------------------------------------------------------------------------
// hotSpring/barraCuda — Dirac staggered operator (f64, SU(3), lattice QCD)
// Source: hotSpring/barracuda/src/lattice/shaders/dirac_staggered_f64.wgsl
// Exercises: f64, uniform struct, complex SU(3) matrix multiply, neighbor table
// ---------------------------------------------------------------------------

const DIRAC_STAGGERED_F64_WGSL: &str = r"
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

    var or0: f64 = params.mass_re * psi_in[psi_base + 0u];
    var oi0: f64 = params.mass_re * psi_in[psi_base + 1u];
    var or1: f64 = params.mass_re * psi_in[psi_base + 2u];
    var oi1: f64 = params.mass_re * psi_in[psi_base + 3u];
    var or2: f64 = params.mass_re * psi_in[psi_base + 4u];
    var oi2: f64 = params.mass_re * psi_in[psi_base + 5u];

    for (var mu: u32 = 0u; mu < 4u; mu = mu + 1u) {
        let half_eta = params.hop_sign * f64(0.5) * phases[site * 4u + mu];

        let fwd = nbr[site * 8u + mu * 2u];
        let fp = fwd * 6u;
        let fl = (site * 4u + mu) * 18u;

        var fr0 = links[fl+0u]*psi_in[fp+0u] - links[fl+1u]*psi_in[fp+1u]
                + links[fl+2u]*psi_in[fp+2u] - links[fl+3u]*psi_in[fp+3u]
                + links[fl+4u]*psi_in[fp+4u] - links[fl+5u]*psi_in[fp+5u];
        var fi0 = links[fl+0u]*psi_in[fp+1u] + links[fl+1u]*psi_in[fp+0u]
                + links[fl+2u]*psi_in[fp+3u] + links[fl+3u]*psi_in[fp+2u]
                + links[fl+4u]*psi_in[fp+5u] + links[fl+5u]*psi_in[fp+4u];
        var fr1 = links[fl+6u]*psi_in[fp+0u] - links[fl+7u]*psi_in[fp+1u]
                + links[fl+8u]*psi_in[fp+2u] - links[fl+9u]*psi_in[fp+3u]
                + links[fl+10u]*psi_in[fp+4u] - links[fl+11u]*psi_in[fp+5u];
        var fi1 = links[fl+6u]*psi_in[fp+1u] + links[fl+7u]*psi_in[fp+0u]
                + links[fl+8u]*psi_in[fp+3u] + links[fl+9u]*psi_in[fp+2u]
                + links[fl+10u]*psi_in[fp+5u] + links[fl+11u]*psi_in[fp+4u];
        var fr2 = links[fl+12u]*psi_in[fp+0u] - links[fl+13u]*psi_in[fp+1u]
                + links[fl+14u]*psi_in[fp+2u] - links[fl+15u]*psi_in[fp+3u]
                + links[fl+16u]*psi_in[fp+4u] - links[fl+17u]*psi_in[fp+5u];
        var fi2 = links[fl+12u]*psi_in[fp+1u] + links[fl+13u]*psi_in[fp+0u]
                + links[fl+14u]*psi_in[fp+3u] + links[fl+15u]*psi_in[fp+2u]
                + links[fl+16u]*psi_in[fp+5u] + links[fl+17u]*psi_in[fp+4u];

        let bwd = nbr[site * 8u + mu * 2u + 1u];
        let bp = bwd * 6u;
        let bl = (bwd * 4u + mu) * 18u;

        var br0 = links[bl+0u]*psi_in[bp+0u] + links[bl+1u]*psi_in[bp+1u]
                + links[bl+6u]*psi_in[bp+2u] + links[bl+7u]*psi_in[bp+3u]
                + links[bl+12u]*psi_in[bp+4u] + links[bl+13u]*psi_in[bp+5u];
        var bi0 = links[bl+0u]*psi_in[bp+1u] - links[bl+1u]*psi_in[bp+0u]
                + links[bl+6u]*psi_in[bp+3u] - links[bl+7u]*psi_in[bp+2u]
                + links[bl+12u]*psi_in[bp+5u] - links[bl+13u]*psi_in[bp+4u];
        var br1 = links[bl+2u]*psi_in[bp+0u] + links[bl+3u]*psi_in[bp+1u]
                + links[bl+8u]*psi_in[bp+2u] + links[bl+9u]*psi_in[bp+3u]
                + links[bl+14u]*psi_in[bp+4u] + links[bl+15u]*psi_in[bp+5u];
        var bi1 = links[bl+2u]*psi_in[bp+1u] - links[bl+3u]*psi_in[bp+0u]
                + links[bl+8u]*psi_in[bp+3u] - links[bl+9u]*psi_in[bp+2u]
                + links[bl+14u]*psi_in[bp+5u] - links[bl+15u]*psi_in[bp+4u];
        var br2 = links[bl+4u]*psi_in[bp+0u] + links[bl+5u]*psi_in[bp+1u]
                + links[bl+10u]*psi_in[bp+2u] + links[bl+11u]*psi_in[bp+3u]
                + links[bl+16u]*psi_in[bp+4u] + links[bl+17u]*psi_in[bp+5u];
        var bi2 = links[bl+4u]*psi_in[bp+1u] - links[bl+5u]*psi_in[bp+0u]
                + links[bl+10u]*psi_in[bp+3u] - links[bl+11u]*psi_in[bp+2u]
                + links[bl+16u]*psi_in[bp+5u] - links[bl+17u]*psi_in[bp+4u];

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
";

// ---------------------------------------------------------------------------
// hotSpring/barraCuda — sum_reduce_f64 (workgroup shared memory, tree reduce)
// Source: hotSpring/barracuda/src/lattice/shaders/sum_reduce_f64.wgsl
// Exercises: f64, var<workgroup>, workgroupBarrier, var<uniform>, loop
// This shader exposed the original f64 emission + BAR.SYNC bugs.
// ---------------------------------------------------------------------------

const SUM_REDUCE_F64_WGSL: &str = r"
struct ReduceParams {
    size: u32,
    _pad1: u32,
    _pad2: u32,
    _pad3: u32,
}

@group(0) @binding(0) var<storage, read> input: array<f64>;
@group(0) @binding(1) var<storage, read_write> output: array<f64>;
@group(0) @binding(2) var<uniform> params: ReduceParams;

var<workgroup> shared_data: array<f64, 256>;

@compute @workgroup_size(256)
fn main(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(local_invocation_id) local_id: vec3<u32>,
    @builtin(workgroup_id) workgroup_id: vec3<u32>,
    @builtin(num_workgroups) nwg: vec3<u32>,
) {
    let tid = local_id.x;
    let gid = global_id.x + global_id.y * nwg.x * 256u;

    if (gid < params.size) {
        shared_data[tid] = input[gid];
    } else {
        shared_data[tid] = f64(0.0);
    }
    workgroupBarrier();

    for (var stride = 128u; stride > 0u; stride = stride >> 1u) {
        if (tid < stride) {
            shared_data[tid] = shared_data[tid] + shared_data[tid + stride];
        }
        workgroupBarrier();
    }

    if (tid == 0u) {
        let wg_linear = workgroup_id.x + workgroup_id.y * nwg.x;
        output[wg_linear] = shared_data[0];
    }
}
";

// ===========================================================================
// SM70 (Volta) compilation tests
// ===========================================================================

#[test]
fn spring_anderson_lyapunov_sm70() {
    let r = compile_wgsl(ANDERSON_LYAPUNOV_WGSL, &sm70_opts());
    assert!(
        r.is_ok(),
        "anderson_lyapunov f64 should compile for SM70: {r:?}"
    );
    assert!(!r.unwrap().is_empty());
}

#[test]
fn spring_yukawa_force_f64_sm70() {
    let r = compile_wgsl(YUKAWA_FORCE_F64_WGSL, &sm70_opts());
    assert!(r.is_ok(), "yukawa_force_f64 should compile for SM70: {r:?}");
    assert!(!r.unwrap().is_empty());
}

#[test]
fn spring_dirac_staggered_f64_sm70() {
    let r = compile_wgsl(DIRAC_STAGGERED_F64_WGSL, &sm70_opts());
    assert!(
        r.is_ok(),
        "dirac_staggered_f64 should compile for SM70: {r:?}"
    );
    assert!(!r.unwrap().is_empty());
}

#[test]
fn spring_sum_reduce_f64_sm70() {
    let r = compile_wgsl(SUM_REDUCE_F64_WGSL, &sm70_opts());
    assert!(r.is_ok(), "sum_reduce_f64 should compile for SM70: {r:?}");
    assert!(!r.unwrap().is_empty());
}

// ===========================================================================
// AMD RDNA2 compilation tests
// ===========================================================================

#[test]
#[ignore = "RDNA2 f64 ops need literal constant materialization (VOP3 limitation)"]
fn spring_anderson_lyapunov_amd() {
    let r = compile_wgsl(ANDERSON_LYAPUNOV_WGSL, &amd_opts());
    assert!(
        r.is_ok(),
        "anderson_lyapunov f64 should compile for RDNA2: {r:?}"
    );
    assert!(!r.unwrap().is_empty());
}

#[test]
#[ignore = "RDNA2 f64 ops need literal constant materialization (VOP3 limitation)"]
fn spring_yukawa_force_f64_amd() {
    let r = compile_wgsl(YUKAWA_FORCE_F64_WGSL, &amd_opts());
    assert!(
        r.is_ok(),
        "yukawa_force_f64 should compile for RDNA2: {r:?}"
    );
    assert!(!r.unwrap().is_empty());
}

#[test]
#[ignore = "RDNA2 f64 ops need literal constant materialization (VOP3 limitation)"]
fn spring_dirac_staggered_f64_amd() {
    let r = compile_wgsl(DIRAC_STAGGERED_F64_WGSL, &amd_opts());
    assert!(
        r.is_ok(),
        "dirac_staggered_f64 should compile for RDNA2: {r:?}"
    );
    assert!(!r.unwrap().is_empty());
}

#[test]
#[ignore = "RDNA2 f64 ops need literal constant materialization (VOP3 limitation)"]
fn spring_sum_reduce_f64_amd() {
    let r = compile_wgsl(SUM_REDUCE_F64_WGSL, &amd_opts());
    assert!(r.is_ok(), "sum_reduce_f64 should compile for RDNA2: {r:?}");
    assert!(!r.unwrap().is_empty());
}
