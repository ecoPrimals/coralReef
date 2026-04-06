// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright © 2026 ecoPrimals

//! Operations that require hardware when no device is attached.

use crate::GpuContext;
use crate::error::GpuError;
use coral_driver::BufferHandle;
use coral_reef::GpuTarget;

#[test]
fn alloc_fails_without_device() {
    let mut ctx = GpuContext::new(GpuTarget::default()).unwrap();
    let err = ctx.alloc(1024).unwrap_err();
    assert!(matches!(err, GpuError::NoDeviceAttached));
}

#[test]
fn dispatch_fails_without_device() {
    let mut ctx = GpuContext::new(GpuTarget::default()).unwrap();
    let kernel = ctx
        .compile_wgsl("@compute @workgroup_size(1) fn main() {}")
        .unwrap();
    let err = ctx.dispatch(&kernel, &[], [1, 1, 1]).unwrap_err();
    assert!(matches!(err, GpuError::NoDeviceAttached));
}

#[test]
fn sync_fails_without_device() {
    let mut ctx = GpuContext::new(GpuTarget::default()).unwrap();
    let err = ctx.sync().unwrap_err();
    assert!(matches!(err, GpuError::NoDeviceAttached));
}

#[test]
fn readback_fails_without_device() {
    let ctx = GpuContext::new(GpuTarget::default()).unwrap();
    let buf = BufferHandle::from_id(1);
    let err = ctx.readback(buf, 1).unwrap_err();
    assert!(matches!(err, GpuError::NoDeviceAttached));
}

#[test]
fn upload_fails_without_device() {
    let mut ctx = GpuContext::new(GpuTarget::default()).unwrap();
    let buf = BufferHandle::from_id(1);
    let err = ctx.upload(buf, b"x").unwrap_err();
    assert!(matches!(err, GpuError::NoDeviceAttached));
}

#[test]
fn free_fails_without_device() {
    let mut ctx = GpuContext::new(GpuTarget::default()).unwrap();
    let buf = BufferHandle::from_id(1);
    let err = ctx.free(buf).unwrap_err();
    assert!(matches!(err, GpuError::NoDeviceAttached));
}
