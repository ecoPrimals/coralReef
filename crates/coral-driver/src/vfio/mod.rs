// SPDX-License-Identifier: AGPL-3.0-only
//! Minimal VFIO layer for sovereign GPU dispatch.
//!
//! Provides direct PCIe device access via Linux VFIO: open container/group/device,
//! map BAR regions, and allocate IOMMU-mapped DMA buffers. This is coralReef's
//! own dispatch-focused VFIO implementation — toadStool handles the hardware
//! lifecycle (binding GPUs to `vfio-pci`, IOMMU setup, permissions).
//!
//! # Prerequisites (provided by toadStool)
//!
//! - GPU bound to `vfio-pci` (not nouveau/nvidia)
//! - IOMMU enabled in BIOS and kernel (`intel_iommu=on` or `amd_iommu=on`)
//! - VFIO group permissions for the user (udev rules or group membership)
//!
//! # Architecture
//!
//! ```text
//! VfioDevice
//!   ├─ /dev/vfio/vfio          (container — IOMMU domain)
//!   ├─ /dev/vfio/{group}       (IOMMU group)
//!   ├─ device fd               (PCIe function)
//!   ├─ BAR mappings            (mmap'd register/memory regions)
//!   └─ DMA buffers             (IOMMU-mapped host memory)
//! ```

pub mod device;
pub mod dma;
pub mod ioctl;
pub mod types;

pub use device::VfioDevice;
pub use dma::DmaBuffer;
