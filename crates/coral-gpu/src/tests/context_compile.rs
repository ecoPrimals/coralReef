// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2026 ecoPrimals

//! [`GpuContext`] compile paths without an attached device (or target-only).

use crate::GpuContext;
use coral_reef::{AmdArch, GpuTarget, NvArch};

#[test]
fn gpu_context_compile_only() {
    let ctx = GpuContext::new(GpuTarget::default()).unwrap();
    assert!(!ctx.has_device());
}

#[test]
fn gpu_context_compile_wgsl() {
    let ctx = GpuContext::new(GpuTarget::default()).unwrap();
    let kernel = ctx.compile_wgsl("@compute @workgroup_size(1) fn main() {}");
    assert!(kernel.is_ok());
    let k = kernel.unwrap();
    assert!(!k.binary.is_empty());
}

#[test]
fn gpu_context_amd_compile() {
    let ctx = GpuContext::new(GpuTarget::Amd(AmdArch::Rdna2)).unwrap();
    let kernel = ctx.compile_wgsl("@compute @workgroup_size(1) fn main() {}");
    assert!(kernel.is_ok());
}

#[test]
fn compiled_kernel_has_target() {
    let ctx = GpuContext::new(GpuTarget::Nvidia(NvArch::Sm86)).unwrap();
    let kernel = ctx
        .compile_wgsl("@compute @workgroup_size(1) fn main() {}")
        .unwrap();
    assert!(matches!(kernel.target, GpuTarget::Nvidia(NvArch::Sm86)));
}
