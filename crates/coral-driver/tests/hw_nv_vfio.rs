// SPDX-License-Identifier: AGPL-3.0-only
//! NVIDIA VFIO hardware validation — core device opening, BAR0, basic ops.
//!
//! These tests exercise the VFIO compute pipeline:
//! open → alloc → upload → dispatch → sync → readback.
//!
//! # Prerequisites
//!
//! - GPU bound to `vfio-pci` (not nouveau/nvidia)
//! - IOMMU enabled in BIOS and kernel
//! - User has `/dev/vfio/*` permissions
//! - Set `CORALREEF_VFIO_BDF` env var to the GPU's PCIe address
//!
//! # GlowPlug integration
//!
//! If `coral-glowplug` is running and holds the VFIO fd, the test harness
//! automatically borrows the device via `device.lend` and returns it via
//! `device.reclaim` on drop. No manual VFIO management needed.
//!
//! Run: `CORALREEF_VFIO_BDF=0000:01:00.0 cargo test --test hw_nv_vfio --features vfio -- --ignored --test-threads=1`
//!
//! `--test-threads=1` is required because all tests share ember's single IOMMU
//! IOAS. Parallel device creation maps the same fixed IOVAs and gets `EEXIST`
//! from `IOMMU_IOAS_MAP`.

#[cfg(feature = "vfio")]
#[path = "glowplug_client.rs"]
mod glowplug_client;

#[cfg(feature = "vfio")]
#[path = "ember_client.rs"]
mod ember_client;

#[cfg(feature = "vfio")]
#[path = "hw_nv_vfio/helpers.rs"]
mod helpers;

#[cfg(feature = "vfio")]
#[path = "hw_nv_vfio/basic_ops.rs"]
mod basic_ops;

#[cfg(feature = "vfio")]
#[path = "hw_nv_vfio/dispatch.rs"]
mod dispatch;

#[cfg(feature = "vfio")]
#[path = "hw_nv_vfio/diagnostics.rs"]
mod diagnostics;

#[cfg(feature = "vfio")]
#[path = "hw_nv_vfio/falcon.rs"]
mod falcon;

#[cfg(feature = "vfio")]
#[path = "hw_nv_vfio/firmware.rs"]
mod firmware;

#[cfg(feature = "vfio")]
#[path = "hw_nv_vfio/error_handling.rs"]
mod error_handling;

#[cfg(feature = "vfio")]
#[path = "hw_nv_vfio/sec2_cmdq.rs"]
mod sec2_cmdq;
