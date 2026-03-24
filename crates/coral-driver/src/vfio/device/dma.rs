// SPDX-License-Identifier: AGPL-3.0-only
//! VFIO DMA backend types (legacy container vs iommufd IOAS).

use std::os::fd::OwnedFd;
use std::sync::Arc;

/// Which VFIO backend is in use — allows callers (ember, glowplug) to
/// branch on the backend without accessing raw fds or panicking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VfioBackendKind {
    /// Legacy VFIO container/group (kernel < 6.2).
    /// SCM_RIGHTS sends 3 fds: container, group, device.
    Legacy,
    /// Modern iommufd/cdev (kernel 6.2+).
    /// SCM_RIGHTS sends 2 fds: iommufd, device.
    /// `ioas_id` must be transmitted out-of-band (JSON metadata).
    Iommufd {
        /// IOAS id from `IOMMU_IOAS_ALLOC`, needed by the receiver for DMA.
        ioas_id: u32,
    },
}

/// DMA mapping backend — abstracts the difference between the legacy VFIO
/// container and the modern iommufd IOAS. Both variants are cheap to clone
/// (`Arc`-wrapped) so they can be passed to [`super::DmaBuffer`] and channel
/// code that needs to create IOMMU mappings.
#[derive(Clone)]
pub enum DmaBackend {
    /// Legacy VFIO container fd (kernel < 6.2). DMA maps via
    /// `VFIO_IOMMU_MAP_DMA` / `VFIO_IOMMU_UNMAP_DMA` on this fd.
    LegacyContainer(Arc<OwnedFd>),
    /// Modern iommufd IOAS (kernel 6.2+). DMA maps via `IOMMU_IOAS_MAP` /
    /// `IOMMU_IOAS_UNMAP` on the iommufd, referencing the IOAS by id.
    Iommufd {
        /// Open `/dev/iommu` file descriptor.
        fd: Arc<OwnedFd>,
        /// IOAS id from `IOMMU_IOAS_ALLOC`.
        ioas_id: u32,
    },
}

/// VFIO file descriptors received via `SCM_RIGHTS` — backend-aware.
///
/// Ember sends these to glowplug; glowplug passes them to
/// [`VfioDevice::from_received`] to reconstruct a device handle.
pub enum ReceivedVfioFds {
    /// Legacy path: container, group, device (3 fds).
    Legacy {
        /// VFIO container fd.
        container: OwnedFd,
        /// VFIO IOMMU group fd.
        group: OwnedFd,
        /// VFIO device fd.
        device: OwnedFd,
    },
    /// Modern iommufd path: iommufd, device (2 fds + ioas_id metadata).
    Iommufd {
        /// `/dev/iommu` fd with IOAS already allocated and device attached.
        iommufd: OwnedFd,
        /// VFIO cdev device fd.
        device: OwnedFd,
        /// IOAS id (from JSON metadata, not an fd).
        ioas_id: u32,
    },
}

/// Internal backend state for the VFIO open path. Legacy carries the group
/// and container fds; iommufd carries the iommufd and IOAS id.
pub(crate) enum VfioBackend {
    LegacyGroup {
        container: Arc<OwnedFd>,
        group: std::fs::File,
    },
    Iommufd {
        iommufd: Arc<OwnedFd>,
        ioas_id: u32,
    },
}
