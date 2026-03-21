// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2026 ecoPrimals

//! Buffer and dispatch behavior with a mock [`coral_driver::ComputeDevice`].

use crate::GpuContext;
use crate::error::GpuError;
use coral_driver::{DriverError, MemoryDomain};
use coral_reef::{GpuTarget, NvArch};

use super::common::ctx_with_mock;

#[test]
fn alloc_upload_readback_roundtrip() {
    let mut ctx = ctx_with_mock();
    let buf = ctx.alloc(16).unwrap();
    let data = b"hello world!!!!";
    ctx.upload(buf, data).unwrap();
    let out = ctx.readback(buf, data.len()).unwrap();
    assert_eq!(out, data);
}

#[test]
fn alloc_in_different_domains() {
    let mut ctx = ctx_with_mock();
    let vram = ctx.alloc_in(64, MemoryDomain::Vram).unwrap();
    let gtt = ctx.alloc_in(64, MemoryDomain::Gtt).unwrap();
    let either = ctx.alloc_in(64, MemoryDomain::VramOrGtt).unwrap();
    assert_ne!(vram, gtt);
    assert_ne!(gtt, either);
    ctx.upload(vram, b"vram").unwrap();
    ctx.upload(gtt, b"gtt").unwrap();
    assert_eq!(ctx.readback(vram, 4).unwrap(), b"vram");
    assert_eq!(ctx.readback(gtt, 3).unwrap(), b"gtt");
}

#[test]
fn dispatch_with_compiled_kernel() {
    let mut ctx = ctx_with_mock();
    let kernel = ctx
        .compile_wgsl("@compute @workgroup_size(1) fn main() {}")
        .unwrap();
    let buf = ctx.alloc(64).unwrap();
    ctx.dispatch(&kernel, &[buf], [1, 1, 1]).unwrap();
}

#[test]
fn dispatch_with_multiple_buffers() {
    let mut ctx = ctx_with_mock();
    let kernel = ctx
        .compile_wgsl("@compute @workgroup_size(1) fn main() {}")
        .unwrap();
    let a = ctx.alloc(32).unwrap();
    let b = ctx.alloc(32).unwrap();
    ctx.dispatch(&kernel, &[a, b], [1, 1, 1]).unwrap();
}

#[test]
fn free_then_use_freed_buffer_fails() {
    let mut ctx = ctx_with_mock();
    let buf = ctx.alloc(64).unwrap();
    ctx.free(buf).unwrap();
    let err = ctx.upload(buf, b"x").unwrap_err();
    assert!(matches!(
        err,
        GpuError::Driver(DriverError::BufferNotFound(_))
    ));
    let err = ctx.readback(buf, 1).unwrap_err();
    assert!(matches!(
        err,
        GpuError::Driver(DriverError::BufferNotFound(_))
    ));
}

#[test]
fn compile_wgsl_dispatch_sync_pipeline() {
    let mut ctx = ctx_with_mock();
    let kernel = ctx
        .compile_wgsl("@compute @workgroup_size(64) fn main() {}")
        .unwrap();
    let buf = ctx.alloc(256).unwrap();
    ctx.dispatch(&kernel, &[buf], [4, 1, 1]).unwrap();
    ctx.sync().unwrap();
}

#[test]
fn readback_returns_correct_data() {
    let mut ctx = ctx_with_mock();
    let buf = ctx.alloc(8).unwrap();
    ctx.upload(buf, &[1, 2, 3, 4, 5, 6, 7, 8]).unwrap();
    let out = ctx.readback(buf, 8).unwrap();
    assert_eq!(out, [1, 2, 3, 4, 5, 6, 7, 8]);
}

#[test]
fn has_device_returns_true_when_attached() {
    let ctx = ctx_with_mock();
    assert!(ctx.has_device());
}

#[test]
fn compile_spirv_method() {
    let ctx = GpuContext::new(GpuTarget::Nvidia(NvArch::Sm70)).unwrap();
    let invalid = [0x0723_0203_u32, 0x0001_0000, 0, 0, 0];
    let r = ctx.compile_spirv(&invalid);
    assert!(r.is_err());
}
