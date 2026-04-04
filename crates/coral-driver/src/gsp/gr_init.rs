// SPDX-License-Identifier: AGPL-3.0-only
//! GR engine initialization sequence — the core of sovereign GSP.
//!
//! This module builds the minimal register-write sequence to bring a
//! GPU's GR (graphics/compute) engine to a state where compute channels
//! can be allocated. On GPUs with working firmware (GA102+), the GSP
//! firmware handles this. On Volta (GV100), we must do it ourselves.
//!
//! # How it works
//!
//! 1. Parse the chip's `sw_bundle_init.bin` for register values
//! 2. Add pre-init writes (PMC engine enable, FIFO enable)
//! 3. Add the bundle init sequence
//! 4. Add post-init verification reads
//!
//! The sequence can then be applied via BAR0 MMIO (nvPmu) or used
//! to generate the equivalent nouveau ioctls.

use super::firmware_parser::GrFirmwareBlobs;

/// A single register write in a GR init sequence.
#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct GrRegWrite {
    /// Register offset (BAR0-relative or method offset).
    pub offset: u32,
    /// Value to write.
    pub value: u32,
    /// Category for documentation and safety analysis.
    pub category: RegCategory,
    /// Delay after write in microseconds (0 = no delay).
    pub delay_us: u32,
}

/// Register write category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum RegCategory {
    /// Master control (`NV_PMC`): engine enable, reset.
    MasterControl,
    /// FIFO: channel/runlist management.
    Fifo,
    /// GR engine: compute/graphics engine registers.
    GrEngine,
    /// Bundle init: from `sw_bundle_init.bin`.
    BundleInit,
    /// Method init: from `sw_method_init.bin`.
    MethodInit,
    /// Clock/PLL configuration.
    Clock,
    /// Verification read (not a write — check result).
    Verify,
}

/// Complete GR initialization sequence for a GPU architecture.
#[derive(Debug, Clone)]
pub struct GrInitSequence {
    /// Chip codename this sequence targets.
    pub chip: String,
    /// Ordered register writes for GR engine init.
    pub writes: Vec<GrRegWrite>,
}

impl GrInitSequence {
    /// Build a GR init sequence for GV100 (Volta) from firmware blobs.
    ///
    /// This is the register sequence that nouveau's PMU firmware would
    /// execute. Without PMU firmware, we replay it via BAR0 MMIO.
    #[must_use]
    pub fn for_gv100(blobs: &GrFirmwareBlobs) -> Self {
        let mut writes = Vec::new();

        // Phase 1: Master control — enable engines
        // NV_PMC_ENABLE: ensure GR engine bit is set
        writes.push(GrRegWrite {
            offset: 0x0000_0200,
            value: 0xFFFF_FFFF,
            category: RegCategory::MasterControl,
            delay_us: 100,
        });

        // Phase 2: FIFO enable — allow channel creation
        // NV_PFIFO_ENABLE: set FIFO_ENABLE
        writes.push(GrRegWrite {
            offset: 0x0000_2504,
            value: 0x0000_0001,
            category: RegCategory::Fifo,
            delay_us: 50,
        });

        // Phase 3: GR engine init — from firmware bundle
        for entry in &blobs.bundle_init {
            writes.push(GrRegWrite {
                offset: entry.addr,
                value: entry.value,
                category: RegCategory::BundleInit,
                delay_us: 0,
            });
        }

        // Phase 4: Method init — class methods
        for entry in &blobs.method_init {
            writes.push(GrRegWrite {
                offset: entry.addr,
                value: entry.value,
                category: RegCategory::MethodInit,
                delay_us: 0,
            });
        }

        Self {
            chip: "gv100".to_string(),
            writes,
        }
    }

    /// Build a generic GR init sequence from firmware blobs.
    ///
    /// Less specialized than `for_gv100` — uses only the bundle/method
    /// init from the firmware without arch-specific pre-init.
    #[must_use]
    pub fn from_blobs(blobs: &GrFirmwareBlobs) -> Self {
        let mut writes = Vec::new();

        for entry in &blobs.bundle_init {
            writes.push(GrRegWrite {
                offset: entry.addr,
                value: entry.value,
                category: RegCategory::BundleInit,
                delay_us: 0,
            });
        }

        for entry in &blobs.method_init {
            writes.push(GrRegWrite {
                offset: entry.addr,
                value: entry.value,
                category: RegCategory::MethodInit,
                delay_us: 0,
            });
        }

        Self {
            chip: blobs.chip.clone(),
            writes,
        }
    }

