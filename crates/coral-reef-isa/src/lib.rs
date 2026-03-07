// SPDX-License-Identifier: AGPL-3.0-only
#![deny(unsafe_code)]
//! # coral-reef-isa — GPU ISA Tables
//!
//! Instruction encoding tables and latency data for GPU architectures.
//!
//! ## Contents
//!
//! Encoding and scheduling data evolved from upstream sources:
//! - SM70+ encoding — Volta+ (primary target)
//! - SM50 encoding — Maxwell
//! - SM32 encoding — Kepler
//! - SM20 encoding — Fermi (legacy)
//! - `sm*_instr_latencies` — scheduling latency tables
//! - Shader Program Header
//! - Queue Management Descriptor
//!
//! ## Public API (target)
//!
//! ```rust,ignore
//! use coral_reef_isa::{Sm70Encoder, InstrLatency};
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
    /// Volta (SM70) — primary target for coralReef.
    Sm70,
    /// Turing (SM75).
    Sm75,
    /// Ampere (SM80).
    Sm80,
}
