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
    pub const fn success(&self) -> bool {
        self.errors.is_empty()
    }
}

/// Push buffer method headers encode `method >> 2` in 13 bits.
/// Entries with addresses above this are BAR0 register writes, not
/// channel-submittable methods.
const MAX_CHANNEL_METHOD: u32 = 0x7FFC;

/// Split a GR init sequence into BAR0-applicable and FECS-channel parts.
///
/// Routing is both category-aware and address-aware:
/// - `MasterControl`, `Fifo`, `Clock` → always BAR0
/// - `BundleInit`, `MethodInit` with `offset > MAX_CHANNEL_METHOD` → BAR0
///   (these are PGRAPH register addresses, not push buffer methods)
/// - `BundleInit`, `MethodInit` with `offset <= MAX_CHANNEL_METHOD` → FECS
/// - `GrEngine`, `Verify` → FECS
#[must_use]
pub fn split_for_application(seq: &GrInitSequence) -> (Vec<&GrRegWrite>, Vec<&GrRegWrite>) {
    let mut bar0_writes = Vec::new();
    let mut fecs_entries = Vec::new();

    for write in &seq.writes {
        match write.category {
            RegCategory::MasterControl | RegCategory::Fifo | RegCategory::Clock => {
                bar0_writes.push(write);
            }
            RegCategory::BundleInit | RegCategory::MethodInit => {
                if write.offset > MAX_CHANNEL_METHOD {
                    bar0_writes.push(write);
                } else {
                    fecs_entries.push(write);
                }
            }
            RegCategory::GrEngine | RegCategory::Verify => {
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
pub fn apply_bar0<R: RegisterAccess>(seq: &GrInitSequence, regs: &mut R) -> ApplyResult {
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
        (
            0x0000_0200,
            0x0000_0001,
            "NV_PMC_ENABLE bit 0 (engine master)",
        ),
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
    use crate::gsp::test_utils::MockBar0;

    /// Large enough for real `GrInitSequence::for_gv100` BAR0-class writes (bundle MMIO offsets).
    const MOCK_BAR0_SIZE: usize = 0x0020_0000;

    #[test]
    fn dry_run_gv100() {
        match GrFirmwareBlobs::parse("gv100") {
            Ok(blobs) => {
                let seq = GrInitSequence::for_gv100(&blobs);
                let result = dry_run(&seq);
                assert!(result.dry_run);
                assert!(result.success());
                assert!(result.bar0_writes > 0, "should have BAR0 writes");
                tracing::debug!(
                    bar0_writes = result.bar0_writes,
                    fecs_entries = result.fecs_entries,
                    "GV100 dry run"
                );
            }
            Err(e) => tracing::debug!(error = %e, "GV100 firmware not present"),
        }
    }

    #[test]
    fn apply_bar0_mock() {
        // Synthetic bundle: avoids filesystem and keeps BAR0 offsets inside `MockBar0` (real
        // `sw_bundle_init.bin` can include MMIO past the mock window or odd alignments).
        let bundle = {
            let mut b = Vec::new();
            // Above `MAX_CHANNEL_METHOD` so `split_for_application` keeps these as BAR0; stay
            // inside [`MOCK_BAR0_SIZE`] (unlike real blobs that can use high PGRAPH MMIO).
            for (addr, value) in [(0x0001_0000u32, 1u32), (0x0001_0004u32, 2u32)] {
                b.extend_from_slice(&addr.to_le_bytes());
                b.extend_from_slice(&value.to_le_bytes());
            }
            b
        };
        let blobs = GrFirmwareBlobs::from_legacy_bytes(&bundle, &[], &[], &[], "gv100");
        let seq = GrInitSequence::for_gv100(&blobs);
        let mut regs = MockBar0::new(MOCK_BAR0_SIZE);
        let result = apply_bar0(&seq, &mut regs);
        assert!(!result.dry_run);
        assert!(result.success(), "{result:?}");
        assert!(result.bar0_writes >= 2, "at least PMC_ENABLE + FIFO_ENABLE");

        let errs = verify_pre_init(&regs);
        assert!(errs.is_empty(), "verification should pass: {errs:?}");
    }

    #[test]
    fn apply_error_display() {
        let e = ApplyError::MmioFailed {
            offset: 0x0000_0200,
            detail: "permission denied".to_string(),
        };
        let msg = e.to_string();
        assert!(msg.contains("0x00000200"));
        assert!(msg.contains("permission denied"));

        let e2 = ApplyError::VerifyFailed {
            offset: 0x2504,
            actual: 0,
            expected: 1,
        };
        let msg2 = e2.to_string();
        assert!(msg2.contains("0x00002504"));
        assert!(msg2.contains("got"));
        assert!(msg2.contains("expected"));

        let e3 = ApplyError::ThermalLimit {
            temp_c: 95.0,
            limit_c: 90.0,
        };
        let msg3 = e3.to_string();
        assert!(msg3.contains("95"));
        assert!(msg3.contains("90"));
    }

    #[test]
    fn split_for_application_bar0_vs_fecs_by_offset() {
        use crate::gsp::gr_init::{GrRegWrite, RegCategory};
        let seq = GrInitSequence {
            chip: "test".to_string(),
            writes: vec![
                GrRegWrite {
                    offset: 0x0000_0200,
                    value: 1,
                    category: RegCategory::MasterControl,
                    delay_us: 0,
                },
                GrRegWrite {
                    offset: 0x1000, // <= 0x7FFC, goes to FECS
                    value: 0x42,
                    category: RegCategory::MethodInit,
                    delay_us: 0,
                },
                GrRegWrite {
                    offset: 0x0080_0000, // > 0x7FFC, BAR0
                    value: 0xDEAD,
                    category: RegCategory::BundleInit,
                    delay_us: 0,
                },
            ],
        };
        let (bar0, fecs) = split_for_application(&seq);
        assert_eq!(bar0.len(), 2, "MasterControl + high-offset BundleInit");
        assert_eq!(fecs.len(), 1, "low-offset MethodInit");
    }

    #[test]
    fn apply_bar0_mock_failure_propagates() {
        use crate::gsp::gr_init::{GrRegWrite, RegCategory};

        struct FailingMock;
        impl RegisterAccess for FailingMock {
            fn read_u32(&self, _: u32) -> Result<u32, ApplyError> {
                Err(ApplyError::MmioFailed {
                    offset: 0,
                    detail: "read failed".to_string(),
                })
            }
            fn write_u32(&mut self, offset: u32, _: u32) -> Result<(), ApplyError> {
                Err(ApplyError::MmioFailed {
                    offset,
                    detail: "write failed".to_string(),
                })
            }
        }

        let seq = GrInitSequence {
            chip: "test".to_string(),
            writes: vec![GrRegWrite {
                offset: 0x0000_0200,
                value: 1,
                category: RegCategory::MasterControl,
                delay_us: 0,
            }],
        };
        let mut regs = FailingMock;
        let result = apply_bar0(&seq, &mut regs);
        assert!(!result.success());
        assert_eq!(result.errors.len(), 1);
        assert!(result.bar0_writes == 0);
    }

    #[test]
    fn verify_pre_init_fails_on_mismatch() {
        struct MockRegsReadWrong;
        impl RegisterAccess for MockRegsReadWrong {
            fn read_u32(&self, offset: u32) -> Result<u32, ApplyError> {
                Ok(match offset {
                    0x0000_0200 => 0, // expected bit 0 set
                    0x0000_2504 => 0, // expected bit 0 set
                    _ => 0,
                })
            }
            fn write_u32(&mut self, _: u32, _: u32) -> Result<(), ApplyError> {
                Ok(())
            }
        }

        let regs = MockRegsReadWrong;
        let errs = verify_pre_init(&regs);
        assert_eq!(errs.len(), 2);
        assert!(
            errs.iter()
                .all(|e| matches!(e, ApplyError::VerifyFailed { .. }))
        );
    }

    #[test]
    fn split_categories() {
        match GrFirmwareBlobs::parse("gv100") {
            Ok(blobs) => {
                let seq = GrInitSequence::for_gv100(&blobs);
                let (bar0, fecs) = split_for_application(&seq);

                let total = bar0.len() + fecs.len();
                assert_eq!(total, seq.len());
                tracing::debug!(
                    bar0 = bar0.len(),
                    fecs = fecs.len(),
                    total,
                    "split_for_application"
                );
            }
            Err(e) => tracing::debug!(error = %e, "GV100 firmware not present"),
        }
    }

    #[test]
    fn split_for_application_gr_engine_and_verify_go_to_fecs() {
        use crate::gsp::gr_init::{GrRegWrite, RegCategory};
        let seq = GrInitSequence {
            chip: "test".to_string(),
            writes: vec![
                GrRegWrite {
                    offset: 0x0040_1000,
                    value: 1,
                    category: RegCategory::GrEngine,
                    delay_us: 0,
                },
                GrRegWrite {
                    offset: 0x0040_2000,
                    value: 2,
                    category: RegCategory::Verify,
                    delay_us: 0,
                },
            ],
        };
        let (bar0, fecs) = split_for_application(&seq);
        assert_eq!(bar0.len(), 0);
        assert_eq!(fecs.len(), 2);
    }

    #[test]
    fn split_for_application_clock_goes_to_bar0() {
        use crate::gsp::gr_init::{GrRegWrite, RegCategory};
        let seq = GrInitSequence {
            chip: "test".to_string(),
            writes: vec![GrRegWrite {
                offset: 0x0010_0000,
                value: 0x1234,
                category: RegCategory::Clock,
                delay_us: 50,
            }],
        };
        let (bar0, fecs) = split_for_application(&seq);
        assert_eq!(bar0.len(), 1);
        assert_eq!(bar0[0].offset, 0x0010_0000);
        assert_eq!(bar0[0].value, 0x1234);
        assert_eq!(bar0[0].delay_us, 50);
        assert_eq!(fecs.len(), 0);
    }

    #[test]
    fn split_for_application_fifo_goes_to_bar0() {
        use crate::gsp::gr_init::{GrRegWrite, RegCategory};
        let seq = GrInitSequence {
            chip: "test".to_string(),
            writes: vec![GrRegWrite {
                offset: 0x0000_2504,
                value: 1,
                category: RegCategory::Fifo,
                delay_us: 0,
            }],
        };
        let (bar0, fecs) = split_for_application(&seq);
        assert_eq!(bar0.len(), 1);
        assert_eq!(fecs.len(), 0);
    }

    #[test]
    fn apply_result_success_with_no_errors() {
        let result = ApplyResult {
            bar0_writes: 5,
            fecs_entries: 100,
            errors: Vec::new(),
            dry_run: false,
        };
        assert!(result.success());
    }

    #[test]
    fn apply_result_success_false_with_errors() {
        let result = ApplyResult {
            bar0_writes: 2,
            fecs_entries: 50,
            errors: vec![ApplyError::MmioFailed {
                offset: 0x200,
                detail: "test".to_string(),
            }],
            dry_run: false,
        };
        assert!(!result.success());
    }

    #[test]
    fn apply_bar0_mock_register_map_verification() {
        use crate::gsp::gr_init::{GrRegWrite, RegCategory};
        let seq = GrInitSequence {
            chip: "test".to_string(),
            writes: vec![
                GrRegWrite {
                    offset: 0x0000_0200,
                    value: 0xFFFF_FFFF,
                    category: RegCategory::MasterControl,
                    delay_us: 0,
                },
                GrRegWrite {
                    offset: 0x0000_2504,
                    value: 0x0000_0001,
                    category: RegCategory::Fifo,
                    delay_us: 0,
                },
            ],
        };
        let mut regs = MockBar0::new(MOCK_BAR0_SIZE);
        let result = apply_bar0(&seq, &mut regs);
        assert!(result.success());
        assert_eq!(result.bar0_writes, 2);

        assert_eq!(regs.read_u32(0x0000_0200).expect("read PMC"), 0xFFFF_FFFF);
        assert_eq!(regs.read_u32(0x0000_2504).expect("read PFIFO"), 0x0000_0001);

        let errs = verify_pre_init(&regs);
        assert!(errs.is_empty());
    }

    #[test]
    fn mock_bar0_apply_bar0_then_verify_pre_init_bytes() {
        use crate::gsp::gr_init::{GrRegWrite, RegCategory};
        let seq = GrInitSequence {
            chip: "mock-bytes".to_string(),
            writes: vec![
                GrRegWrite {
                    offset: 0x0000_0200,
                    value: 0x0000_0001,
                    category: RegCategory::MasterControl,
                    delay_us: 0,
                },
                GrRegWrite {
                    offset: 0x0000_2504,
                    value: 0x0000_0001,
                    category: RegCategory::Fifo,
                    delay_us: 0,
                },
            ],
        };
        let mut bar = MockBar0::new(MOCK_BAR0_SIZE);
        let result = apply_bar0(&seq, &mut bar);
        assert!(result.success(), "{result:?}");
        assert_eq!(result.bar0_writes, 2);
        assert_eq!(result.fecs_entries, 0);

        assert_eq!(bar.read_u32(0x200).expect("PMC"), 1);
        assert_eq!(bar.read_u32(0x2504).expect("PFIFO"), 1);
        assert!(verify_pre_init(&bar).is_empty());
    }

    #[test]
    fn mock_bar0_seed_then_verify_pre_init() {
        let mut bar = MockBar0::new(MOCK_BAR0_SIZE);
        bar.seed_u32(0x0000_0200, 0x0000_0001);
        bar.seed_u32(0x0000_2504, 0x0000_0001);
        assert!(verify_pre_init(&bar).is_empty());
    }

    #[test]
    fn dry_run_synthetic_sequence_no_firmware() {
        use crate::gsp::gr_init::{GrRegWrite, RegCategory};
        let seq = GrInitSequence {
            chip: "synthetic".to_string(),
            writes: vec![
                GrRegWrite {
                    offset: 0x0000_0200,
                    value: 1,
                    category: RegCategory::MasterControl,
                    delay_us: 100,
                },
                GrRegWrite {
                    offset: 0x1000,
                    value: 0x42,
                    category: RegCategory::MethodInit,
                    delay_us: 0,
                },
            ],
        };
        let result = dry_run(&seq);
        assert!(result.dry_run);
        assert!(result.success());
        assert_eq!(result.bar0_writes, 1);
        assert_eq!(result.fecs_entries, 1);
    }
}
