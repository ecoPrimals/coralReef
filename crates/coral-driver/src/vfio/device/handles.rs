// SPDX-License-Identifier: AGPL-3.0-or-later
//! FD handles, backend metadata, and introspection for VFIO devices.

use crate::error::{DriverError, DriverResult};
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, RawFd};
use std::sync::Arc;

use super::VfioDevice;
use super::dma::{DmaBackend, VfioBackend, VfioBackendKind};

impl VfioDevice {
    /// DMA mapping backend for this device. Pass this to [`crate::vfio::DmaBuffer`],
    /// [`crate::vfio::VfioChannel`], and other code that needs to create IOMMU mappings.
    #[must_use]
    pub fn dma_backend(&self) -> DmaBackend {
        match &self.backend {
            VfioBackend::LegacyGroup { container, .. } => {
                DmaBackend::LegacyContainer(Arc::clone(container))
            }
            VfioBackend::Iommufd { iommufd, ioas_id } => DmaBackend::Iommufd {
                fd: Arc::clone(iommufd),
                ioas_id: *ioas_id,
            },
        }
    }

    /// Which backend this device is using. Callers (ember, glowplug) use this
    /// to select the right IPC fd-passing strategy without panicking.
    #[must_use]
    pub fn backend_kind(&self) -> VfioBackendKind {
        match &self.backend {
            VfioBackend::LegacyGroup { .. } => VfioBackendKind::Legacy,
            VfioBackend::Iommufd { ioas_id, .. } => VfioBackendKind::Iommufd { ioas_id: *ioas_id },
        }
    }

    /// File descriptors to pass via `SCM_RIGHTS` for this device.
    ///
    /// - **Legacy**: `[container, group, device]` (3 fds)
    /// - **Iommufd**: `[iommufd, device]` (2 fds)
    ///
    /// The receiver must also know the [`backend_kind`](Self::backend_kind) to
    /// reconstruct the device on the other side.
    #[must_use]
    pub fn sendable_fds(&self) -> Vec<BorrowedFd<'_>> {
        match &self.backend {
            VfioBackend::LegacyGroup {
                container, group, ..
            } => vec![container.as_fd(), group.as_fd(), self.device.as_fd()],
            VfioBackend::Iommufd { iommufd, .. } => {
                vec![iommufd.as_fd(), self.device.as_fd()]
            }
        }
    }

    /// Raw fd of the VFIO container (legacy path only, for ember `SCM_RIGHTS`).
    ///
    /// Prefer [`sendable_fds`](Self::sendable_fds) for backend-agnostic fd passing.
    ///
    /// # Errors
    ///
    /// Returns [`DriverError::Unsupported`] on an iommufd-backed device (no legacy container fd).
    pub fn container_fd(&self) -> DriverResult<RawFd> {
        match &self.backend {
            VfioBackend::LegacyGroup { container, .. } => Ok(container.as_raw_fd()),
            VfioBackend::Iommufd { .. } => Err(DriverError::Unsupported(
                "container_fd() not available on iommufd backend".into(),
            )),
        }
    }

    /// Borrowed handle to the VFIO container fd (legacy path only).
    ///
    /// Prefer [`sendable_fds`](Self::sendable_fds) for backend-agnostic fd passing.
    ///
    /// # Errors
    ///
    /// Returns [`DriverError::Unsupported`] on an iommufd-backed device (no legacy container fd).
    pub fn container_as_fd(&self) -> DriverResult<BorrowedFd<'_>> {
        match &self.backend {
            VfioBackend::LegacyGroup { container, .. } => Ok(container.as_fd()),
            VfioBackend::Iommufd { .. } => Err(DriverError::Unsupported(
                "container_as_fd() not available on iommufd backend".into(),
            )),
        }
    }

    /// PCIe BDF address.
    #[must_use]
    pub fn bdf(&self) -> &str {
        &self.bdf
    }

    /// Number of BAR regions reported by the device.
    #[must_use]
    pub const fn num_regions(&self) -> u32 {
        self.num_regions
    }

    /// Leak the VFIO file descriptors so they are NOT closed on drop.
    ///
    /// This prevents the kernel from performing a PM reset on the GPU,
    /// preserving HBM2 training state across process exits. Call this
    /// when you want to keep the GPU warm between test runs.
    ///
    /// The fds become unreachable but the kernel keeps them alive until
    /// the process exits, at which point the kernel will reset the device.
    pub fn leak(self) {
        std::mem::forget(self);
    }

    /// Raw fd of the VFIO device (for SCM\_RIGHTS fd passing to/from coral-ember).
    #[must_use]
    pub fn device_fd(&self) -> RawFd {
        self.device.as_raw_fd()
    }

    /// Borrowed handle to the VFIO device fd (for `SCM_RIGHTS` / [`AsFd`] APIs).
    #[must_use]
    pub fn device_as_fd(&self) -> BorrowedFd<'_> {
        self.device.as_fd()
    }

    /// Raw fd of the VFIO group (legacy path only, for ember `SCM_RIGHTS`).
    ///
    /// # Errors
    ///
    /// Returns [`DriverError::Unsupported`] on an iommufd-backed device (no VFIO group fd).
    pub fn group_fd(&self) -> DriverResult<RawFd> {
        match &self.backend {
            VfioBackend::LegacyGroup { group, .. } => Ok(group.as_raw_fd()),
            VfioBackend::Iommufd { .. } => Err(DriverError::Unsupported(
                "group_fd() not available on iommufd backend".into(),
            )),
        }
    }

    /// Borrowed handle to the VFIO group fd (legacy path only).
    ///
    /// # Errors
    ///
    /// Returns [`DriverError::Unsupported`] on an iommufd-backed device (no VFIO group fd).
    pub fn group_as_fd(&self) -> DriverResult<BorrowedFd<'_>> {
        match &self.backend {
            VfioBackend::LegacyGroup { group, .. } => Ok(group.as_fd()),
            VfioBackend::Iommufd { .. } => Err(DriverError::Unsupported(
                "group_as_fd() not available on iommufd backend".into(),
            )),
        }
    }
}

impl std::fmt::Debug for VfioDevice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VfioDevice")
            .field("bdf", &self.bdf)
            .field("num_regions", &self.num_regions)
            .finish_non_exhaustive()
    }
}
