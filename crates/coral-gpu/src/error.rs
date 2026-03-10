// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2026 ecoPrimals

/// Errors from the unified GPU abstraction.
#[derive(Debug, thiserror::Error)]
pub enum GpuError {
    /// WGSL/SPIR-V parsing or codegen failed.
    #[error("compilation error: {0}")]
    Compile(#[from] coral_reef::CompileError),

    /// Low-level driver ioctl or device operation failed.
    #[error("driver error: {0}")]
    Driver(#[from] coral_driver::DriverError),

    /// No suitable GPU found for the requested target or preference.
    #[error("no GPU device available for target {0}")]
    NoDevice(std::borrow::Cow<'static, str>),

    /// Context has no device attached; call `auto()` or `with_device()` first.
    #[error("no device attached — call `auto()` or `with_device()` to bind hardware")]
    NoDeviceAttached,
}

/// Result type for GPU operations.
pub type GpuResult<T> = Result<T, GpuError>;
