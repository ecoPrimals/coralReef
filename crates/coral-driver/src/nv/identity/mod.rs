// SPDX-License-Identifier: AGPL-3.0-or-later
//! GPU identity probing via sysfs — no ioctl dependencies.
//!
//! Reads PCI vendor/device IDs from `/sys/class/drm/` to identify the
//! GPU model and map it to an SM architecture version.

mod chip_map;
mod constants;
mod firmware;
mod gpu_identity;
mod sysfs;

#[cfg(test)]
mod tests;

pub use chip_map::{boot0_to_sm, chip_name, chipset_variant, sm_to_compute_class};
pub use constants::{PCI_VENDOR_AMD, PCI_VENDOR_INTEL, PCI_VENDOR_NVIDIA};
pub use firmware::{FirmwareInventory, FwStatus, check_nouveau_firmware, firmware_inventory};
pub use gpu_identity::GpuIdentity;
pub use sysfs::probe_gpu_identity;
