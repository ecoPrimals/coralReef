// SPDX-License-Identifier: AGPL-3.0-only
//! PCIe Armor — systematic, hardware-agnostic PCIe safety layer.
//!
//! Consolidates all PCIe hardening learned from GV100 root-complex freeze
//! debugging into a single, per-device struct that is armed at acquisition
//! and disarmed at release. Saves original register values so the bridge
//! can be restored to its pre-ember state when the device is released.
//!
//! # Protections applied
//!
//! | Layer | What it prevents |
//! |-------|-----------------|
//! | Completion timeout | CPU stalls on unresponsive BAR0 reads (returns 0xFFFF_FFFF fast) |
//! | DPC disable | PCIe link teardown on errors (link stays up, software handles it) |
//! | AER disable | Kernel AER handler chasing errors through stuck downstream link |
//! | NMI watchdog | Premature kernel panic during bus-reset recovery |
//! | Reset method clear | Prevents VFIO from triggering resets on fd close |
//!
//! # Write-ordering constraint (MmioSequencer)
//!
//! Root cause of all GV100 lockups: **PRAMIN writes after PMC engine reset
//! freeze the entire PCIe root complex**. The [`MmioSequencer`] enforces
//! the safe ordering at runtime: bulk VRAM writes must complete before any
//! engine reset. After an engine reset, only register I/O is permitted
//! until the device transitions back to `Pristine` state.

use crate::sysfs;

/// Per-device PCIe protection state. Created at device acquisition,
/// dropped (with optional restore) at device release.
#[derive(Debug)]
pub struct PcieArmor {
    bdf: String,
    bridge_bdf: Option<String>,
    aer_saved: Option<u32>,
    sequencer: MmioSequencer,
}

impl PcieArmor {
    /// Apply all PCIe protections for a device. Call at acquisition time.
    ///
    /// This is idempotent — calling it twice is safe (the second call
    /// detects already-applied settings and skips them).
    pub fn arm(bdf: &str) -> Self {
        let bridge_bdf = sysfs::find_parent_bridge(bdf);

        // Aggressive completion timeouts: 50µs–100µs on device, 1ms–10ms on bridge
        sysfs::harden_pcie_timeouts(bdf);

        // Save AER state before we disable it (for later restore)
        let aer_saved = bridge_bdf.as_ref().and_then(|b| {
            sysfs::mask_bridge_aer(bdf).map(|(_, original)| original)
        });

        tracing::info!(
            bdf,
            bridge = bridge_bdf.as_deref().unwrap_or("none"),
            aer_saved = aer_saved.map(|v| format!("{v:#010x}")),
            "PCIe armor ARMED — AER/DPC/timeouts/NMI hardened"
        );

        Self {
            bdf: bdf.to_string(),
            bridge_bdf,
            aer_saved,
            sequencer: MmioSequencer::new(),
        }
    }

    /// Restore original PCIe settings on the bridge. Call at device release.
    ///
    /// Only restores AER — DPC and timeouts are left hardened since other
    /// devices may share the bridge, and aggressive timeouts are always safer.
    pub fn disarm(&self) {
        if let (Some(bridge), Some(original)) = (&self.bridge_bdf, self.aer_saved) {
            sysfs::unmask_bridge_aer(bridge, original);
            tracing::info!(
                bdf = %self.bdf,
                bridge = %bridge,
                "PCIe armor DISARMED — AER restored"
            );
        }
    }

    /// Access the write-ordering sequencer for this device.
    pub fn sequencer(&self) -> &MmioSequencer {
        &self.sequencer
    }

    /// Mutable access to the sequencer (for state transitions).
    pub fn sequencer_mut(&mut self) -> &mut MmioSequencer {
        &mut self.sequencer
    }
}

// ─── MMIO Write-Ordering Sequencer ─────────────────────────────────────────

/// Tracks whether an engine reset has occurred, enforcing the rule that
/// bulk VRAM (PRAMIN) writes must happen BEFORE any engine reset.
///
/// # The GV100 Root-Complex Freeze
///
/// On NVIDIA GV100, writing to the PRAMIN window (BAR0 0x700000) after a
/// PMC engine reset (toggling bits in PMC_ENABLE) causes the GPU's internal
/// memory arbiter to deadlock. This freezes the entire PCIe root complex —
/// all CPU cores, not just the one issuing the write. No software recovery
/// is possible; only a hard reboot clears it.
///
/// The safe ordering is: PRAMIN writes first (GPU pristine) → engine reset →
/// register I/O only. The sequencer enforces this at runtime.
#[derive(Debug, Clone)]
pub struct MmioSequencer {
    state: SequencerState,
}

