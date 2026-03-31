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
#[path = "hw_nv_vfio/falcon_exp095_phase3.rs"]
mod falcon_exp095_phase3;

#[cfg(feature = "vfio")]
#[path = "hw_nv_vfio/falcon/mod.rs"]
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

#[cfg(feature = "vfio")]
#[path = "hw_nv_vfio/sec2_conversation.rs"]
mod sec2_conversation;

#[cfg(feature = "vfio")]
#[path = "hw_nv_vfio/sec2_emem_discovery.rs"]
mod sec2_emem_discovery;

#[cfg(feature = "vfio")]
#[path = "hw_nv_vfio/post_nouveau_falcon_state.rs"]
mod post_nouveau_falcon_state;

#[cfg(feature = "vfio")]
#[path = "hw_nv_vfio/exp100_dma_fix.rs"]
mod exp100_dma_fix;

#[cfg(feature = "vfio")]
#[path = "hw_nv_vfio/exp103_no_flr.rs"]
mod exp103_no_flr;

#[cfg(feature = "vfio")]
#[path = "hw_nv_vfio/exp110_matrix.rs"]
mod exp110_matrix;

#[cfg(feature = "vfio")]
#[path = "hw_nv_vfio/exp111_vram_native.rs"]
mod exp111_vram_native;

#[cfg(feature = "vfio")]
#[path = "hw_nv_vfio/exp113_trap_analysis.rs"]
mod exp113_trap_analysis;

#[cfg(feature = "vfio")]
#[path = "hw_nv_vfio/exp114_ls_mailbox.rs"]
mod exp114_ls_mailbox;

#[cfg(feature = "vfio")]
#[path = "hw_nv_vfio/exp115_direct_boot.rs"]
mod exp115_direct_boot;

#[cfg(feature = "vfio")]
#[path = "hw_nv_vfio/exp116_wpr_reuse.rs"]
mod exp116_wpr_reuse;

#[cfg(feature = "vfio")]
#[path = "hw_nv_vfio/exp117_wpr2_state.rs"]
mod exp117_wpr2_state;

#[cfg(feature = "vfio")]
#[path = "hw_nv_vfio/exp118_wpr2_preserve.rs"]
mod exp118_wpr2_preserve;

#[cfg(feature = "vfio")]
#[path = "hw_nv_vfio/exp119_cold_boot_wpr2.rs"]
mod exp119_cold_boot_wpr2;

#[cfg(feature = "vfio")]
#[path = "hw_nv_vfio/exp120_sovereign_devinit.rs"]
mod exp120_sovereign_devinit;

#[cfg(feature = "vfio")]
#[path = "hw_nv_vfio/exp121_minimal_acr.rs"]
mod exp121_minimal_acr;

#[cfg(feature = "vfio")]
#[path = "hw_nv_vfio/exp122_wpr2_resolution.rs"]
mod exp122_wpr2_resolution;

#[cfg(feature = "vfio")]
#[path = "hw_nv_vfio/exp126_warm_dispatch_diagnostic.rs"]
mod exp126_warm_dispatch_diagnostic;
