// SPDX-License-Identifier: AGPL-3.0-only
//! Typed errors for sysfs, swap, and trace operations.
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
    #[error("{0}")]
    Other(String),
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
