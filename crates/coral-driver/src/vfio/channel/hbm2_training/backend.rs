// SPDX-License-Identifier: AGPL-3.0-only
#![expect(missing_docs, reason = "training backend; full docs planned")]
//! Training backend selection.

/// Selects which backend drives the HBM2 training register writes.
#[derive(Debug, Clone)]
pub enum TrainingBackend {
    /// Execute VBIOS init scripts from host CPU via BAR0.
    VbiosInterpreter { rom: Vec<u8> },
    /// Replay a captured register diff from an oracle card.
    DifferentialReplay { golden_state: Vec<(usize, u32)> },
    /// Upload DEVINIT firmware to PMU FALCON and execute.
    FalconUpload { rom: Vec<u8> },
}
