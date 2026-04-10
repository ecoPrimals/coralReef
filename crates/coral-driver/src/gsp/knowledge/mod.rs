// SPDX-License-Identifier: AGPL-3.0-or-later
//! Sovereign GSP knowledge base — learned from all available hardware.
//!
//! Aggregates initialization knowledge from multiple GPU architectures
//! and vendors to build a cross-architecture understanding of GPU
//! compute initialization. This knowledge drives both:
//!
//! - **Init on old hardware**: Apply learned init sequences to GPUs
//!   without firmware (Volta, older Turing)
//! - **Optimization on modern hardware**: Dispatch hints, workgroup
//!   sizing, memory placement based on observed hardware behavior

mod chip;
mod gpu_knowledge;
mod types;

#[cfg(test)]
mod tests;

pub use gpu_knowledge::GpuKnowledge;
pub use types::{AddressSpace, ArchKnowledge, GenerationStats, RegisterTransferMap};
#[allow(unused_imports)]
pub use types::{GpuVendor, KnowledgeSummary};
