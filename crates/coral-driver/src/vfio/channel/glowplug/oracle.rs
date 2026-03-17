// SPDX-License-Identifier: AGPL-3.0-only
//! Oracle register cloning from a nouveau-warm card.

use super::super::pri_monitor::{PriBusMonitor, WriteOutcome};
use super::super::registers::misc;
use super::GlowPlug;
use super::constants::{ORACLE_RANGES, is_dangerous_register};

impl GlowPlug<'_> {
    /// Clone register state from an oracle (nouveau-warm) card of the same model.
    ///
    /// Opens the oracle's BAR0 via sysfs `resource0` (must be readable),
    /// reads all registers in `ORACLE_RANGES`, compares with the cold card,
    /// and applies differences. Returns `(applied, stuck, total_diff)` counts.
    pub fn apply_oracle_registers(&self, log: &mut Vec<String>) -> (usize, usize, usize) {
        let oracle_bdf = match &self.oracle_bdf {
            Some(b) => b.clone(),
            None => {
                log.push("oracle: no oracle BDF configured".into());
                return (0, 0, 0);
            }
        };

        use crate::vfio::sysfs_bar0::{DEFAULT_BAR0_SIZE, SysfsBar0};

        let oracle_bar0 = match SysfsBar0::open(&oracle_bdf, DEFAULT_BAR0_SIZE) {
            Ok(b) => b,
            Err(e) => {
                log.push(format!("oracle: {e}"));
                return (0, 0, 0);
            }
        };

        // Verify oracle is the same GPU
        let oracle_boot0 = oracle_bar0.read_u32(0);
        let cold_boot0 = self.r(misc::BOOT0);
        if oracle_boot0 != cold_boot0 {
            log.push(format!(
                "oracle: BOOT0 mismatch! oracle={oracle_boot0:#010x} cold={cold_boot0:#010x}"
            ));
            return (0, 0, 0);
        }

        log.push(format!(
            "oracle: reading {} ranges from {oracle_bdf} (BOOT0={oracle_boot0:#010x})",
            ORACLE_RANGES.len()
        ));

        // Collect all diffs
        let mut diffs: Vec<(usize, u32, u32)> = Vec::new();
        for &(name, start, end) in ORACLE_RANGES {
            let mut range_diffs = 0;
            for off in (start..end).step_by(4) {
                let ov = oracle_bar0.read_u32(off);
                let cv = self.r(off);
                // Skip if both are error patterns or identical
                if ov == cv {
                    continue;
                }
                if ov == 0xFFFFFFFF || ov == 0xDEADDEAD {
                    continue;
                }
                if (ov & 0xFFFF0000) == 0xBADF0000 {
                    continue;
                } // PRI error on oracle
                diffs.push((off, ov, cv));
                range_diffs += 1;
            }
            if range_diffs > 0 {
                log.push(format!("oracle: {name}: {range_diffs} diffs"));
            }
        }

        let total_diff = diffs.len();
        log.push(format!("oracle: total {total_diff} register differences"));

        // Apply oracle values with PRI backpressure monitoring
        let mut applied = 0;
        let mut stuck = 0;
        let mut pri_skipped = 0;
        let mut monitor = PriBusMonitor::new(self.bar0).with_fault_threshold(5);

        for &(off, ov, _cv) in &diffs {
            if is_dangerous_register(off) {
                continue;
            }
            match monitor.write_u32(off, ov) {
                WriteOutcome::Applied => applied += 1,
                WriteOutcome::SkippedFaulted | WriteOutcome::Throttled => pri_skipped += 1,
                WriteOutcome::AppliedButFaulted => {
                    applied += 1;
                    if applied % 20 == 0 {
                        monitor.attempt_recovery();
                    }
                }
            }
        }

        if pri_skipped > 0 {
            log.push(format!(
                "oracle: {pri_skipped} writes PRI-skipped (domain faulted)"
            ));
        }

        std::thread::sleep(std::time::Duration::from_millis(50));

        // Verify a sample of writes
        for &(off, ov, _) in diffs.iter().take(100) {
            if is_dangerous_register(off) {
                continue;
            }
            let rb = self.r(off);
            if rb != ov {
                stuck += 1;
                if stuck <= 10 {
                    log.push(format!(
                        "oracle: STUCK [{off:#010x}] wrote={ov:#010x} rb={rb:#010x}"
                    ));
                }
            }
        }

        log.push(format!(
            "oracle: applied {applied}, stuck {stuck}, total_diff {total_diff}"
        ));

        drop(oracle_bar0);
        (applied, stuck, total_diff)
    }
}
