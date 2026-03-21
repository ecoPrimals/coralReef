// SPDX-License-Identifier: AGPL-3.0-only
// Copyright © 2026 ecoPrimals
#![forbid(unsafe_code)]
#![warn(missing_docs)]
//! # coral-gpu — Unified GPU Compute
//!
//! Sovereign GPU compute abstraction: compile WGSL → native binary →
//! dispatch on hardware, all in pure Rust.
//!
//! Replaces `wgpu` for compute workloads in the ecosystem and the wider
//! ecoPrimals ecosystem.
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────┐
//! │              coral-gpu                       │
//! │  ┌──────────┐  ┌──────────┐  ┌───────────┐ │
//! │  │ Compiler │  │  Driver  │  │  Context   │ │
//! │  │(coral-   │  │(coral-   │  │(compile +  │ │
//! │  │  reef)   │  │  driver) │  │  dispatch) │ │
//! │  └──────────┘  └──────────┘  └───────────┘ │
//! └─────────────────────────────────────────────┘
//! ```
//!
//! ## Example
//!
//! ```no_run
//! # fn main() -> Result<(), coral_gpu::GpuError> {
//! use coral_gpu::{GpuContext, GpuTarget};
//!
//! let mut ctx = GpuContext::auto()?;
//! let shader = ctx.compile_wgsl("@compute @workgroup_size(64) fn main() {}")?;
//! let mut buf = ctx.alloc(1024)?;
//! ctx.dispatch(&shader, &[buf], [16, 1, 1])?;
//! ctx.sync()?;
//! let _data = ctx.readback(buf, 1024)?;
//! # Ok(())
//! # }
//! ```

mod context;
mod driver;
mod error;
mod fma;
mod hash;
mod kernel;
mod pcie;
mod preference;

pub use context::GpuContext;
pub use error::{GpuError, GpuResult};
pub use fma::FmaCapability;
pub use kernel::{CompiledKernel, KernelCacheEntry};
pub use pcie::{PcieDeviceInfo, probe_pcie_topology};
pub use preference::DriverPreference;

pub use coral_driver::{BufferHandle, ComputeDevice, DispatchDims, MemoryDomain, ShaderInfo};
pub use coral_reef::{AmdArch, CompileOptions, FmaPolicy, GpuTarget, NvArch};

pub use driver::default_nv_sm;
pub use driver::default_nv_sm_nouveau;
pub use driver::{DEFAULT_NV_SM, DEFAULT_NV_SM_NOUVEAU};

#[cfg(test)]
mod tests;
