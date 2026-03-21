// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2026 ecoPrimals

//! [`crate::FmaCapability`] and policy for GPU targets.

use crate::FmaCapability;
use coral_reef::{AmdArch, FmaPolicy, GpuTarget, IntelArch, NvArch};

use super::common::ctx_with_mock;

#[test]
fn fma_capability_nvidia_sm70_has_dfma() {
    let cap = FmaCapability::for_target(GpuTarget::Nvidia(NvArch::Sm70));
    assert!(cap.f32_fma);
    assert!(cap.f64_fma, "SM70 (Volta) has DFMA");
    assert_eq!(cap.recommended_policy, FmaPolicy::Auto);
    assert!(cap.f32_fma_throughput_ratio > 1.0);
}

#[test]
fn fma_capability_nvidia_sm75_has_dfma() {
    let cap = FmaCapability::for_target(GpuTarget::Nvidia(NvArch::Sm75));
    assert!(cap.f32_fma);
    assert!(cap.f64_fma, "SM75 (Turing) has DFMA at 1/32 rate");
}

#[test]
fn fma_capability_nvidia_sm80_has_dfma() {
    let cap = FmaCapability::for_target(GpuTarget::Nvidia(NvArch::Sm80));
    assert!(cap.f64_fma, "SM80 (Ampere) has DFMA");
}

#[test]
fn fma_capability_amd_rdna2() {
    let cap = FmaCapability::for_target(GpuTarget::Amd(AmdArch::Rdna2));
    assert!(cap.f32_fma);
    assert!(cap.f64_fma, "RDNA2 has native f64");
}

#[test]
fn fma_capability_amd_rdna3() {
    let cap = FmaCapability::for_target(GpuTarget::Amd(AmdArch::Rdna3));
    assert!(cap.f32_fma);
    assert!(cap.f64_fma, "RDNA3 has native f64");
    assert!(cap.f32_fma_throughput_ratio > 1.0);
}

#[test]
fn fma_capability_via_context() {
    let ctx = ctx_with_mock();
    let cap = ctx.fma_capability();
    assert!(cap.f32_fma);
    assert_eq!(cap.recommended_policy, FmaPolicy::Auto);
}

#[test]
fn fma_capability_debug_format() {
    let cap = FmaCapability::for_target(GpuTarget::Nvidia(NvArch::Sm70));
    let debug = format!("{cap:?}");
    assert!(debug.contains("FmaCapability"));
    assert!(debug.contains("f32_fma"));
}

#[test]
fn fma_capability_intel_planned_target() {
    let cap = FmaCapability::for_target(GpuTarget::Intel(IntelArch::XeHpg));
    assert!(cap.f32_fma);
    assert!(!cap.f64_fma);
    assert_eq!(cap.recommended_policy, FmaPolicy::Auto);
    const INTEL_FMA_RATIO_TOLERANCE: f32 = 1e-6;
    assert!(
        (cap.f32_fma_throughput_ratio - 1.0).abs() < INTEL_FMA_RATIO_TOLERANCE,
        "expected 1.0 throughput ratio for Intel placeholder, got {}",
        cap.f32_fma_throughput_ratio
    );
}

#[test]
fn fma_capability_amd_rdna4() {
    let cap = FmaCapability::for_target(GpuTarget::Amd(AmdArch::Rdna4));
    assert!(cap.f32_fma);
    assert!(cap.f64_fma);
}

#[test]
fn fma_capability_nvidia_sm89_dfma() {
    let cap = FmaCapability::for_target(GpuTarget::Nvidia(NvArch::Sm89));
    assert!(cap.f64_fma);
}

#[test]
fn fma_capability_nvidia_sm86_matches_dfma_model() {
    let cap = FmaCapability::for_target(GpuTarget::Nvidia(NvArch::Sm86));
    assert!(cap.f32_fma);
    assert!(cap.f64_fma);
    assert_eq!(cap.f32_fma_throughput_ratio, 2.0);
}

#[test]
fn fma_capability_all_nvidia_archs_report_dfma() {
    for &nv in NvArch::ALL {
        let cap = FmaCapability::for_target(GpuTarget::Nvidia(nv));
        assert!(cap.f64_fma, "{nv:?} should report DFMA");
    }
}

#[test]
fn fma_capability_amd_rdna3_rdna4_throughput() {
    let rdna3 = FmaCapability::for_target(GpuTarget::Amd(AmdArch::Rdna3));
    let rdna4 = FmaCapability::for_target(GpuTarget::Amd(AmdArch::Rdna4));
    const EPS: f32 = 1e-6;
    assert!((rdna3.f32_fma_throughput_ratio - 2.0).abs() < EPS);
    assert!((rdna4.f32_fma_throughput_ratio - 2.0).abs() < EPS);
    assert!(rdna3.f64_fma && rdna4.f64_fma);
}
