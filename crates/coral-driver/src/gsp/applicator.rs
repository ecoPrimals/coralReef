// SPDX-License-Identifier: AGPL-3.0-only
//! Sovereign GSP applicator — bridge between learned init and hardware.
//!
//! Converts the sovereign GSP's [`GrInitSequence`] into a format suitable
//! for application via BAR0 MMIO. This module provides the integration
//! point between:
//!
//! - `coral-driver::gsp::gr_init` (the learned init sequence)
//! - `nvpmu::bar0` (the hardware access layer)
//!
//! # Address space handling
//!
//! Volta's `sw_bundle_init.bin` uses **FECS method offsets** (0x000xxxxx),
//! not BAR0 register addresses. The init sequence must be submitted through
//! a FECS channel, not written directly to BAR0.
//!
//! However, the **pre-init steps** (master control, FIFO enable) ARE BAR0
//! register writes — they prepare the GPU so that FECS channels can be
//! created.
//!
//! The applicator splits the sequence into:
//! 1. **BAR0 writes**: Master control + FIFO (direct MMIO)
//! 2. **FECS method data**: Bundle/method init (submitted via channel)

use super::gr_init::{GrInitSequence, GrRegWrite, RegCategory};

/// Trait for BAR0 register access.
///
/// Matches the `hw_learn::applicator::RegisterAccess` interface
/// so nvPmu's `Bar0Access` can implement it directly.
pub trait RegisterAccess {
    /// Read a 32-bit register at a BAR0-relative offset.
    ///
    /// # Errors
    /// Returns error if hardware access fails.
    fn read_u32(&self, offset: u32) -> Result<u32, ApplyError>;

    /// Write a 32-bit register at a BAR0-relative offset.
    ///
    /// # Errors
    /// Returns error if hardware access fails.
    fn write_u32(&mut self, offset: u32, value: u32) -> Result<(), ApplyError>;
}

/// Errors during sovereign GSP application.
#[derive(Debug, thiserror::Error)]
pub enum ApplyError {
    /// BAR0 MMIO access failed.
    #[error("MMIO access failed at offset {offset:#010x}: {detail}")]
    MmioFailed {
        /// Register offset.
        offset: u32,
        /// Error detail.
        detail: String,
    },
    /// Verification read returned unexpected value.
    #[error("Verification failed at {offset:#010x}: got {actual:#010x}, expected {expected:#010x}")]
    VerifyFailed {
        /// Register offset.
        offset: u32,
        /// Value read from hardware.
        actual: u32,
        /// Expected value.
        expected: u32,
    },
    /// Thermal safety limit exceeded.
    #[error("Thermal safety: temperature {temp_c}C exceeds limit {limit_c}C")]
    ThermalLimit {
        /// Current temperature.
        temp_c: f64,
        /// Safety threshold.
        limit_c: f64,
    },
}

/// Result of applying a sovereign GSP init sequence.
#[derive(Debug)]
pub struct ApplyResult {
    /// Number of BAR0 writes successfully applied.
    pub bar0_writes: usize,
    /// Number of FECS method entries prepared (not yet submitted).
    pub fecs_entries: usize,
    /// Errors during application.
    pub errors: Vec<ApplyError>,
    /// Whether this was a dry run.
    pub dry_run: bool,
}

impl ApplyResult {
    /// Whether all BAR0 writes succeeded with no errors.
    #[must_use]
    pub fn success(&self) -> bool {
        self.errors.is_empty()
    }
}

/// Split a GR init sequence into BAR0-applicable and FECS-channel parts.
#[must_use]
pub fn split_for_application(seq: &GrInitSequence) -> (Vec<&GrRegWrite>, Vec<&GrRegWrite>) {
    let mut bar0_writes = Vec::new();
    let mut fecs_entries = Vec::new();

    for write in &seq.writes {
        match write.category {
            RegCategory::MasterControl | RegCategory::Fifo | RegCategory::Clock => {
                bar0_writes.push(write);
            }
            RegCategory::BundleInit
            | RegCategory::MethodInit
            | RegCategory::GrEngine
            | RegCategory::Verify => {
                fecs_entries.push(write);
            }
        }
    }

    (bar0_writes, fecs_entries)
}

/// Dry-run the BAR0 portion of a sovereign GSP init sequence.
///
/// Logs what would be written without touching hardware.
#[must_use]
pub fn dry_run(seq: &GrInitSequence) -> ApplyResult {
    let (bar0, fecs) = split_for_application(seq);

    for w in &bar0 {
        tracing::info!(
            chip = %seq.chip,
            offset = format!("{:#010x}", w.offset),
            value = format!("{:#010x}", w.value),
            category = ?w.category,
            delay_us = w.delay_us,
            "DRY RUN: would write BAR0 register"
        );
    }

    tracing::info!(
        chip = %seq.chip,
        fecs_entries = fecs.len(),
        "DRY RUN: {} FECS method entries would be submitted via channel",
        fecs.len()
    );

    ApplyResult {
        bar0_writes: bar0.len(),
        fecs_entries: fecs.len(),
        errors: Vec::new(),
        dry_run: true,
    }
}

