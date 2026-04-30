// SPDX-License-Identifier: AGPL-3.0-or-later
//! NVIDIA UVM (Unified Virtual Memory) interface for proprietary driver compute.
//!
//! The nvidia-drm render node (`/dev/dri/renderD*`) provides device identification
//! but not buffer management or compute dispatch. For GPU compute on the proprietary
//! driver, NVIDIA's UVM subsystem is required:
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────┐    ┌──────────────┐    ┌──────────────────┐
//! │ /dev/nvidia0 │    │ /dev/nvidiactl│    │ /dev/nvidia-uvm  │
//! │  (device)    │    │  (control)    │    │ (virtual memory)  │
//! └──────┬──────┘    └──────┬───────┘    └──────────┬───────┘
//!        │                  │                       │
//!        ▼                  ▼                       ▼
//!   RM client         RM control            UVM allocation
//!   Channel alloc     Object mgmt           GPU VA mapping
//!   Work submit       Driver caps           Page migration
//! ```
//!
//! ## Ioctl sources
//!
//! Definitions derived from NVIDIA open-gpu-kernel-modules (MIT license):
//! - `kernel-open/nvidia-uvm/uvm_linux_ioctl.h`
//! - `kernel-open/common/inc/nv-ioctl-numbers.h`
//! - `kernel-open/common/inc/nv-ioctl.h`

pub mod constants;
mod devices;
pub(crate) mod rm_client;
mod rm_helpers;
pub mod structs;

pub use constants::*;
pub use devices::{ExternalMapping, NvCtlDevice, NvGpuDevice, NvUvmDevice, nvidia_uvm_available};
pub use rm_client::RmClient;
pub use structs::*;

#[cfg(test)]
mod uvm_tests;
