// SPDX-License-Identifier: AGPL-3.0-only
//! # coral-nak-isa — NVIDIA GPU ISA Tables
//!
//! Instruction encoding tables and latency data for NVIDIA GPU architectures.
//!
//! ## Extracted from NAK
//!
//! The original NAK sources contain instruction encoding in:
//! - `sm70.rs` / `sm70_encode.rs` — Volta+ encoding (primary target)
//! - `sm50.rs` — Maxwell encoding
//! - `sm32.rs` — Kepler encoding
//! - `sm20.rs` — Fermi encoding (legacy)
//! - `sm*_instr_latencies.rs` — scheduling latency tables
//! - `sph.rs` — Shader Program Header
//! - `qmd.rs` — Queue Management Descriptor
//!
//! ## Public API (target)
//!
//! ```rust,ignore
//! use coral_nak_isa::{Sm70Encoder, InstrLatency};
//!
//! let latency = InstrLatency::for_arch(GpuArch::Sm70, &instr);
//! let binary = Sm70Encoder::encode(&shader)?;
//! ```

/// Instruction latency model for scheduling.
pub mod latency;

/// Shader Program Header (SPH) format.
pub mod sph;

pub use latency::InstrLatency;
pub use sph::SphBuilder;

/// Target architectures for instruction encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IsaTarget {
    /// Volta (SM70) — primary target for coralNak.
    Sm70,
    /// Turing (SM75).
    Sm75,
    /// Ampere (SM80).
    Sm80,
}
