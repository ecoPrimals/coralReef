// SPDX-License-Identifier: AGPL-3.0-only
//! Vendor-specific GPU lifecycle hooks for safe driver transitions.
//!
//! Different GPU vendors (and even chip families within a vendor) have
//! wildly different behaviors when VFIO-PCI unbinds, bus resets fire,
//! or native drivers rebind. This module encodes those differences as
//! a trait so the core swap logic in [`super::swap`] stays generic.
//!
//! The key insight from empirical testing:
//!
//! - **NVIDIA GV100 (Volta)**: Bus reset **destroys HBM2 training state**
//!   (VRAM reads return `0xbad0acXX`, memory fabric dead, PBDMA cannot DMA).
//!   The `reset_method` must be cleared before vfio-pci unbind/bind to
//!   preserve HBM2 across driver transitions.
//!
//! - **AMD Vega 20 (GFX906)**: Bus reset triggers D3cold, killing SMU
//!   firmware state. The reset_method must be disabled before vfio-pci
//!   unbind. Native driver rebind needs PCI remove/rescan to avoid
//!   sysfs EEXIST from stale kobjects.
//!
//! - **Intel Xe/Arc**: FLR typically available, expected to be well-behaved.
//!   Stubbed with conservative defaults until empirically validated.

mod amd;
mod brainchip;
mod detect;
mod generic;
mod intel;
mod nvidia;
mod types;

#[cfg(test)]
mod tests;

pub use amd::{AmdRdnaLifecycle, AmdVega20Lifecycle};
pub use brainchip::BrainChipLifecycle;
pub use detect::{detect_lifecycle, detect_lifecycle_for_target};
pub use generic::GenericLifecycle;
pub use intel::IntelXeLifecycle;
pub use nvidia::{
    NvidiaKeplerLifecycle, NvidiaLifecycle, NvidiaOpenLifecycle, NvidiaOracleLifecycle,
};
pub use types::{RebindStrategy, ResetMethod, VendorLifecycle};

#[cfg(test)]
pub(crate) use detect::{is_amd_vega20, is_nvidia_kepler, lifecycle_from_pci_ids};
