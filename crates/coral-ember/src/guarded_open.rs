// SPDX-License-Identifier: AGPL-3.0-only
//! Thread-guarded VFIO device open — D-state isolation for cold hardware.
//!
//! # Problem
//!
//! Opening a VFIO device triggers kernel-side PCI config space access
//! (`VFIO_DEVICE_GET_INFO`, `enable_bus_master`, IOMMU attach). When the
//! PCIe endpoint is cold (not POSTed), these accesses cause PCIe completion
//! timeouts that put the calling thread into **uninterruptible D-state**
//! (`TASK_UNINTERRUPTIBLE`). The thread cannot be killed — even `SIGKILL`
//! is deferred until the kernel I/O completes, which never happens.
//!
//! This affects Tesla K80 (GK210) GPUs that boot cold on `vfio-pci`
//! without a prior VBIOS POST, and any PCIe device behind a powered-down
//! bridge.
//!
//! # Solution
//!
//! Perform `VfioDevice::open` in a **dedicated thread** with a timeout.
//! If the thread completes normally, the `VfioDevice` is moved to the
//! caller via channel. If the thread hangs (D-state), the caller returns
//! an error and the daemon continues. The D-state thread is leaked — it
//! cannot be killed, but it does not block the main event loop.
//!
//! Thread isolation (vs child process) is chosen here because:
//! - `VfioDevice` can be moved across threads via `std::sync::mpsc`
//! - No need for fd passing via `SCM_RIGHTS`
//! - Thread D-state does not prevent `std::process::exit()` from
//!   terminating the process (the kernel reaps all threads)
//!
//! # Usage
//!
//! ```ignore
//! match guarded_vfio_open("0000:4c:00.0", Duration::from_secs(15)) {
//!     Ok(device) => { /* VFIO device is live */ }
//!     Err(GuardedOpenError::Timeout { .. }) => { /* device is cold, defer */ }
//!     Err(GuardedOpenError::OpenFailed { .. }) => { /* VFIO error (not cold) */ }
//! }
//! ```

use std::time::Duration;

use coral_driver::vfio::VfioDevice;

/// Default timeout for guarded VFIO opens. Long enough for a slow-but-alive
/// device (iommufd setup + bus master enable), short enough to not stall
/// daemon startup when hardware is truly cold.
pub const GUARDED_OPEN_TIMEOUT: Duration = Duration::from_secs(15);

/// Errors from [`guarded_vfio_open`].
#[derive(Debug)]
pub enum GuardedOpenError {
    /// The open thread did not complete within the timeout — the device is
    /// likely cold/unPOSTed and the thread is stuck in kernel D-state.
    /// The thread is leaked; it will be reaped when the process exits.
    Timeout {
        /// PCI bus/device/function.
        bdf: String,
        /// How long we waited.
        timeout: Duration,
    },
    /// `VfioDevice::open` returned an error (device is not cold, just
    /// misconfigured or already held by another process).
    OpenFailed {
        /// PCI bus/device/function.
        bdf: String,
        /// The underlying driver error.
        error: String,
    },
    /// The open thread panicked.
    ThreadPanic {
        /// PCI bus/device/function.
        bdf: String,
    },
}

impl std::fmt::Display for GuardedOpenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Timeout { bdf, timeout } => write!(
                f,
                "{bdf}: VFIO open timed out after {}s (device likely cold/unPOSTed — \
                 thread leaked in D-state)",
                timeout.as_secs()
            ),
            Self::OpenFailed { bdf, error } => {
                write!(f, "{bdf}: VFIO open failed: {error}")
            }
            Self::ThreadPanic { bdf } => {
                write!(f, "{bdf}: VFIO open thread panicked")
            }
        }
    }
}

/// Open a VFIO device in a timeout-guarded thread.
///
/// If the device is alive, this completes in milliseconds and returns
/// the `VfioDevice`. If the device is cold (PCIe endpoint unresponsive),
/// the thread enters D-state and this function returns `Timeout` after
/// the deadline, leaking the D-state thread.
pub fn guarded_vfio_open(bdf: &str, timeout: Duration) -> Result<VfioDevice, GuardedOpenError> {
    let bdf_owned = bdf.to_string();
    let (tx, rx) = std::sync::mpsc::channel();

    let thread_bdf = bdf_owned.clone();
    let builder = std::thread::Builder::new().name(format!("vfio-open-{bdf_owned}"));

    let handle = builder.spawn(move || {
        let result = VfioDevice::open(&thread_bdf);
        // If the receiver is gone (timeout), the VfioDevice drops here,
        // closing fds and freeing IOMMU resources — which is correct.
        let _ = tx.send(result);
    });

    let handle = match handle {
        Ok(h) => h,
        Err(e) => {
            tracing::error!(bdf = %bdf_owned, error = %e, "failed to spawn VFIO open thread");
            return Err(GuardedOpenError::OpenFailed {
                bdf: bdf_owned,
                error: format!("thread spawn: {e}"),
            });
        }
    };

    match rx.recv_timeout(timeout) {
        Ok(Ok(device)) => Ok(device),
        Ok(Err(driver_err)) => Err(GuardedOpenError::OpenFailed {
            bdf: bdf_owned,
            error: driver_err.to_string(),
        }),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            tracing::error!(
                bdf = %bdf_owned,
                timeout_secs = timeout.as_secs(),
                thread = ?handle.thread().id(),
                "VFIO open TIMED OUT — device is cold/unPOSTed. \
                 The open thread is stuck in kernel D-state and will be leaked. \
                 Device will be deferred until POSTed."
            );
            // Do NOT join the handle — it would block forever. Leak the thread.
            // When the process exits, the kernel reaps all threads including D-state ones.
            std::mem::forget(handle);
            Err(GuardedOpenError::Timeout {
                bdf: bdf_owned,
                timeout,
            })
        }
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            tracing::error!(bdf = %bdf_owned, "VFIO open thread panicked or dropped sender");
            Err(GuardedOpenError::ThreadPanic { bdf: bdf_owned })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timeout_error_display() {
        let err = GuardedOpenError::Timeout {
            bdf: "0000:4c:00.0".to_string(),
            timeout: Duration::from_secs(15),
        };
        let msg = err.to_string();
        assert!(msg.contains("timed out"), "{msg}");
        assert!(msg.contains("D-state"), "{msg}");
    }

    #[test]
    fn open_failed_error_display() {
        let err = GuardedOpenError::OpenFailed {
            bdf: "0000:4c:00.0".to_string(),
            error: "device busy".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("device busy"), "{msg}");
    }
}
