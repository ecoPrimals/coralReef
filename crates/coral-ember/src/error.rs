// SPDX-License-Identifier: AGPL-3.0-only
//! Typed errors for sysfs, swap, trace, and IPC transport.
#![allow(missing_docs)] // Variants are self-describing via `#[error]` and thiserror `Display`.

/// Errors from sysfs driver operations.
#[derive(Debug, thiserror::Error)]
pub enum SysfsError {
    #[error("sysfs write to {path}: {reason}")]
    Write {
        /// Sysfs path that was written.
        path: String,
        /// Failure reason.
        reason: String,
    },
    #[error("sysfs read from {path}: {reason}")]
    Read {
        /// Sysfs path that was read.
        path: String,
        /// Failure reason.
        reason: String,
    },
    #[error("driver bind failed for {bdf}: {reason}")]
    DriverBind {
        /// PCI BDF.
        bdf: String,
        /// Failure reason.
        reason: String,
    },
    #[error("PCI reset failed for {bdf}: {reason}")]
    PciReset {
        /// PCI BDF.
        bdf: String,
        /// Failure reason.
        reason: String,
    },
    #[error("parent PCI bridge not found for device {bdf}")]
    BridgeNotFound {
        /// PCI BDF.
        bdf: String,
    },
    #[error("parent bridge {bridge_bdf} has no sysfs reset file (device {bdf})")]
    BridgeResetMissing {
        /// PCI BDF of the device.
        bdf: String,
        /// PCI BDF of the parent bridge.
        bridge_bdf: String,
    },
    #[error("PCI device {bdf} did not re-appear after bus rescan")]
    DeviceNotReappeared {
        /// PCI BDF.
        bdf: String,
    },
    #[error("{bdf}: PM power cycle resulted in D3cold")]
    PmCycleD3cold {
        /// PCI BDF.
        bdf: String,
    },
}

/// Errors from swap orchestration (preflight, sysfs, DRM isolation, trace).
#[derive(Debug, thiserror::Error)]
pub enum SwapError {
    #[error("preflight check failed for {bdf}: {reason}")]
    Preflight {
        /// PCI BDF.
        bdf: String,
        /// Failure reason.
        reason: String,
    },
    #[error("DRM isolation check failed: {0}")]
    DrmIsolation(String),
    #[error("external VFIO holders detected for {bdf}: {count} holders")]
    ExternalVfioHolders {
        /// PCI BDF.
        bdf: String,
        /// Number of external holders.
        count: usize,
    },
    #[error("sysfs operation failed: {0}")]
    Sysfs(#[from] SysfsError),
    #[error("unknown target driver: {0}")]
    UnknownTarget(String),
    #[error("trace operation failed: {0}")]
    Trace(String),
    #[error("post-bind verification failed for {bdf}: {detail}")]
    VerifyHealth {
        /// PCI BDF.
        bdf: String,
        /// Failure detail.
        detail: String,
    },
    #[error("swap blocked: active display GPU at {bdf} — unbinding would crash the system")]
    ActiveDisplayGpu {
        /// PCI BDF.
        bdf: String,
    },
    #[error("VFIO reacquire failed for {bdf}: {reason}")]
    VfioReacquire {
        /// PCI BDF.
        bdf: String,
        /// Failure reason.
        reason: String,
    },
    #[error("unknown or unsupported reset method: {0}")]
    InvalidResetMethod(String),
    #[error("{0}")]
    Other(String),
}

impl From<String> for SwapError {
    fn from(s: String) -> Self {
        Self::Other(s)
    }
}

/// Errors from mmiotrace enable/disable and trace capture.
#[derive(Debug, thiserror::Error)]
pub enum TraceError {
    #[error("mmiotrace enable failed: {0}")]
    Enable(String),
    #[error("mmiotrace disable failed: {0}")]
    Disable(String),
    #[error("trace capture failed for {bdf}: {reason}")]
    Capture {
        /// PCI BDF.
        bdf: String,
        /// Failure reason.
        reason: String,
    },
}

/// Transport / serialization failures for the ember JSON-RPC IPC layer (not JSON-RPC fault payloads).
#[derive(Debug, thiserror::Error)]
pub enum EmberIpcError {
    #[error("invalid request: {0}")]
    InvalidRequest(&'static str),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("UTF-8 decode error: {0}")]
    Utf8(#[from] std::str::Utf8Error),
    #[error("RwLock poisoned")]
    LockPoisoned,
    #[error("JSON serialization failed: {0}")]
    JsonSerialize(String),
    #[error("sendmsg failed: {0}")]
    SendMsg(String),
    /// String errors from synchronous JSON-RPC handlers (I/O mapping, lock poison as string, etc.).
    #[error("{0}")]
    Dispatch(String),
}

impl From<String> for EmberIpcError {
    fn from(s: String) -> Self {
        Self::Dispatch(s)
    }
}
