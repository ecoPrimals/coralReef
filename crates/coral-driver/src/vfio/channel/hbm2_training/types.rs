// SPDX-License-Identifier: AGPL-3.0-only
#![expect(missing_docs, reason = "HBM2 typestate types; full docs planned")]
//! Typestate phase markers, error types, training log, and backend enum.

use std::fmt;

// ── Newtype register domain offsets ─────────────────────────────────────

/// An offset within the FBPA (Framebuffer Partition Array) register domain.
/// Prevents accidental use of FBPA offsets in PMC or LTC contexts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FbpaOffset(pub usize);

/// An offset within the LTC (L2 Cache) register domain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LtcOffset(pub usize);

/// An offset within the PFB (Framebuffer controller) register domain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PfbOffset(pub usize);

/// An offset within the PCLOCK register domain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PclockOffset(pub usize);

// ── Typestate phase markers ─────────────────────────────────────────────

/// GPU memory controller has not been initialized. FBPA clocks may be gated.
pub struct Untrained;
/// FBPA clock domains are enabled. PHY is powered but not calibrated.
pub struct PhyUp;
/// HBM2 PHY link training is complete. DRAM timing not yet configured.
pub struct LinkTrained;
/// DRAM timing registers are configured. Memory controller is ready.
pub struct DramReady;
/// VRAM accessibility verified via PRAMIN sentinel write/readback.
pub struct Verified;

/// Trait bound for all HBM2 training phases.
pub trait Hbm2Phase: sealed::Sealed + fmt::Debug {}
impl Hbm2Phase for Untrained {}
impl Hbm2Phase for PhyUp {}
impl Hbm2Phase for LinkTrained {}
impl Hbm2Phase for DramReady {}
impl Hbm2Phase for Verified {}

mod sealed {
    pub trait Sealed {}
    impl Sealed for super::Untrained {}
    impl Sealed for super::PhyUp {}
    impl Sealed for super::LinkTrained {}
    impl Sealed for super::DramReady {}
    impl Sealed for super::Verified {}
}

impl fmt::Debug for Untrained {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Untrained")
    }
}
impl fmt::Debug for PhyUp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PhyUp")
    }
}
impl fmt::Debug for LinkTrained {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "LinkTrained")
    }
}
impl fmt::Debug for DramReady {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "DramReady")
    }
}
impl fmt::Debug for Verified {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Verified")
    }
}

// ── Training phase error ────────────────────────────────────────────────

/// Which phase of HBM2 training failed and why.
#[derive(Debug, Clone)]
pub struct Hbm2TrainingError {
    pub phase: &'static str,
    pub detail: String,
    pub register_snapshot: Vec<(usize, u32)>,
}

impl fmt::Display for Hbm2TrainingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "HBM2 training failed at {}: {}", self.phase, self.detail)
    }
}

impl std::error::Error for Hbm2TrainingError {}

// ── Training log ────────────────────────────────────────────────────────

/// A single recorded action during HBM2 training.
#[derive(Debug, Clone)]
pub enum TrainingAction {
    RegWrite {
        offset: usize,
        value: u32,
        old: u32,
    },
    RegRead {
        offset: usize,
        value: u32,
    },
    Delay {
        ms: u64,
    },
    PhaseTransition {
        from: String,
        to: String,
    },
    Verification {
        offset: usize,
        expected: u32,
        actual: u32,
        ok: bool,
    },
}

/// Accumulated log from a training attempt.
#[derive(Debug, Clone, Default)]
pub struct TrainingLog {
    pub actions: Vec<TrainingAction>,
}

impl TrainingLog {
    pub(crate) fn log_write(&mut self, offset: usize, value: u32, old: u32) {
        self.actions
            .push(TrainingAction::RegWrite { offset, value, old });
    }

    pub(crate) fn log_read(&mut self, offset: usize, value: u32) {
        self.actions.push(TrainingAction::RegRead { offset, value });
    }

    pub(crate) fn log_delay(&mut self, ms: u64) {
        self.actions.push(TrainingAction::Delay { ms });
    }

    pub(crate) fn log_phase(&mut self, from: &str, to: &str) {
        self.actions.push(TrainingAction::PhaseTransition {
            from: from.into(),
            to: to.into(),
        });
    }

    pub(crate) fn log_verify(&mut self, offset: usize, expected: u32, actual: u32) {
        self.actions.push(TrainingAction::Verification {
            offset,
            expected,
            actual,
            ok: actual == expected,
        });
    }

    /// Count of register writes performed.
    pub fn write_count(&self) -> usize {
        self.actions
            .iter()
            .filter(|a| matches!(a, TrainingAction::RegWrite { .. }))
            .count()
    }
}