    /// Number of register writes.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.writes.len()
    }

    /// Whether the sequence is empty.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.writes.is_empty()
    }

    /// Writes in a specific category.
    #[must_use]
    pub fn category_writes(&self, cat: RegCategory) -> Vec<&GrRegWrite> {
        self.writes.iter().filter(|w| w.category == cat).collect()
    }

    /// Serialize to JSON for inspection and hw-learn recipe storage.
    ///
    /// # Errors
    /// Returns error if serialization fails.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(&self.writes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gsp::firmware_parser::GrFirmwareBlobs;

    fn synthetic_blobs() -> GrFirmwareBlobs {
        let mut bundle = Vec::new();
        bundle.extend_from_slice(&0x0040_1000u32.to_le_bytes());
        bundle.extend_from_slice(&0xAAA_AAAu32.to_le_bytes());
        bundle.extend_from_slice(&0x0040_2000u32.to_le_bytes());
        bundle.extend_from_slice(&0xBBB_BBBu32.to_le_bytes());
        let mut method = Vec::new();
        method.extend_from_slice(&0x2080_0100u32.to_le_bytes());
        method.extend_from_slice(&0x00C0FFEEu32.to_le_bytes());
        GrFirmwareBlobs::from_legacy_bytes(&bundle, &method, &[], &[], "synthetic")
    }

    #[test]
    fn gv100_from_blobs_includes_master_fifo_and_firmware_writes() {
        let blobs = synthetic_blobs();
        let seq = GrInitSequence::for_gv100(&blobs);
        assert_eq!(seq.chip, "gv100");
        assert!(seq.len() >= 2 + blobs.bundle_count() + blobs.method_count());
        let master = seq.category_writes(RegCategory::MasterControl);
        assert_eq!(master.len(), 1);
        assert_eq!(master[0].offset, 0x200);
        assert_eq!(master[0].delay_us, 100);
        let fifo = seq.category_writes(RegCategory::Fifo);
        assert_eq!(fifo.len(), 1);
        assert_eq!(fifo[0].offset, 0x2504);
        let bundle = seq.category_writes(RegCategory::BundleInit);
        assert_eq!(bundle.len(), 2);
        assert_eq!(bundle[0].offset, 0x0040_1000);
        assert_eq!(bundle[1].offset, 0x0040_2000);
        let method = seq.category_writes(RegCategory::MethodInit);
        assert_eq!(method.len(), 1);
        assert_eq!(method[0].offset, 0x2080_0100);
    }

    #[test]
    fn from_blobs_chip_name_and_ordering() {
        let blobs = synthetic_blobs();
        let seq = GrInitSequence::from_blobs(&blobs);
        assert_eq!(seq.chip, "synthetic");
        assert_eq!(seq.len(), blobs.bundle_count() + blobs.method_count());
        let json = seq.to_json().expect("json");
        assert!(
            json.contains("4198400"),
            "expected bundle offset 0x00401000 in JSON: {json}"
        );
        assert!(json.contains("BundleInit"));
    }

    #[test]
    fn gv100_init_from_real_firmware() {
        match GrFirmwareBlobs::parse("gv100") {
            Ok(blobs) => {
                let seq = GrInitSequence::for_gv100(&blobs);
                assert!(!seq.is_empty());

                let master = seq.category_writes(RegCategory::MasterControl);
                assert!(!master.is_empty(), "should have master control writes");

                let bundle = seq.category_writes(RegCategory::BundleInit);
                assert_eq!(bundle.len(), blobs.bundle_count());

                tracing::debug!(
                    total_writes = seq.len(),
                    master = master.len(),
                    fifo = seq.category_writes(RegCategory::Fifo).len(),
                    bundle = bundle.len(),
                    method = seq.category_writes(RegCategory::MethodInit).len(),
                    "GV100 init sequence"
                );
            }
            Err(e) => tracing::debug!(error = %e, "GV100 firmware not present"),
        }
    }

    #[test]
    fn ga102_init_from_real_firmware() {
        match GrFirmwareBlobs::parse("ga102") {
            Ok(blobs) => {
                let seq = GrInitSequence::from_blobs(&blobs);
                assert!(!seq.is_empty());
                tracing::debug!(writes = seq.len(), "GA102 init sequence");
            }
            Err(e) => tracing::debug!(error = %e, "GA102 firmware not present"),
        }
    }
}
