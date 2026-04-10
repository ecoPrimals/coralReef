// SPDX-License-Identifier: AGPL-3.0-or-later
#![expect(missing_docs, reason = "memory abstraction types; full docs planned")]
//! Unified memory abstraction for GPU/CPU bidirectional topology.
//!
//! Models GPU and CPU memory as a graph of regions connected by access paths.
//! Every read/write is both an operation and an observation — writing a sentinel
//! from one side and reading from the other reveals the GPU's internal state.
//!
//! Three implementations unify the previously separate memory access patterns:
//! - [`DmaRegion`] — system memory (CPU alloc, IOMMU-mapped, GPU reads via MMU)
//! - [`PraminRegion`] — VRAM via the 64KB BAR0 PRAMIN window
//! - [`MmioRegion`] — BAR0 register space (volatile MMIO)

mod core;
mod regions;
mod topology;

#[cfg(test)]
mod tests;

pub use core::{Aperture, MemoryError, PathStatus};
pub use regions::{DmaRegion, MemoryRegion, MmioRegion, PraminRegion};
pub use topology::{AccessPath, MemoryDelta, MemoryTopology, PathMethod};
