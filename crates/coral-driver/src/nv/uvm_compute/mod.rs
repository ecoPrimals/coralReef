// SPDX-License-Identifier: AGPL-3.0-or-later
//! UVM-based compute device — dispatches via the NVIDIA proprietary driver.
//!
//! Bypasses nouveau entirely, using the RM object hierarchy through
//! `/dev/nvidiactl` and UVM through `/dev/nvidia-uvm` for memory management.
//! Reuses the identical QMD and push buffer formats from the nouveau path.

mod channel_setup;
mod compute_trait;
mod ctx_buffers;
mod device;
mod types;

pub use device::NvUvmComputeDevice;

#[cfg(test)]
mod tests;
