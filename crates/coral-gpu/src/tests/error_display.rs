// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright © 2026 ecoPrimals

//! Human-readable [`crate::GpuError`] messages.

use crate::GpuContext;
use crate::error::GpuError;
use coral_driver::{BufferHandle, DriverError};
use coral_reef::GpuTarget;

#[test]
fn gpu_error_compile_display_contains_compilation() {
    let err = GpuContext::new(GpuTarget::default())
        .unwrap()
        .compile_wgsl("")
        .unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.starts_with("compilation error:"),
        "expected prefix, got: {msg}"
    );
}

#[test]
fn gpu_error_no_device_display() {
    let err = GpuError::NoDevice(std::borrow::Cow::Borrowed("missing"));
    let msg = err.to_string();
    assert!(
        msg.starts_with("no GPU device available for target "),
        "got: {msg}"
    );
    assert!(msg.contains("missing"), "got: {msg}");
}

#[test]
fn gpu_error_no_device_attached_display() {
    let err = GpuError::NoDeviceAttached;
    assert_eq!(
        err.to_string(),
        "no device attached — call `auto()` or `with_device()` to bind hardware"
    );
}

#[test]
fn gpu_error_driver_display() {
    let err = GpuError::Driver(DriverError::BufferNotFound(BufferHandle::from_id(99)));
    let msg = err.to_string();
    assert!(
        msg.starts_with("driver error:"),
        "expected prefix, got: {msg}"
    );
    assert!(msg.contains("buffer not found"), "got: {msg}");
}
