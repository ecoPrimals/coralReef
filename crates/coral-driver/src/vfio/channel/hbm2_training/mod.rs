// SPDX-License-Identifier: AGPL-3.0-only
#![allow(missing_docs)]
//! Sovereign HBM2 memory training via typestate-enforced phase transitions.
//!
//! This module provides compile-time guarantees that HBM2 training phases
//! execute in the correct order. The Rust type system prevents:
//! - Verifying VRAM before PHY calibration (compile error)
//! - Skipping link training (compile error)
//! - Holding references to "untrained" state during "trained" operations
//!
//! # Training Phases
//!
//! ```text
//! Untrained → PhyUp → LinkTrained → DramReady → Verified
//!    │          │          │             │           │
//!    │   enable_phy()  train_links() init_dram() verify_vram()
//!    │          │          │             │           │
//!    ▼          ▼          ▼             ▼           ▼
//! (cold)   (clocks on)  (PHY cal)   (timings)   (VRAM alive)
//! ```
//!
//! # Training Backends
//!
//! Three backends can drive the phase transitions:
//! - **VbiosInterpreter**: Execute VBIOS init scripts from the host CPU
//! - **DifferentialReplay**: Replay captured register diffs from an oracle card
//! - **FalconUpload**: Upload and execute DEVINIT firmware on the PMU FALCON
//!
//! # Rust Type System Advantages
//!
//! - Ownership transfer: each phase transition consumes the previous state
//! - Newtype register domains: `FbpaOffset` vs `LtcOffset` prevent cross-domain mix-ups
//! - Zero-cost abstractions: all typestate checks vanish at compile time
//! - The compiler can prove no aliased writes to active memory controllers

mod backend;
mod constants;
mod controller;
mod minimal;
mod oracle;
mod snapshot;
mod train;
mod types;

#[cfg(test)]
mod tests;

pub use backend::TrainingBackend;
pub use constants::volta_hbm2;
pub use controller::Hbm2Controller;
pub use minimal::binary_search_minimal_writes;
pub use oracle::{
    DomainCapture, GoldenCapture, HBM2_CAPTURE_DOMAINS, ReplayResult, capture_oracle_state,
    diff_golden_vs_cold, differential_training, replay_golden_diff,
};
pub use snapshot::{FbpaSnapshot, snapshot_fbpa};
pub use train::train_hbm2;
pub use types::{DramReady, LinkTrained, PhyUp, TrainingAction, TrainingLog, Untrained, Verified};
pub use types::{FbpaOffset, Hbm2Phase, Hbm2TrainingError, LtcOffset, PclockOffset, PfbOffset};
