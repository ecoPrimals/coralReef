// SPDX-License-Identifier: AGPL-3.0-or-later
//! Driver error types.

mod vfio;
pub use vfio::{ChannelError, DevinitError, PciDiscoveryError, SovereignStagesError};

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

    /// Operation or API not available for this device / backend (e.g. legacy VFIO group fd on iommufd).
    #[error("unsupported: {0}")]
    Unsupported(Cow<'static, str>),

    /// Hardware guard refused a register write to protect the GPU.
    ///
    /// This is a **non-recoverable** error — the software must abort
    /// the current init/dispatch sequence immediately. The GPU is either
    /// dead (PCIe link down) or the register is on the blocklist.
    #[error("hardware guard: {0}")]
    HardwareGuardRefusal(Cow<'static, str>),

    /// Device is exclusively held by a live coral-ember instance.
    ///
    /// Direct hardware access (VFIO open, sysfs BAR0 mmap) is blocked to
    /// prevent accidental probing that could kill fragile GPUs (e.g. K80
    /// through a PLX bridge). Use `EmberSession::connect()` or glowplug's
    /// `request_fds` to obtain access through ember's safety perimeter.
    #[error("device {bdf} is held by ember — use EmberSession::connect() instead of direct open")]
    DeviceHeldByEmber {
        /// PCI BDF address of the held device.
        bdf: String,
    },

    /// PCI sysfs/config-space discovery or PM transition failed.
    #[error("PCI discovery: {0}")]
    PciDiscovery(#[from] PciDiscoveryError),

    /// VFIO channel oracle / BAR0 resource access failed.
    #[error("channel: {0}")]
    Channel(#[from] ChannelError),

    /// VBIOS / devinit (PROM, interpreter, PMU upload) failed.
    #[error("devinit: {0}")]
    Devinit(#[from] DevinitError),

    /// Sovereign init stage helpers (BAR0 probe, memory training, falcon/GR boot, verify).
    #[error("sovereign stages: {0}")]
    SovereignStages(#[from] SovereignStagesError),
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
            let e = DriverError::AllocFailed {
                size: 8192,
                domain,
                detail: "test".into(),
            };
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
    fn error_display_unsupported() {
        let e = DriverError::Unsupported("legacy API on iommufd".into());
        assert!(e.to_string().contains("unsupported"));
        assert!(e.to_string().contains("legacy API"));
    }

    #[test]
    fn error_display_pci_discovery_variant() {
        let inner = PciDiscoveryError::InvalidBdf { bdf: "bad".into() };
        let e: DriverError = inner.into();
        assert!(e.to_string().contains("PCI discovery"));
        assert!(e.to_string().contains("bad"));
    }

    #[test]
    fn error_display_channel_variant() {
        let inner = ChannelError::Bar0ReadsAllOnes;
        let e: DriverError = inner.into();
        assert!(e.to_string().contains("channel"));
        assert!(e.to_string().contains("0xFFFFFFFF"));
    }

    #[test]
    fn error_display_devinit_variant() {
        let inner = DevinitError::BitSignatureNotFound;
        let e: DriverError = inner.into();
        assert!(e.to_string().contains("devinit"));
        assert!(e.to_string().contains("BIT"));
    }

    #[test]
    fn error_display_sovereign_stages_variant() {
        let inner = SovereignStagesError::Bar0ProbeTimeout;
        let e: DriverError = inner.into();
        assert!(e.to_string().contains("sovereign stages"));
        assert!(e.to_string().contains("BAR0"));
    }

    #[test]
    fn sovereign_stages_vfio_compute_preserves_source() {
        let inner = DriverError::DeviceNotFound(std::borrow::Cow::Borrowed("missing firmware"));
        let sse = SovereignStagesError::VfioCompute(Box::new(inner));
        let de: DriverError = sse.into();
        assert!(de.source().is_some());
        assert!(de.to_string().contains("sovereign stages"));
    }

    #[test]
    fn error_display_bar0_oob() {
        let e = ChannelError::Bar0ReadOutOfBounds {
            offset: 0x1000_0000,
            map_size: 0x0100_0000,
        };
        let s = e.to_string();
        assert!(s.contains("read out of bounds"));
        assert!(s.contains("10000000"));
    }

    #[test]
    #[cfg(feature = "vfio")]
    fn error_channel_resource_io_source() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "missing");
        let e = ChannelError::resource_io("read", "/tmp/x", io_err);
        let s = e.to_string();
        assert!(s.contains("read"));
        assert!(s.contains("/tmp/x"));
        let de: DriverError = e.into();
        assert!(de.source().is_some());
    }

    #[test]
    #[cfg(feature = "vfio")]
    fn error_devinit_vbios_resource_io_source() {
        let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied");
        let e = DevinitError::vbios_resource_io("read", "/sys/.../rom", io_err);
        let de: DriverError = e.into();
        assert!(de.source().is_some());
        assert!(de.to_string().contains("devinit"));
    }

    #[test]
    fn pci_discovery_config_too_short_display() {
        let e = PciDiscoveryError::ConfigTooShort { len: 32, need: 64 };
        let s = e.to_string();
        assert!(s.contains("32"));
        assert!(s.contains("64"));
    }

    #[test]
    fn error_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<DriverError>();
    }

    #[test]
    fn error_display_open_failed() {
        let e = DriverError::OpenFailed("permission denied".into());
        assert!(e.to_string().contains("permission denied"));
    }

    #[test]
    fn error_display_dispatch_failed() {
        let e = DriverError::DispatchFailed("illegal instruction".into());
        assert!(e.to_string().contains("illegal instruction"));
    }

    #[test]
    fn error_display_sync_failed() {
        let e = DriverError::SyncFailed("fence wait failed".into());
        assert!(e.to_string().contains("fence wait failed"));
    }

    #[test]
    fn error_display_oracle_error() {
        let e = DriverError::OracleError("bar0 walk failed".into());
        assert!(e.to_string().contains("bar0 walk failed"));
    }

    #[test]
    fn error_oracle_helper_builds_variant() {
        let e = DriverError::oracle("dynamic oracle message");
        assert!(matches!(e, DriverError::OracleError(_)));
        assert!(e.to_string().contains("dynamic oracle message"));
    }

    #[test]
    fn error_oracle_static_cow() {
        let e = DriverError::oracle(Cow::Borrowed("static oracle"));
        assert_eq!(e.to_string(), "oracle error: static oracle");
    }
}