/// The three states of the MMIO sequencer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SequencerState {
    /// GPU is in nouveau-warmed state. PRAMIN writes and register I/O are
    /// both safe. This is the initial state after warm-up or recovery.
    Pristine,

    /// An engine reset (PMC toggle) has occurred. Only register I/O is
    /// safe. PRAMIN writes are BLOCKED — attempting them returns an error
    /// explaining the required ordering.
    EngineResetDone,

    /// PRAMIN writes have been completed. Engine resets and register I/O
    /// are both safe. Transitions to `EngineResetDone` after a PMC reset.
    VramWritten,
}

/// Error returned when an operation violates the write-ordering constraint.
#[derive(Debug)]
pub struct SequencerViolation {
    pub attempted: &'static str,
    pub current_state: SequencerState,
    pub explanation: String,
}

impl std::fmt::Display for SequencerViolation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "MMIO ordering violation: cannot {} in state {:?} — {}",
            self.attempted, self.current_state, self.explanation
        )
    }
}

impl MmioSequencer {
    pub fn new() -> Self {
        Self {
            state: SequencerState::Pristine,
        }
    }

    /// Current sequencer state.
    pub fn state(&self) -> SequencerState {
        self.state
    }

    /// Check if a bulk VRAM (PRAMIN) write is allowed. Returns `Ok(())` if
    /// safe, or a `SequencerViolation` explaining why it's blocked.
    pub fn check_vram_write(&self) -> Result<(), SequencerViolation> {
        match self.state {
            SequencerState::Pristine | SequencerState::VramWritten => Ok(()),
            SequencerState::EngineResetDone => Err(SequencerViolation {
                attempted: "bulk VRAM write",
                current_state: self.state,
                explanation:
                    "PRAMIN writes after engine reset freeze the PCIe root complex. \
                     Write VRAM data BEFORE the engine reset, or recover the device \
                     to Pristine state first (ember.device.recover)."
                    .to_string(),
            }),
        }
    }

    /// Check if an engine reset (PMC toggle) is allowed. Always allowed,
    /// but transitions state.
    pub fn check_engine_reset(&self) -> Result<(), SequencerViolation> {
        Ok(())
    }

    /// Record that bulk VRAM writes have been completed.
    pub fn mark_vram_written(&mut self) {
        self.state = SequencerState::VramWritten;
        tracing::debug!("sequencer: VRAM written → state=VramWritten");
    }

    /// Record that an engine reset has occurred.
    pub fn mark_engine_reset(&mut self) {
        self.state = SequencerState::EngineResetDone;
        tracing::debug!("sequencer: engine reset → state=EngineResetDone");
    }

    /// Reset to pristine state (after warm cycle or device recovery).
    pub fn reset_to_pristine(&mut self) {
        self.state = SequencerState::Pristine;
        tracing::debug!("sequencer: reset → state=Pristine");
    }
}

impl Default for MmioSequencer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pristine_allows_vram_write() {
        let seq = MmioSequencer::new();
        assert!(seq.check_vram_write().is_ok());
    }

    #[test]
    fn engine_reset_blocks_vram_write() {
        let mut seq = MmioSequencer::new();
        seq.mark_engine_reset();
        assert!(seq.check_vram_write().is_err());
    }

    #[test]
    fn vram_then_reset_is_ok() {
        let mut seq = MmioSequencer::new();
        seq.mark_vram_written();
        assert!(seq.check_engine_reset().is_ok());
        seq.mark_engine_reset();
        assert!(seq.check_vram_write().is_err());
    }

    #[test]
    fn recovery_resets_to_pristine() {
        let mut seq = MmioSequencer::new();
        seq.mark_engine_reset();
        assert!(seq.check_vram_write().is_err());
        seq.reset_to_pristine();
        assert!(seq.check_vram_write().is_ok());
    }
}
