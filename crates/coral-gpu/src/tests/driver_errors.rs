// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2026 ecoPrimals

//! Driver error propagation through [`crate::GpuContext`] with [`super::common::FailingMockDevice`].

use crate::GpuContext;
use crate::error::GpuError;
use coral_driver::DriverError;
use coral_reef::GpuTarget;

use super::common::FailingMockDevice;

#[test]
fn alloc_error_propagates() {
    let mut ctx = GpuContext::with_device(
        GpuTarget::default(),
        Box::new(FailingMockDevice::new().fail_alloc()),
    )
    .unwrap();
    let err = ctx.alloc(1024).unwrap_err();
    assert!(matches!(
        err,
        GpuError::Driver(DriverError::AllocFailed { .. })
    ));
}

#[test]
fn free_error_propagates() {
    let mut ctx = GpuContext::with_device(
        GpuTarget::default(),
        Box::new(FailingMockDevice::new().fail_free()),
    )
    .unwrap();
    let buf = ctx.alloc(64).unwrap();
    let err = ctx.free(buf).unwrap_err();
    assert!(matches!(
        err,
        GpuError::Driver(DriverError::BufferNotFound(_))
    ));
}

#[test]
fn upload_error_propagates() {
    let mut ctx = GpuContext::with_device(
        GpuTarget::default(),
        Box::new(FailingMockDevice::new().fail_upload()),
    )
    .unwrap();
    let buf = ctx.alloc(64).unwrap();
    let err = ctx.upload(buf, b"data").unwrap_err();
    assert!(matches!(err, GpuError::Driver(DriverError::MmapFailed(_))));
}

#[test]
fn readback_error_propagates() {
    let mut ctx = GpuContext::with_device(
        GpuTarget::default(),
        Box::new(FailingMockDevice::new().fail_readback()),
    )
    .unwrap();
    let buf = ctx.alloc(64).unwrap();
    ctx.upload(buf, b"x").unwrap();
    let err = ctx.readback(buf, 1).unwrap_err();
    assert!(matches!(err, GpuError::Driver(DriverError::MmapFailed(_))));
}

#[test]
fn dispatch_error_propagates() {
    let mut ctx = GpuContext::with_device(
        GpuTarget::default(),
        Box::new(FailingMockDevice::new().fail_dispatch()),
    )
    .unwrap();
    let kernel = ctx
        .compile_wgsl("@compute @workgroup_size(1) fn main() {}")
        .unwrap();
    let buf = ctx.alloc(64).unwrap();
    let err = ctx.dispatch(&kernel, &[buf], [1, 1, 1]).unwrap_err();
    assert!(matches!(
        err,
        GpuError::Driver(DriverError::SubmitFailed(_))
    ));
}

#[test]
fn dispatch_precompiled_error_propagates() {
    let mut ctx = GpuContext::with_device(
        GpuTarget::default(),
        Box::new(FailingMockDevice::new().fail_dispatch()),
    )
    .unwrap();
    let kernel = ctx
        .compile_wgsl("@compute @workgroup_size(1) fn main() {}")
        .unwrap();
    let entry = kernel.to_cache_entry();
    let buf = ctx.alloc(64).unwrap();
    let err = ctx
        .dispatch_precompiled(&entry, &[buf], [1, 1, 1])
        .unwrap_err();
    assert!(matches!(
        err,
        GpuError::Driver(DriverError::SubmitFailed(_))
    ));
}

#[test]
fn sync_error_propagates() {
    let mut ctx = GpuContext::with_device(
        GpuTarget::default(),
        Box::new(FailingMockDevice::new().fail_sync()),
    )
    .unwrap();
    let err = ctx.sync().unwrap_err();
    assert!(matches!(
        err,
        GpuError::Driver(DriverError::FenceTimeout { .. })
    ));
}