/// Apply the BAR0 portion of a sovereign GSP init sequence to hardware.
///
/// Only writes the pre-init registers (master control, FIFO, clock).
/// The FECS method data is returned for channel submission separately.
///
/// # Errors
/// Returns result with any errors encountered during writes.
pub fn apply_bar0<R: RegisterAccess>(
    seq: &GrInitSequence,
    regs: &mut R,
) -> ApplyResult {
    let (bar0, fecs) = split_for_application(seq);
    let mut errors = Vec::new();
    let mut applied = 0usize;

    for w in &bar0 {
        match regs.write_u32(w.offset, w.value) {
            Ok(()) => {
                applied += 1;
                if w.delay_us > 0 {
                    std::thread::sleep(std::time::Duration::from_micros(u64::from(w.delay_us)));
                }
            }
            Err(e) => {
                tracing::error!(
                    offset = format!("{:#010x}", w.offset),
                    "BAR0 write failed: {e}"
                );
                errors.push(e);
            }
        }
    }

    ApplyResult {
        bar0_writes: applied,
        fecs_entries: fecs.len(),
        errors,
        dry_run: false,
    }
}

/// Verify engine state after BAR0 pre-init.
///
/// Reads key registers to confirm the GPU is ready for FECS channel creation:
/// - `NV_PMC_ENABLE` (0x200): All engines enabled
/// - `NV_PFIFO_ENABLE` (0x2504): FIFO running
///
/// # Errors
/// Returns verification errors for any failed checks.
pub fn verify_pre_init<R: RegisterAccess>(regs: &R) -> Vec<ApplyError> {
    let mut errors = Vec::new();

    let checks: &[(u32, u32, &str)] = &[
        (0x0000_0200, 0x0000_0001, "NV_PMC_ENABLE bit 0 (engine master)"),
        (0x0000_2504, 0x0000_0001, "NV_PFIFO_ENABLE"),
    ];

    for &(offset, expected_mask, desc) in checks {
        match regs.read_u32(offset) {
            Ok(val) => {
                if val & expected_mask != expected_mask {
                    tracing::warn!(
                        offset = format!("{offset:#010x}"),
                        value = format!("{val:#010x}"),
                        expected_mask = format!("{expected_mask:#010x}"),
                        desc,
                        "Verification failed"
                    );
                    errors.push(ApplyError::VerifyFailed {
                        offset,
                        actual: val,
                        expected: expected_mask,
                    });
                }
            }
            Err(e) => errors.push(e),
        }
    }

    errors
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gsp::firmware_parser::GrFirmwareBlobs;
    use crate::gsp::gr_init::GrInitSequence;
    use std::collections::BTreeMap;

    struct MockRegs {
        registers: BTreeMap<u32, u32>,
    }

    impl MockRegs {
        fn new() -> Self {
            Self {
                registers: BTreeMap::new(),
            }
        }
    }

    impl RegisterAccess for MockRegs {
        fn read_u32(&self, offset: u32) -> Result<u32, ApplyError> {
            self.registers.get(&offset).copied().ok_or(ApplyError::MmioFailed {
                offset,
                detail: "uninitialized".to_string(),
            })
        }

        fn write_u32(&mut self, offset: u32, value: u32) -> Result<(), ApplyError> {
            self.registers.insert(offset, value);
            Ok(())
        }
    }

    #[test]
    fn dry_run_gv100() {
        match GrFirmwareBlobs::parse("gv100") {
            Ok(blobs) => {
                let seq = GrInitSequence::for_gv100(&blobs);
                let result = dry_run(&seq);
                assert!(result.dry_run);
                assert!(result.success());
                assert!(result.bar0_writes > 0, "should have BAR0 writes");
                assert!(result.fecs_entries > 0, "should have FECS entries");
                eprintln!(
                    "GV100 dry run: {} BAR0 writes, {} FECS entries",
                    result.bar0_writes, result.fecs_entries
                );
            }
            Err(e) => eprintln!("GV100 firmware not present: {e}"),
        }
    }

    #[test]
    fn apply_bar0_mock() {
        match GrFirmwareBlobs::parse("gv100") {
            Ok(blobs) => {
                let seq = GrInitSequence::for_gv100(&blobs);
                let mut regs = MockRegs::new();
                let result = apply_bar0(&seq, &mut regs);
                assert!(!result.dry_run);
                assert!(result.success());
                assert_eq!(result.bar0_writes, 2); // PMC_ENABLE + FIFO_ENABLE

                // Verify the writes went through
                let errs = verify_pre_init(&regs);
                assert!(errs.is_empty(), "verification should pass: {errs:?}");
            }
            Err(e) => eprintln!("GV100 firmware not present: {e}"),
        }
    }

    #[test]
    fn split_categories() {
        match GrFirmwareBlobs::parse("gv100") {
            Ok(blobs) => {
                let seq = GrInitSequence::for_gv100(&blobs);
                let (bar0, fecs) = split_for_application(&seq);

                let total = bar0.len() + fecs.len();
                assert_eq!(total, seq.len());
                eprintln!(
                    "Split: {} BAR0 (pre-init) + {} FECS (channel) = {} total",
                    bar0.len(),
                    fecs.len(),
                    total
                );
            }
            Err(e) => eprintln!("GV100 firmware not present: {e}"),
        }
    }
}
