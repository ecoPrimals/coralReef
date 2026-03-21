// SPDX-License-Identifier: AGPL-3.0-only
//! f64 math lowering across NVIDIA arches — exercises `naga_translate` software
//! transcendental paths and SM75/SM80 encoder + latency tables (vs SM70 baseline).

use coral_reef::{CompileError, CompileOptions, GpuTarget, NvArch};

fn opts_nv(nv: NvArch) -> CompileOptions {
    CompileOptions {
        target: GpuTarget::Nvidia(nv),
        opt_level: 2,
        debug_info: false,
        fp64_software: true,
        ..CompileOptions::default()
    }
}

fn try_compile_nv(wgsl: &str, nv: NvArch) {
    match coral_reef::compile_wgsl(wgsl, &opts_nv(nv)) {
        Ok(binary) => assert!(!binary.is_empty(), "{nv}: empty binary"),
        Err(CompileError::NotImplemented(_)) => {}
        Err(e) => panic!("{nv}: {e}"),
    }
}

/// f64 transcendentals + roots — software lowering without hitting known encoder edge cases.
const F64_MATH_CORE: &str = r"
enable f64;
@group(0) @binding(0) var<storage, read_write> out: array<f64>;
@group(0) @binding(1) var<storage, read> inp: array<f64>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let x = inp[gid.x];
    let y = inp[gid.x + 1u];
    let a = sin(x) + cos(x);
    let b = exp2(x * 0.1) + log2(x + 2.0);
    let c = sqrt(x + 1.0) + inverseSqrt(x + 1.0);
    let d = pow(x + 0.5, y * 0.25 + 0.5);
    out[gid.x] = a + b + c + d;
}
";

#[test]
fn f64_math_core_sm70() {
    try_compile_nv(F64_MATH_CORE, NvArch::Sm70);
}

#[test]
fn f64_math_core_sm75() {
    try_compile_nv(F64_MATH_CORE, NvArch::Sm75);
}

#[test]
fn f64_math_core_sm80() {
    try_compile_nv(F64_MATH_CORE, NvArch::Sm80);
}

#[test]
fn f64_math_core_sm86() {
    try_compile_nv(F64_MATH_CORE, NvArch::Sm86);
}

/// Vector f64 ops — extra `expr` / `func_ops` coverage.
const F64_VEC_WGSL: &str = r"
enable f64;
@group(0) @binding(0) var<storage, read_write> out: array<f64>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let a = vec3<f64>(f64(gid.x), f64(gid.y), 1.0);
    let b = vec3<f64>(1.0, 2.0, 3.0);
    let d = dot(a, b);
    let c = cross(vec3<f64>(1.0, 0.0, 0.0), vec3<f64>(0.0, 1.0, 0.0));
    out[gid.x] = d + length(c);
}
";

#[test]
fn f64_vec_dot_cross_sm75_sm80() {
    for nv in [NvArch::Sm75, NvArch::Sm80] {
        try_compile_nv(F64_VEC_WGSL, nv);
    }
}
