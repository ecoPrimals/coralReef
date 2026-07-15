// SPDX-License-Identifier: AGPL-3.0-or-later
//! Advanced Spring ecosystem shader absorption regression tests.
//!
//! Inline WGSL workloads for FMA patterns, neural-compute activations,
//! and ODE/population-genetics kernels. Complements `spring_absorption.rs`.

use coral_reef::{AmdArch, CompileOptions, GpuTarget, NvArch, compile_wgsl_full};

fn sm70_opts() -> CompileOptions {
    CompileOptions {
        target: GpuTarget::Nvidia(NvArch::Sm70),
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
// Compute-pipeline — SU(3) link update with heavy FMA usage
// ---------------------------------------------------------------------------

#[test]
fn spring_su3_link_update_fma_sm70() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> links: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let idx = gid.x * 18u;
    var u_re = array<f32, 9>();
    var u_im = array<f32, 9>();
    for (var i = 0u; i < 9u; i = i + 1u) {
        u_re[i] = links[idx + i * 2u];
        u_im[i] = links[idx + i * 2u + 1u];
    }
    var c_re = array<f32, 9>();
    var c_im = array<f32, 9>();
    for (var i = 0u; i < 3u; i = i + 1u) {
        for (var j = 0u; j < 3u; j = j + 1u) {
            var sum_re = 0.0f;
            var sum_im = 0.0f;
            for (var k = 0u; k < 3u; k = k + 1u) {
                let a_re = u_re[i * 3u + k];
                let a_im = u_im[i * 3u + k];
                let b_re = u_re[k * 3u + j];
                let b_im = u_im[k * 3u + j];
                sum_re = fma(a_re, b_re, sum_re) - a_im * b_im;
                sum_im = fma(a_re, b_im, sum_im) + a_im * b_re;
            }
            c_re[i * 3u + j] = sum_re;
            c_im[i * 3u + j] = sum_im;
        }
    }
    for (var i = 0u; i < 9u; i = i + 1u) {
        links[idx + i * 2u] = c_re[i];
        links[idx + i * 2u + 1u] = c_im[i];
    }
}
";
    let opts = CompileOptions {
        target: GpuTarget::Nvidia(NvArch::Sm70),
        ..CompileOptions::default()
    };
    let result = compile_wgsl_full(wgsl, &opts);
    assert!(
        result.is_ok(),
        "su3_link_update FMA pattern should compile for SM70: {result:?}"
    );
}

#[test]
fn spring_su3_link_update_fma_rdna2() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> links: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let idx = gid.x * 18u;
    var u_re = array<f32, 9>();
    var u_im = array<f32, 9>();
    for (var i = 0u; i < 9u; i = i + 1u) {
        u_re[i] = links[idx + i * 2u];
        u_im[i] = links[idx + i * 2u + 1u];
    }
    var c_re = array<f32, 9>();
    var c_im = array<f32, 9>();
    for (var i = 0u; i < 3u; i = i + 1u) {
        for (var j = 0u; j < 3u; j = j + 1u) {
            var sum_re = 0.0f;
            var sum_im = 0.0f;
            for (var k = 0u; k < 3u; k = k + 1u) {
                let a_re = u_re[i * 3u + k];
                let a_im = u_im[i * 3u + k];
                let b_re = u_re[k * 3u + j];
                let b_im = u_im[k * 3u + j];
                sum_re = fma(a_re, b_re, sum_re) - a_im * b_im;
                sum_im = fma(a_re, b_im, sum_im) + a_im * b_re;
            }
            c_re[i * 3u + j] = sum_re;
            c_im[i * 3u + j] = sum_im;
        }
    }
    for (var i = 0u; i < 9u; i = i + 1u) {
        links[idx + i * 2u] = c_re[i];
        links[idx + i * 2u + 1u] = c_im[i];
    }
}
";
    let opts = CompileOptions {
        target: GpuTarget::Amd(AmdArch::Rdna2),
        ..CompileOptions::default()
    };
    let result = compile_wgsl_full(wgsl, &opts);
    assert!(
        result.is_ok(),
        "su3_link_update FMA pattern should compile for RDNA2: {result:?}"
    );
}

// ---------------------------------------------------------------------------
// Compute-pipeline — Wilson plaquette with FMA accumulation
// ---------------------------------------------------------------------------

#[test]
fn spring_wilson_plaquette_fma_sm70() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> plaq: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let idx = gid.x * 4u;
    var trace = 0.0f;
    for (var mu = 0u; mu < 4u; mu = mu + 1u) {
        let u_re = plaq[idx + mu];
        trace = fma(u_re, u_re, trace);
    }
    let action = 1.0 - trace / 3.0;
    plaq[idx] = action;
}
";
    let opts = CompileOptions {
        target: GpuTarget::Nvidia(NvArch::Sm70),
        ..CompileOptions::default()
    };
    let result = compile_wgsl_full(wgsl, &opts);
    assert!(
        result.is_ok(),
        "wilson_plaquette FMA pattern should compile for SM70: {result:?}"
    );
}

