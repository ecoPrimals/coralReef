// SPDX-License-Identifier: AGPL-3.0-or-later
//! Parity compilation tests — same WGSL compiled for multiple targets.
//!
//! Validates that the compiler produces valid, non-empty binaries with
//! reasonable metadata for both NVIDIA SM86 and AMD RDNA2 from identical
//! WGSL sources. Does NOT require hardware — purely compiler-level.
//!
//! `@builtin(global_invocation_id)` is resolved using compile-time workgroup
//! size constants, which works across all targets (NVIDIA and AMD).

use coral_reef::{AmdArch, CompileOptions, FmaPolicy, GpuTarget, NvArch};

fn opts_for_sm86() -> CompileOptions {
    CompileOptions {
        target: GpuTarget::Nvidia(NvArch::Sm86),
        opt_level: 2,
        debug_info: false,
        fp64_software: false,
        fma_policy: FmaPolicy::Fused,
        ..CompileOptions::default()
    }
}

fn opts_for_rdna2() -> CompileOptions {
    CompileOptions {
        target: GpuTarget::Amd(AmdArch::Rdna2),
        opt_level: 2,
        debug_info: false,
        fp64_software: false,
        fma_policy: FmaPolicy::Fused,
        ..CompileOptions::default()
    }
}

fn compile_sm86(wgsl: &str) -> coral_reef::backend::CompiledBinary {
    coral_reef::compile_wgsl_full(wgsl, &opts_for_sm86()).expect("SM86 compilation should succeed")
}

fn compile_rdna2(wgsl: &str) -> coral_reef::backend::CompiledBinary {
    coral_reef::compile_wgsl_full(wgsl, &opts_for_rdna2())
        .expect("RDNA2 compilation should succeed")
}

// --- Shaders that work on both targets ---

const STORE_42: &str = r"
@group(0) @binding(0)
var<storage, read_write> out: array<u32>;

@compute @workgroup_size(1)
fn main() {
    out[0] = 42u;
}
";

const STORE_99: &str = r"
@group(0) @binding(0)
var<storage, read_write> out: array<u32>;

@compute @workgroup_size(1)
fn main() {
    out[0] = 99u;
}
";

// --- Shaders that use global_invocation_id (works on all targets) ---

const VECADD: &str = r"
@group(0) @binding(0) var<storage> a: array<f32>;
@group(0) @binding(1) var<storage> b: array<f32>;
@group(0) @binding(2) var<storage, read_write> out: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    out[gid.x] = a[gid.x] + b[gid.x];
}
";

const SAXPY: &str = r"
@group(0) @binding(0) var<storage> x: array<f32>;
@group(0) @binding(1) var<storage> y: array<f32>;
@group(0) @binding(2) var<storage, read_write> out: array<f32>;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let alpha: f32 = 2.5;
    out[gid.x] = alpha * x[gid.x] + y[gid.x];
}
";

const MATMUL_TILE: &str = r"
@group(0) @binding(0) var<storage> a: array<f32>;
@group(0) @binding(1) var<storage> b: array<f32>;
@group(0) @binding(2) var<storage, read_write> c: array<f32>;

const N: u32 = 64u;

@compute @workgroup_size(8, 8)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let row = gid.y;
    let col = gid.x;
    var sum: f32 = 0.0;
    for (var k: u32 = 0u; k < N; k = k + 1u) {
        sum = sum + a[row * N + k] * b[k * N + col];
    }
    c[row * N + col] = sum;
}
";

// ============================================================
// Tests for shaders that compile on BOTH targets
// ============================================================

#[test]
fn parity_store_42_both_produce_nonempty_binary() {
    let sm86 = compile_sm86(STORE_42);
    let rdna2 = compile_rdna2(STORE_42);
    assert!(!sm86.binary.is_empty(), "SM86 binary must be non-empty");
    assert!(!rdna2.binary.is_empty(), "RDNA2 binary must be non-empty");
}

#[test]
fn parity_store_42_metadata_reasonable() {
    let sm86 = compile_sm86(STORE_42);
    let rdna2 = compile_rdna2(STORE_42);
    assert!(sm86.info.gpr_count > 0, "SM86 must use at least 1 GPR");
    assert!(rdna2.info.gpr_count > 0, "RDNA2 must use at least 1 GPR");
    assert!(sm86.info.instr_count > 0);
    assert!(rdna2.info.instr_count > 0);
}

#[test]
fn parity_store_42_binaries_differ() {
    let sm86 = compile_sm86(STORE_42);
    let rdna2 = compile_rdna2(STORE_42);
    assert_ne!(
        sm86.binary, rdna2.binary,
        "different ISAs should produce different binaries"
    );
}

#[test]
fn parity_store_99_both_compile() {
    let sm86 = compile_sm86(STORE_99);
    let rdna2 = compile_rdna2(STORE_99);
    assert!(!sm86.binary.is_empty());
    assert!(!rdna2.binary.is_empty());
}

#[test]
fn parity_store_99_binaries_differ() {
    let sm86 = compile_sm86(STORE_99);
    let rdna2 = compile_rdna2(STORE_99);
    assert_ne!(sm86.binary, rdna2.binary);
}

#[test]
fn parity_workgroup_size_1_matches() {
    let sm86 = compile_sm86(STORE_42);
    let rdna2 = compile_rdna2(STORE_42);
    assert_eq!(sm86.info.local_size, [1, 1, 1]);
    assert_eq!(rdna2.info.local_size, [1, 1, 1]);
}

// ============================================================
// Cross-target tests for shaders using global_invocation_id
// ============================================================

#[test]
fn parity_vecadd_both_compile() {
    let sm86 = compile_sm86(VECADD);
    let rdna2 = compile_rdna2(VECADD);
    assert!(!sm86.binary.is_empty());
    assert!(sm86.info.gpr_count >= 3);
    assert_eq!(sm86.info.local_size, [64, 1, 1]);
    assert!(!rdna2.binary.is_empty());
    assert_eq!(rdna2.info.local_size, [64, 1, 1]);
}

#[test]
fn parity_vecadd_binaries_differ() {
    let sm86 = compile_sm86(VECADD);
    let rdna2 = compile_rdna2(VECADD);
    assert_ne!(
        sm86.binary, rdna2.binary,
        "different ISAs should produce different binaries"
    );
}

#[test]
fn parity_saxpy_both_compile() {
    let sm86 = compile_sm86(SAXPY);
    let rdna2 = compile_rdna2(SAXPY);
    assert!(!sm86.binary.is_empty());
    assert!(!rdna2.binary.is_empty());
}

#[test]
fn parity_matmul_both_compile() {
    let sm86 = compile_sm86(MATMUL_TILE);
    let rdna2 = compile_rdna2(MATMUL_TILE);
    assert!(!sm86.binary.is_empty());
    assert!(sm86.info.gpr_count >= 4);
    assert_eq!(sm86.info.local_size, [8, 8, 1]);
    assert!(!rdna2.binary.is_empty());
    assert_eq!(rdna2.info.local_size, [8, 8, 1]);
}
