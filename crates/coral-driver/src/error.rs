// SPDX-License-Identifier: AGPL-3.0-only
//! Driver error types.

/// Result alias for driver operations.
pub type DriverResult<T> = Result<T, DriverError>;

/// Errors from GPU device operations.
#[derive(Debug, thiserror::Error)]
pub enum DriverError {
    #[error("device not found: {0}")]
    DeviceNotFound(String),

    #[error("DRM ioctl failed: {name} returned {errno}")]
    IoctlFailed { name: &'static str, errno: i32 },

    #[error("buffer allocation failed: size={size}, domain={domain:?}")]
    AllocFailed {
        size: u64,
        domain: crate::MemoryDomain,
    },

    #[error("buffer not found: handle={0:?}")]
    BufferNotFound(crate::BufferHandle),

    #[error("mmap failed: {0}")]
    MmapFailed(String),

    #[error("command submission failed: {0}")]
    SubmitFailed(String),

    #[error("fence timeout after {ms}ms")]
    FenceTimeout { ms: u64 },

    #[error("unsupported operation: {0}")]
    Unsupported(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}