#[test]
fn spring_wilson_plaquette_fma_rdna2() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> plaq: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let idx = gid.x * 4u;
    var trace = 0.0f;
    for (var mu = 0u; mu < 4u; mu = mu + 1u) {
        let u_re = plaq[idx + mu];
        trace = fma(u_re, u_re, trace);
    }
    let action = 1.0 - trace / 3.0;
    plaq[idx] = action;
}
";
    let opts = CompileOptions {
        target: GpuTarget::Amd(AmdArch::Rdna2),
        ..CompileOptions::default()
    };
    let result = compile_wgsl_full(wgsl, &opts);
    assert!(
        result.is_ok(),
        "wilson_plaquette FMA pattern should compile for RDNA2: {result:?}"
    );
}

// ---------------------------------------------------------------------------
// Neural-compute domain — LogSumExp (neural network activation normalization)
// ---------------------------------------------------------------------------

#[test]
fn spring_logsumexp_f32_sm70() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> data: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let base = gid.x * 128u;
    var max_val = data[base];
    for (var i = 1u; i < 128u; i = i + 1u) {
        max_val = max(max_val, data[base + i]);
    }
    var sum_exp = 0.0f;
    for (var i = 0u; i < 128u; i = i + 1u) {
        sum_exp = sum_exp + exp(data[base + i] - max_val);
    }
    data[base] = max_val + log(sum_exp);
}
";
    let result = compile_wgsl_full(wgsl, &sm70_opts());
    assert!(
        result.is_ok(),
        "logsumexp should compile for SM70: {result:?}"
    );
}

#[test]
fn spring_logsumexp_f32_rdna2() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> data: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let base = gid.x * 128u;
    var max_val = data[base];
    for (var i = 1u; i < 128u; i = i + 1u) {
        max_val = max(max_val, data[base + i]);
    }
    var sum_exp = 0.0f;
    for (var i = 0u; i < 128u; i = i + 1u) {
        sum_exp = sum_exp + exp(data[base + i] - max_val);
    }
    data[base] = max_val + log(sum_exp);
}
";
    let result = compile_wgsl_full(wgsl, &amd_opts());
    assert!(
        result.is_ok(),
        "logsumexp should compile for RDNA2: {result:?}"
    );
}

// ---------------------------------------------------------------------------
// Neural-compute domain — RK45 (Runge-Kutta ODE solver step)
// ---------------------------------------------------------------------------

#[test]
fn spring_rk45_step_f64_sm70() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> state: array<f32>;
@group(0) @binding(1) var<storage, read> params: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let idx = gid.x;
    let y = state[idx];
    let dt = params[0];
    let k1 = y * params[1];
    let k2 = (y + dt * 0.5 * k1) * params[1];
    let k3 = (y + dt * 0.5 * k2) * params[1];
    let k4 = (y + dt * k3) * params[1];
    state[idx] = y + dt * (k1 + 2.0 * k2 + 2.0 * k3 + k4) / 6.0;
}
";
    let result = compile_wgsl_full(wgsl, &sm70_opts());
    assert!(
        result.is_ok(),
        "rk45_step should compile for SM70: {result:?}"
    );
}

#[test]
fn spring_rk45_step_f64_rdna2() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> state: array<f32>;
@group(0) @binding(1) var<storage, read> params: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let idx = gid.x;
    let y = state[idx];
    let dt = params[0];
    let k1 = y * params[1];
    let k2 = (y + dt * 0.5 * k1) * params[1];
    let k3 = (y + dt * 0.5 * k2) * params[1];
    let k4 = (y + dt * k3) * params[1];
    state[idx] = y + dt * (k1 + 2.0 * k2 + 2.0 * k3 + k4) / 6.0;
}
";
    let result = compile_wgsl_full(wgsl, &amd_opts());
    assert!(
        result.is_ok(),
        "rk45_step should compile for RDNA2: {result:?}"
    );
}

// ---------------------------------------------------------------------------
// Neural-compute domain — Wright-Fisher population genetics
// ---------------------------------------------------------------------------

#[test]
fn spring_wright_fisher_f32_sm70() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> freq: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let idx = gid.x;
    let p = freq[idx];
    let s = 0.01f;
    let mu = 0.001f;
    let w_bar = 1.0 - s * p * (1.0 - p);
    let p_next = (p * (1.0 - mu) * (1.0 - s * (1.0 - p))) / w_bar;
    freq[idx] = clamp(p_next, 0.0, 1.0);
}
";
    let result = compile_wgsl_full(wgsl, &sm70_opts());
    assert!(
        result.is_ok(),
        "wright_fisher should compile for SM70: {result:?}"
    );
}

#[test]
fn spring_wright_fisher_f32_rdna2() {
    let wgsl = r"
@group(0) @binding(0) var<storage, read_write> freq: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let idx = gid.x;
    let p = freq[idx];
    let s = 0.01f;
    let mu = 0.001f;
    let w_bar = 1.0 - s * p * (1.0 - p);
    let p_next = (p * (1.0 - mu) * (1.0 - s * (1.0 - p))) / w_bar;
    freq[idx] = clamp(p_next, 0.0, 1.0);
}
";
    let result = compile_wgsl_full(wgsl, &amd_opts());
    assert!(
        result.is_ok(),
        "wright_fisher should compile for RDNA2: {result:?}"
    );
}
