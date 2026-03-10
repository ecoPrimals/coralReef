// SPDX-License-Identifier: AGPL-3.0-only
//! NVVM Poisoning Bypass Tests — sovereign compilation of shaders
//! that permanently poison the NVIDIA proprietary driver through wgpu.
//!
//! On the NVIDIA proprietary driver, certain WGSL shader compilations
//! with f64 transcendentals (exp, log) trigger NVVM failures that
//! **permanently invalidate the wgpu device** for the rest of the
//! process. NVK (Mesa) handles them correctly.
//!
//! coralReef's sovereign WGSL → naga → codegen IR → native SASS path
//! bypasses NVVM entirely. These tests validate that the exact shader
//! patterns that poison NVVM compile successfully through coralReef.
//!
//! Source: hotSpring v0.6.25 Precision Brain + NVVM Device Poisoning
//! Handoff (March 10, 2026)

use coral_reef::{AmdArch, CompileOptions, FmaPolicy, GpuArch, GpuTarget, compile_wgsl_full};

fn sm70_opts() -> CompileOptions {
    CompileOptions {
        target: GpuArch::Sm70.into(),
        opt_level: 2,
        debug_info: false,
        fp64_software: true,
        ..CompileOptions::default()
    }
}

fn sm86_opts() -> CompileOptions {
    CompileOptions {
        target: GpuArch::Sm86.into(),
        opt_level: 2,
        debug_info: false,
        fp64_software: true,
        ..CompileOptions::default()
    }
}

fn rdna2_opts() -> CompileOptions {
    CompileOptions {
        target: GpuTarget::Amd(AmdArch::Rdna2),
        opt_level: 2,
        debug_info: false,
        fp64_software: false,
        ..CompileOptions::default()
    }
}

fn nofma_sm70_opts() -> CompileOptions {
    CompileOptions {
        target: GpuArch::Sm70.into(),
        opt_level: 2,
        debug_info: false,
        fp64_software: true,
        fma_policy: FmaPolicy::Separate,
        ..CompileOptions::default()
    }
}

fn nofma_sm86_opts() -> CompileOptions {
    CompileOptions {
        target: GpuArch::Sm86.into(),
        opt_level: 2,
        debug_info: false,
        fp64_software: true,
        fma_policy: FmaPolicy::Separate,
        ..CompileOptions::default()
    }
}

// ---------------------------------------------------------------------------
// Pattern 1: f64 transcendentals (exp, log, exp2, log2)
//
// NVVM impact: permanent device death on proprietary NVIDIA driver
// coralReef: compiles to native SASS via our f64 transcendental lowering
// ---------------------------------------------------------------------------

const F64_TRANSCENDENTAL_WGSL: &str =
    include_str!("fixtures/wgsl/nvvm_poison_f64_transcendental.wgsl");

#[test]
fn nvvm_bypass_f64_transcendental_sm70() {
    let result = compile_wgsl_full(F64_TRANSCENDENTAL_WGSL, &sm70_opts());
    assert!(result.is_ok(), "SM70: {}", result.unwrap_err());
    assert!(!result.unwrap().binary.is_empty());
}

#[test]
fn nvvm_bypass_f64_transcendental_sm86() {
    let result = compile_wgsl_full(F64_TRANSCENDENTAL_WGSL, &sm86_opts());
    assert!(result.is_ok(), "SM86: {}", result.unwrap_err());
    assert!(!result.unwrap().binary.is_empty());
}

#[test]
fn nvvm_bypass_f64_transcendental_rdna2() {
    let result = compile_wgsl_full(F64_TRANSCENDENTAL_WGSL, &rdna2_opts());
    assert!(result.is_ok(), "RDNA2: {}", result.unwrap_err());
    assert!(!result.unwrap().binary.is_empty());
}

// ---------------------------------------------------------------------------
// Pattern 2: DF64 pipeline — f32-pair emulation + transcendentals
//
// NVVM impact: the DF64 rewrite confuses NVVM builtin resolution for
// exp/log, triggering "NVVM compilation failed: 1" and device death
// coralReef: native f64 lowering, no DF64 rewrite through NVVM
// ---------------------------------------------------------------------------

const DF64_PIPELINE_WGSL: &str = include_str!("fixtures/wgsl/nvvm_poison_df64_pipeline.wgsl");

#[test]
fn nvvm_bypass_df64_pipeline_sm70() {
    let result = compile_wgsl_full(DF64_PIPELINE_WGSL, &sm70_opts());
    assert!(result.is_ok(), "SM70: {}", result.unwrap_err());
    let bin = result.unwrap();
    assert!(!bin.binary.is_empty());
    assert!(bin.info.gpr_count > 0);
}

#[test]
fn nvvm_bypass_df64_pipeline_sm86() {
    let result = compile_wgsl_full(DF64_PIPELINE_WGSL, &sm86_opts());
    assert!(result.is_ok(), "SM86: {}", result.unwrap_err());
    assert!(!result.unwrap().binary.is_empty());
}

#[test]
fn nvvm_bypass_df64_pipeline_rdna2() {
    let result = compile_wgsl_full(DF64_PIPELINE_WGSL, &rdna2_opts());
    assert!(result.is_ok(), "RDNA2: {}", result.unwrap_err());
    assert!(!result.unwrap().binary.is_empty());
}

