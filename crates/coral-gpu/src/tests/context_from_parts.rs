// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright © 2026 ecoPrimals

//! [`GpuContext::from_parts`] and [`GpuContext::compile_wgsl_cached`] integration with mocks.

use crate::GpuContext;
use coral_reef::{CompileOptions, FmaPolicy, GpuTarget, NvArch};

use super::common::MockDevice;

#[test]
fn from_parts_runs_compile_and_dispatch_with_mock() {
    let opts = CompileOptions {
        opt_level: 3,
        fma_policy: FmaPolicy::Separate,
        ..CompileOptions::default()
    };
    let mut ctx = GpuContext::from_parts(
        GpuTarget::Nvidia(NvArch::Sm86),
        Box::new(MockDevice::new()),
        opts,
    )
    .unwrap();
    assert_eq!(ctx.target(), GpuTarget::Nvidia(NvArch::Sm86));
    assert_eq!(ctx.compile_options().opt_level, 3);
    assert_eq!(ctx.compile_options().fma_policy, FmaPolicy::Separate);

    let kernel = ctx
        .compile_wgsl("@compute @workgroup_size(8) fn main() {}")
        .unwrap();
    let buf = ctx.alloc(64).unwrap();
    ctx.dispatch(&kernel, &[buf], [2, 1, 1]).unwrap();
    ctx.sync().unwrap();
}

#[test]
fn compile_wgsl_cached_second_call_reuses_binary() {
    let mut ctx = GpuContext::from_parts(
        GpuTarget::Nvidia(NvArch::Sm70),
        Box::new(MockDevice::new()),
        CompileOptions::default(),
    )
    .unwrap();
    let src = "@compute @workgroup_size(4) fn main() { var x: u32 = 1u; }";
    let a = ctx.compile_wgsl_cached(src).unwrap();
    let b = ctx.compile_wgsl_cached(src).unwrap();
    assert_eq!(a.source_hash, b.source_hash);
    assert_eq!(a.binary.as_ptr(), b.binary.as_ptr());
}

#[test]
fn multi_target_same_wgsl_records_distinct_targets() {
    let wgsl = "@compute @workgroup_size(1) fn main() {}";
    let k70 = GpuContext::new(GpuTarget::Nvidia(NvArch::Sm70))
        .unwrap()
        .compile_wgsl(wgsl)
        .unwrap();
    let k86 = GpuContext::new(GpuTarget::Nvidia(NvArch::Sm86))
        .unwrap()
        .compile_wgsl(wgsl)
        .unwrap();
    assert_ne!(k70.target, k86.target);
    assert!(!k70.binary.is_empty());
    assert!(!k86.binary.is_empty());
    assert_ne!(
        k70.to_cache_entry().target_id,
        k86.to_cache_entry().target_id
    );
}
