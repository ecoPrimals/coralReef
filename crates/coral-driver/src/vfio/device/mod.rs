// SPDX-License-Identifier: AGPL-3.0-or-later
//! VFIO device management — open container/group/device, map BARs.
//!
//! `VfioDevice` wraps the full VFIO lifecycle for a single PCIe function:
//! container → group → device fd → BAR mmap. DMA buffers are allocated
//! separately via [`super::DmaBuffer`].

mod bus_master;
mod dma;
mod handles;
mod mapped_bar;
mod open;
mod runtime;

use std::os::fd::OwnedFd;

use dma::VfioBackend;

pub use dma::{DmaBackend, ReceivedVfioFds, VfioBackendKind};
pub use mapped_bar::MappedBar;

/// A VFIO-managed PCIe device.
///
/// Holds the device fd lifecycle and the backend-specific state for DMA.
/// Drop order: `device` closes before `backend` fields, matching VFIO's
/// required teardown sequence.
pub struct VfioDevice {
    bdf: String,
    pub(super) device: OwnedFd,
    num_regions: u32,
    backend: VfioBackend,
}