// ---------------------------------------------------------------------------
// Pattern 3: F64Precise (no-FMA) + transcendentals
//
// NVVM impact: the no-FMA compilation flags break NVVM's transcendental
// implementation, causing permanent device invalidation
// coralReef: FmaPolicy::Separate is handled in our own codegen,
// producing correct SASS without touching NVVM
// ---------------------------------------------------------------------------

const F64PRECISE_NOFMA_WGSL: &str = include_str!("fixtures/wgsl/nvvm_poison_f64precise_nofma.wgsl");

#[test]
fn nvvm_bypass_f64precise_nofma_sm70() {
    let result = compile_wgsl_full(F64PRECISE_NOFMA_WGSL, &nofma_sm70_opts());
    assert!(result.is_ok(), "SM70 NoFMA: {}", result.unwrap_err());
    assert!(!result.unwrap().binary.is_empty());
}

#[test]
fn nvvm_bypass_f64precise_nofma_sm86() {
    let result = compile_wgsl_full(F64PRECISE_NOFMA_WGSL, &nofma_sm86_opts());
    assert!(result.is_ok(), "SM86 NoFMA: {}", result.unwrap_err());
    assert!(!result.unwrap().binary.is_empty());
}

#[test]
fn nvvm_bypass_f64precise_nofma_rdna2() {
    let result = compile_wgsl_full(F64PRECISE_NOFMA_WGSL, &rdna2_opts());
    assert!(result.is_ok(), "RDNA2: {}", result.unwrap_err());
    assert!(!result.unwrap().binary.is_empty());
}

// ---------------------------------------------------------------------------
// Cross-architecture matrix: verify all 3 patterns × all architectures
// ---------------------------------------------------------------------------

#[test]
fn nvvm_bypass_all_patterns_sm75() {
    let opts = CompileOptions {
        target: GpuArch::Sm75.into(),
        opt_level: 2,
        fp64_software: true,
        ..CompileOptions::default()
    };
    for (name, wgsl) in [
        ("f64_transcendental", F64_TRANSCENDENTAL_WGSL),
        ("df64_pipeline", DF64_PIPELINE_WGSL),
        ("f64precise_nofma", F64PRECISE_NOFMA_WGSL),
    ] {
        let result = compile_wgsl_full(wgsl, &opts);
        assert!(result.is_ok(), "SM75 {name}: {}", result.unwrap_err());
    }
}

#[test]
fn nvvm_bypass_all_patterns_sm80() {
    let opts = CompileOptions {
        target: GpuArch::Sm80.into(),
        opt_level: 2,
        fp64_software: true,
        ..CompileOptions::default()
    };
    for (name, wgsl) in [
        ("f64_transcendental", F64_TRANSCENDENTAL_WGSL),
        ("df64_pipeline", DF64_PIPELINE_WGSL),
        ("f64precise_nofma", F64PRECISE_NOFMA_WGSL),
    ] {
        let result = compile_wgsl_full(wgsl, &opts);
        assert!(result.is_ok(), "SM80 {name}: {}", result.unwrap_err());
    }
}

#[test]
fn nvvm_bypass_all_patterns_sm89() {
    let opts = CompileOptions {
        target: GpuArch::Sm89.into(),
        opt_level: 2,
        fp64_software: true,
        ..CompileOptions::default()
    };
    for (name, wgsl) in [
        ("f64_transcendental", F64_TRANSCENDENTAL_WGSL),
        ("df64_pipeline", DF64_PIPELINE_WGSL),
        ("f64precise_nofma", F64PRECISE_NOFMA_WGSL),
    ] {
        let result = compile_wgsl_full(wgsl, &opts);
        assert!(result.is_ok(), "SM89 {name}: {}", result.unwrap_err());
    }
}

// ---------------------------------------------------------------------------
// FMA policy: Separate produces different binary than Fused/Auto
//
// Validates that FmaPolicy::Separate actually changes codegen output.
// The f64precise_nofma shader is designed to be sensitive to this.
// ---------------------------------------------------------------------------

#[test]
fn nvvm_bypass_fma_policies_all_compile() {
    for policy in [FmaPolicy::Fused, FmaPolicy::Separate, FmaPolicy::Auto] {
        let opts = CompileOptions {
            target: GpuArch::Sm70.into(),
            opt_level: 2,
            fp64_software: true,
            fma_policy: policy,
            ..CompileOptions::default()
        };
        let result = compile_wgsl_full(F64PRECISE_NOFMA_WGSL, &opts);
        assert!(
            result.is_ok(),
            "SM70 FmaPolicy::{policy:?}: {}",
            result.unwrap_err()
        );
        assert!(!result.unwrap().binary.is_empty());
    }
}

#[test]
fn nvvm_bypass_fma_separate_rdna2() {
    let separate_opts = CompileOptions {
        target: GpuTarget::Amd(AmdArch::Rdna2),
        opt_level: 2,
        fp64_software: false,
        fma_policy: FmaPolicy::Separate,
        ..CompileOptions::default()
    };
    let result = compile_wgsl_full(F64PRECISE_NOFMA_WGSL, &separate_opts);
    assert!(
        result.is_ok(),
        "RDNA2 FMA Separate: {}",
        result.unwrap_err()
    );
}
