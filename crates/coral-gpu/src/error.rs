// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2026 ecoPrimals

/// Errors from the unified GPU abstraction.
#[derive(Debug, thiserror::Error)]
pub enum GpuError {
    #[error("compilation error: {0}")]
    Compile(#[from] coral_reef::CompileError),

    #[error("driver error: {0}")]
    Driver(#[from] coral_driver::DriverError),

    #[error("no GPU device available for target {0}")]
    NoDevice(std::borrow::Cow<'static, str>),

    #[error("no device attached — call `auto()` or `with_device()` to bind hardware")]
    NoDeviceAttached,
}

pub type GpuResult<T> = Result<T, GpuError>;
