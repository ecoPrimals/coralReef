// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2026 ecoPrimals

//! [`GpuContext`] accessors, compile error paths, SPIR-V path, and dispatch with cache entries.

use crate::GpuContext;
use crate::error::GpuError;
use coral_reef::{GpuTarget, NvArch};

use super::common::{ctx_with_mock, wgsl_to_spirv_words};

#[test]
fn gpu_context_target_accessor() {
    let ctx = GpuContext::new(GpuTarget::Nvidia(NvArch::Sm89)).unwrap();
    assert_eq!(ctx.target(), GpuTarget::Nvidia(NvArch::Sm89));
}

#[test]
fn compile_wgsl_empty_source_errors() {
    let ctx = GpuContext::new(GpuTarget::default()).unwrap();
    let err = ctx.compile_wgsl("").unwrap_err();
    assert!(matches!(err, GpuError::Compile(_)));
}

#[test]
fn compile_wgsl_invalid_syntax_errors() {
    let ctx = GpuContext::new(GpuTarget::default()).unwrap();
    let err = ctx.compile_wgsl("not valid wgsl {{{").unwrap_err();
    assert!(matches!(err, GpuError::Compile(_)));
}

#[test]
fn compile_spirv_empty_errors() {
    let ctx = GpuContext::new(GpuTarget::Nvidia(NvArch::Sm70)).unwrap();
    let err = ctx.compile_spirv(&[]).unwrap_err();
    assert!(matches!(err, GpuError::Compile(_)));
}

#[test]
fn compile_spirv_minimal_compute_populates_kernel() {
    let spirv = wgsl_to_spirv_words("@compute @workgroup_size(1) fn main() {}");
    let ctx = GpuContext::new(GpuTarget::Nvidia(NvArch::Sm70)).unwrap();
    let kernel = ctx.compile_spirv(&spirv).unwrap();
    assert!(!kernel.binary.is_empty());
    assert_eq!(kernel.source_hash, 0);
    assert_eq!(kernel.gpr_count, 0);
    assert_eq!(kernel.workgroup, [1, 1, 1]);
    assert_eq!(kernel.target, GpuTarget::Nvidia(NvArch::Sm70));
}

#[test]
fn compile_wgsl_same_source_same_hash() {
    let ctx_a = GpuContext::new(GpuTarget::Nvidia(NvArch::Sm86)).unwrap();
    let ctx_b = GpuContext::new(GpuTarget::Nvidia(NvArch::Sm86)).unwrap();
    let src = "@compute @workgroup_size(8) fn main() {}";
    let k_a = ctx_a.compile_wgsl(src).unwrap();
    let k_b = ctx_b.compile_wgsl(src).unwrap();
    assert_eq!(k_a.source_hash, k_b.source_hash);
}

#[test]
fn dispatch_precompiled_roundtrip() {
    let mut ctx = ctx_with_mock();
    let kernel = ctx
        .compile_wgsl("@compute @workgroup_size(1) fn main() {}")
        .unwrap();
    let entry = kernel.to_cache_entry();
    let buf = ctx.alloc(16).unwrap();
    ctx.dispatch_precompiled(&entry, &[buf], [1, 1, 1]).unwrap();
}
