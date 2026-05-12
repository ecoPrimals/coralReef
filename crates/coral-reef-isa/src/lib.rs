// SPDX-License-Identifier: AGPL-3.0-or-later
#![forbid(unsafe_code)]
#![warn(missing_docs)]
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
//! ```rust
//! use coral_reef_isa::{InstrLatency, IsaTarget};
//!
//! let _latency = InstrLatency::DEFAULT;
//! let _target = IsaTarget::Sm70;
//! ```

/// Instruction latency model for scheduling.
pub mod latency;

/// Shader Program Header (SPH) format.
pub mod sph;

pub use latency::InstrLatency;
pub use sph::SphBuilder;

/// Target architectures for instruction encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IsaTarget {
    /// Volta (SM70) — primary target for coralReef.
    Sm70,
    /// Turing (SM75).
    Sm75,
    /// Ampere (SM80).
    Sm80,
}

impl IsaTarget {
    /// All known ISA targets, ordered by SM version.
    pub const ALL: [Self; 3] = [Self::Sm70, Self::Sm75, Self::Sm80];

    /// SM version number (e.g. 70 for Volta).
    #[must_use]
    pub const fn sm_version(self) -> u8 {
        match self {
            Self::Sm70 => 70,
            Self::Sm75 => 75,
            Self::Sm80 => 80,
        }
    }

    /// Whether this target supports independent thread scheduling.
    #[must_use]
    pub const fn has_independent_thread_scheduling(self) -> bool {
        matches!(self, Self::Sm70 | Self::Sm75 | Self::Sm80)
    }

    /// Whether this target supports uniform datapath.
    #[must_use]
    pub const fn has_uniform_datapath(self) -> bool {
        matches!(self, Self::Sm75 | Self::Sm80)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn isa_target_sm_versions() {
        assert_eq!(IsaTarget::Sm70.sm_version(), 70);
        assert_eq!(IsaTarget::Sm75.sm_version(), 75);
        assert_eq!(IsaTarget::Sm80.sm_version(), 80);
    }

    #[test]
    fn isa_target_all_ordered() {
        let versions: Vec<u8> = IsaTarget::ALL.iter().map(|t| t.sm_version()).collect();
        for w in versions.windows(2) {
            assert!(w[0] < w[1], "ALL must be ordered by SM version");
        }
    }

    #[test]
    fn isa_target_clone_eq() {
        let a = IsaTarget::Sm70;
        #[allow(clippy::clone_on_copy)]
        let b = a.clone();
        assert_eq!(a, b);
    }

    #[test]
    fn isa_target_debug() {
        let dbg = format!("{:?}", IsaTarget::Sm75);
        assert!(dbg.contains("Sm75"));
    }

    #[test]
    fn isa_target_independent_thread_scheduling() {
        for target in &IsaTarget::ALL {
            assert!(target.has_independent_thread_scheduling());
        }
    }

    #[test]
    fn isa_target_uniform_datapath() {
        assert!(!IsaTarget::Sm70.has_uniform_datapath());
        assert!(IsaTarget::Sm75.has_uniform_datapath());
        assert!(IsaTarget::Sm80.has_uniform_datapath());
    }

    #[test]
    fn isa_target_hash() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(IsaTarget::Sm70);
        set.insert(IsaTarget::Sm70);
        assert_eq!(set.len(), 1);
        set.insert(IsaTarget::Sm75);
        assert_eq!(set.len(), 2);
    }
}
