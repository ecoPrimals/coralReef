// SPDX-License-Identifier: AGPL-3.0-only
//! Driver error types.

use std::borrow::Cow;

/// Result alias for driver operations.
///
/// All GPU device operations return this type; errors are [`DriverError`] variants.
pub type DriverResult<T> = Result<T, DriverError>;

/// Errors from GPU device operations.
///
/// String-carrying variants use `Cow<'static, str>` so that static messages
/// (the common case) are zero-alloc, while dynamic messages still work via
/// `format!("...").into()`.
#[derive(Debug, thiserror::Error)]
pub enum DriverError {
    /// No matching GPU device was found (e.g. no amdgpu/nouveau render node).
    #[error("device not found: {0}")]
    DeviceNotFound(Cow<'static, str>),

    /// A DRM ioctl syscall failed; the kernel returned an error.
    #[error("DRM ioctl failed: {name} returned {errno}")]
    IoctlFailed {
        /// Name of the ioctl for error reporting.
        name: &'static str,
        /// Kernel errno (negative on Linux).
        errno: i32,
    },

    /// Buffer allocation failed (OOM or invalid domain).
    #[error("buffer allocation failed: size={size}, domain={domain:?} — {detail}")]
    AllocFailed {
        /// Requested buffer size in bytes.
        size: u64,
        /// Memory domain that was requested.
        domain: crate::MemoryDomain,
        /// Additional context.
        detail: String,
    },

    /// The buffer handle is invalid or was already freed.
    #[error("buffer not found: handle={0:?}")]
    BufferNotFound(crate::BufferHandle),

    /// Memory mapping of a GEM buffer failed.
    #[error("mmap failed: {0}")]
    MmapFailed(Cow<'static, str>),

    /// Command submission to the GPU failed.
    #[error("command submission failed: {0}")]
    SubmitFailed(Cow<'static, str>),

    /// The fence did not signal within the timeout period.
    #[error("fence timeout after {ms}ms")]
    FenceTimeout {
        /// Timeout duration in milliseconds.
        ms: u64,
    },

    /// Device open / context creation failed.
    #[error("device open failed: {0}")]
    OpenFailed(Cow<'static, str>),

    /// Compute dispatch (kernel launch) failed.
    #[error("dispatch failed: {0}")]
    DispatchFailed(Cow<'static, str>),

    /// GPU synchronization (fence / stream sync) failed.
    #[error("sync failed: {0}")]
    SyncFailed(Cow<'static, str>),

    /// Oracle / BAR0 register operation failed (page table walk, PMU probe, etc.).
    #[error("oracle error: {0}")]
    OracleError(Cow<'static, str>),

    /// Wrapped I/O error from file operations.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

impl DriverError {
    /// Platform overflow during numeric conversion (e.g. `usize`→`u64`, `u64`→`off_t`).
    /// Used for conversions that cannot fail on 64-bit Linux but should still
    /// propagate as errors rather than panicking.
    pub(crate) fn platform_overflow(msg: &'static str) -> Self {
        Self::MmapFailed(msg.into())
    }

    /// Create an oracle error from a dynamic string (bridges `Result<T, String>`
    /// from the oracle module into `DriverResult`).
    pub fn oracle(msg: impl Into<Cow<'static, str>>) -> Self {
        Self::OracleError(msg.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::error::Error;

    #[test]
    fn error_display_device_not_found() {
        let e = DriverError::DeviceNotFound("no amdgpu".into());
        assert!(e.to_string().contains("no amdgpu"));
    }

    #[test]
    fn error_display_ioctl_failed() {
        let e = DriverError::IoctlFailed {
            name: "drm_ioctl",
            errno: -22,
        };
        let msg = e.to_string();
        assert!(msg.contains("drm_ioctl"));
        assert!(msg.contains("-22"));
    }

    #[test]
    fn error_display_alloc_failed() {
        let e = DriverError::AllocFailed {
            size: 4096,
            domain: crate::MemoryDomain::Vram,
            detail: "oom".into(),
        };
        assert!(e.to_string().contains("4096"));
    }

    #[test]
    fn error_display_buffer_not_found() {
        let e = DriverError::BufferNotFound(crate::BufferHandle(42));
        assert!(e.to_string().contains("42"));
    }

    #[test]
    fn error_display_mmap_failed() {
        let e = DriverError::MmapFailed("out of memory".into());
        assert!(e.to_string().contains("out of memory"));
    }

    #[test]
    fn error_display_submit_failed() {
        let e = DriverError::SubmitFailed("context lost".into());
        assert!(e.to_string().contains("context lost"));
    }

    #[test]
    fn error_display_fence_timeout() {
        let e = DriverError::FenceTimeout { ms: 5000 };
        assert!(e.to_string().contains("5000"));
    }

    #[test]
    fn error_from_io() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "no device");
        let e: DriverError = io_err.into();
        assert!(e.to_string().contains("no device"));
    }

    #[test]
    fn error_is_std_error() {
        let e: Box<dyn std::error::Error> = Box::new(DriverError::DeviceNotFound("test".into()));
        assert!(e.to_string().contains("test"));
    }

    #[test]
    fn error_platform_overflow() {
        let e = DriverError::platform_overflow("offset exceeds platform pointer width");
        let msg = e.to_string();
        assert!(msg.contains("offset exceeds platform pointer width"));
    }

    #[test]
    fn error_alloc_failed_domain_display() {
        for domain in [
            crate::MemoryDomain::Vram,
            crate::MemoryDomain::Gtt,
            crate::MemoryDomain::VramOrGtt,
        ] {
            let e = DriverError::AllocFailed { size: 8192, domain, detail: "test".into() };
            let msg = e.to_string();
            assert!(msg.contains("8192"));
            assert!(msg.contains("domain"));
        }
    }

    #[test]
    fn error_debug_format() {
        let e = DriverError::DeviceNotFound("probe failed".into());
        let debug = format!("{e:?}");
        assert!(debug.contains("DeviceNotFound"));
        assert!(debug.contains("probe failed"));
    }

    #[test]
    fn error_display_dynamic_cow() {
        let msg = format!("custom error: {}", 42);
        let e = DriverError::MmapFailed(msg.into());
        assert!(e.to_string().contains("custom error: 42"));
    }

    #[test]
    fn error_display_device_not_found_static() {
        let e = DriverError::DeviceNotFound(Cow::Borrowed("static message"));
        assert_eq!(e.to_string(), "device not found: static message");
    }

    #[test]
    fn error_source_chain() {
        let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "root required");
        let e: DriverError = io_err.into();
        let source = e.source();
        assert!(source.is_some());
        assert!(source.unwrap().to_string().contains("root required"));
    }

    #[test]
    fn error_display_io_variant() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let e: DriverError = io_err.into();
        let msg = e.to_string();
        assert!(msg.contains("I/O"), "Io variant should display 'I/O'");
        assert!(msg.contains("file not found"));
    }

    #[test]
    fn error_from_io_conversion() {
        let inner = std::io::Error::new(std::io::ErrorKind::WouldBlock, "would block");
        let e: DriverError = DriverError::from(inner);
        assert!(matches!(e, DriverError::Io(_)));
        assert!(e.to_string().contains("would block"));
    }

    #[test]
    fn error_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<DriverError>();
    }
}
