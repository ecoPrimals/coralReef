// SPDX-License-Identifier: AGPL-3.0-or-later
//! NVIDIA Volta `GpuMetal` implementation.
//!
//! Provides register maps, power domains, engine topology, and warm-up
//! sequences specific to the GV100 (Titan V) and other Volta-class GPUs.
//! This is the reference implementation — AMD Vega will follow the same
//! trait structure with its own register offsets.

mod detect;
mod identity;
mod metal;
mod probe;
mod reg_offsets;

pub(crate) use reg_offsets::volta_regs;

pub use detect::detect_gpu_metal;
pub use identity::NvVoltaIdentity;
pub use metal::NvVoltaMetal;
pub use probe::NvVoltaProbe;

#[cfg(test)]
mod tests;
