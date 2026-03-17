// SPDX-License-Identifier: AGPL-3.0-only
#![allow(missing_docs)]
//! Oracle data loading and digital PMU emulation.
//!
//! This module provides the bridge between captured oracle data
//! (from a nouveau-warm card's BAR0 or an mmiotrace-derived recipe)
//! and the GlowPlug sovereign init sequence.
//!
//! ## Data Sources
//!
//! 1. **BAR0 binary dump** — raw 16MB BAR0 image from sysfs `resource0`
//! 2. **Domain register text dump** — `DOMAIN 0xOFFSET 0xVALUE` per line
//! 3. **mmiotrace-distilled recipe** — JSON `InitRecipe` from hw-learn
//!
//! ## Digital PMU Emulation
//!
//! Instead of running signed firmware on the FALCON microcontroller,
//! we replicate the PMU's *function* in software: writing registers
//! in the correct order with values from the oracle.
//!
//! The key insight: root PLLs at 0x136xxx are in an always-on power
//! domain and writable from the host. These feed reference clocks to
//! PCLOCK, which gates everything downstream. By programming the oracle's
//! PLL values, we can ungate FBPA/LTC/PFB without the PMU.

use std::collections::BTreeMap;
use std::path::Path;

use super::pri_monitor::{PriBusMonitor, WriteOutcome};
use super::registers::pri;
use crate::vfio::device::MappedBar;
use crate::vfio::memory::MemoryRegion;

/// Oracle register state loaded from a capture.
#[derive(Debug, Clone)]
pub struct OracleState {
    /// All register values: offset → value.
    /// Stored in a BTreeMap for ordered iteration (low offsets first).
    pub registers: BTreeMap<usize, u32>,
    /// Source description (e.g., "BAR0 dump from 0000:03:00.0").
    pub source: String,
}

/// Result of applying oracle data to a cold card.
#[derive(Debug)]
pub struct OracleApplyResult {
    /// Total register differences found between oracle and cold card.
    pub total_diffs: usize,
    /// Writes successfully applied (readback matched).
    pub applied: usize,
    /// Writes that didn't take (readback != written value).
    pub stuck: usize,
    /// Writes skipped due to PRI faults in the target domain.
    pub pri_skipped: usize,
    /// Writes skipped because the register is dangerous.
    pub danger_skipped: usize,
    /// Per-domain results.
    pub domain_results: Vec<DomainApplyResult>,
    /// Whether VRAM became accessible during or after application.
    pub vram_unlocked: bool,
    /// Domain after which VRAM first became accessible (if any).
    pub vram_unlocked_after: Option<String>,
    /// Diagnostic log messages.
    pub log: Vec<String>,
}

/// Per-domain application result.
#[derive(Debug, Clone)]
pub struct DomainApplyResult {
    pub name: String,
    pub start: usize,
    pub end: usize,
    pub diffs: usize,
    pub applied: usize,
    pub stuck: usize,
    pub pri_skipped: usize,
}

/// Register domains for ordered application.
/// Applied in dependency order: clocks first, then memory, then engines.
const APPLY_ORDER: &[(&str, usize, usize)] = &[
    // Phase 1: Root clocks (always-on domain — the crack in the wall)
    ("ROOT_PLL", 0x136000, 0x137000),
    // Phase 2: PCLOCK configuration (depends on root PLL)
    ("PCLOCK", 0x137000, 0x138000),
    // Phase 3: Clock distribution (CLK domain)
    ("CLK", 0x130000, 0x136000),
    // Phase 4: Power management controller
    ("PMC", 0x000000, 0x001000),
    // Phase 5: PRI master (bus arbitration)
    ("PRI_MASTER", 0x122000, 0x123000),
    // Phase 6: Memory controller (PFB)
    ("PFB", 0x100000, 0x100800),
    ("FBHUB", 0x100800, 0x100C00),
    ("PFB_NISO", 0x100C00, 0x101000),
    // Phase 7: FBPA partitions (HBM2 controllers)
    ("FBPA0", 0x9A0000, 0x9A4000),
    ("FBPA1", 0x9A4000, 0x9A8000),
    ("FBPA_BC", 0x9A8000, 0x9AC000),
    // Phase 8: L2 cache (LTC)
    ("LTC0", 0x17E000, 0x180000),
    ("LTC1", 0x180000, 0x182000),
    ("LTC2", 0x182000, 0x184000),
    // Phase 9: PMU FALCON
    ("PMU", 0x10A000, 0x10B000),
    // Phase 10: Other domains
    ("PBUS", 0x001000, 0x002000),
    ("PTOP", 0x020000, 0x024000),
    ("FUSE", 0x021000, 0x022000),
    ("PMEM", 0x1FA000, 0x1FB000),
];

/// Registers that must NEVER be written (triggers, invalidations, counters).
fn is_dangerous_register(off: usize) -> bool {
    matches!(off,
        0x009000..=0x0090FF |  // PTIMER (dynamic counters)
        0x610000..=0x610FFF |  // PDISP (display engine)
        0x100CBC | 0x100CB8 | 0x100CEC |  // MMU invalidation triggers
        0x100E24..=0x100E54 |  // Fault buffer registers
        0x10A040..=0x10A048 |  // PMU mailboxes (dynamic)
        0x10A100             | // PMU CPUCTL
        0x10A10C             | // PMU CPUCTL_ALIAS
        0x10A104..=0x10A108    // PMU FALCON IMEM/DMEM triggers
    )
}

impl OracleState {
    /// Load oracle state from a raw BAR0 binary dump (16MB file).
    pub fn from_bar0_dump(path: &Path) -> Result<Self, String> {
        let data = std::fs::read(path)
            .map_err(|e| format!("cannot read BAR0 dump {}: {e}", path.display()))?;
        if data.len() < 0x100000 {
            return Err(format!("BAR0 dump too small: {} bytes", data.len()));
        }

        let mut registers = BTreeMap::new();
        for &(_, start, end) in APPLY_ORDER {
            if end > data.len() {
                continue;
            }
            for off in (start..end).step_by(4) {
                let val =
                    u32::from_le_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]]);
                if val == 0 || val == 0xFFFFFFFF {
                    continue;
                }
                if pri::is_pri_error(val) {
                    continue;
                }
                registers.insert(off, val);
            }
        }

        Ok(Self {
            registers,
            source: format!("BAR0 dump: {}", path.display()),
        })
    }

    /// Load oracle state from a text dump (format: `DOMAIN 0xOFFSET 0xVALUE`).
    pub fn from_text_dump(path: &Path) -> Result<Self, String> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("cannot read text dump {}: {e}", path.display()))?;

        let mut registers = BTreeMap::new();
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() < 2 {
                continue;
            }

            let (off_str, val_str) = if parts.len() == 3 {
                (parts[1], parts[2])
            } else {
                (parts[0], parts[1])
            };

            let off = usize::from_str_radix(off_str.trim_start_matches("0x"), 16)
                .map_err(|_| format!("bad offset: {off_str}"))?;
            let val = u32::from_str_radix(val_str.trim_start_matches("0x"), 16)
                .map_err(|_| format!("bad value: {val_str}"))?;

            registers.insert(off, val);
        }

        Ok(Self {
            registers,
            source: format!("text dump: {}", path.display()),
        })
    }

    /// Load oracle state from a live nouveau-warm card via sysfs mmap.
    pub fn from_live_card(bdf: &str) -> Result<Self, String> {
        use crate::vfio::sysfs_bar0::{DEFAULT_BAR0_SIZE, SysfsBar0};

        let bar0 = SysfsBar0::open(bdf, DEFAULT_BAR0_SIZE)?;

        let mut registers = BTreeMap::new();
        for &(_, start, end) in APPLY_ORDER {
            for off in (start..end.min(bar0.size())).step_by(4) {
                let val = bar0.read_u32(off);
                if val == 0 || val == 0xFFFF_FFFF {
                    continue;
                }
                if pri::is_pri_error(val) {
                    continue;
                }
                registers.insert(off, val);
            }
        }

        Ok(Self {
            registers,
            source: format!("live card: {bdf}"),
        })
    }

    /// Get registers for a specific domain.
    pub fn domain_registers(&self, start: usize, end: usize) -> Vec<(usize, u32)> {
        self.registers
            .range(start..end)
            .map(|(&off, &val)| (off, val))
            .collect()
    }

    /// Get root PLL registers (0x136000-0x136FFF).
    pub fn root_pll_registers(&self) -> Vec<(usize, u32)> {
        self.domain_registers(0x136000, 0x137000)
    }

    /// Get PCLOCK registers (0x137000-0x137FFF).
    pub fn pclock_registers(&self) -> Vec<(usize, u32)> {
        self.domain_registers(0x137000, 0x138000)
    }

    /// Get FBPA registers for a specific partition.
    pub fn fbpa_registers(&self, partition: usize) -> Vec<(usize, u32)> {
        let base = 0x9A0000 + partition * 0x4000;
        self.domain_registers(base, base + 0x4000)
    }
}

/// Digital PMU — replicates the PMU FALCON's initialization function in software.
///
/// Instead of uploading signed firmware to the FALCON and executing it,
/// we program the same registers from the host in the correct order,
/// using oracle data for target values.
pub struct DigitalPmu<'a> {
    bar0: &'a MappedBar,
    oracle: &'a OracleState,
    log: Vec<String>,
}

impl<'a> DigitalPmu<'a> {
    pub fn new(bar0: &'a MappedBar, oracle: &'a OracleState) -> Self {
        Self {
            bar0,
            oracle,
            log: Vec::new(),
        }
    }

    /// Execute the full digital PMU init sequence.
    ///
    /// Applies oracle register values in dependency order:
    /// 1. Root PLLs (always-on domain)
    /// 2. PCLOCK (depends on root PLLs)
    /// 3. CLK distribution
    /// 4. PMC enable
    /// 5. Memory controller (PFB, FBHUB, FBPA)
    /// 6. L2 cache (LTC)
    /// 7. Other domains
    ///
    /// After each phase, checks if the downstream clock gates have opened
    /// and if VRAM has become accessible.
    pub fn execute(&mut self) -> OracleApplyResult {
        let mut result = OracleApplyResult {
            total_diffs: 0,
            applied: 0,
            stuck: 0,
            pri_skipped: 0,
            danger_skipped: 0,
            domain_results: Vec::new(),
            vram_unlocked: false,
            vram_unlocked_after: None,
            log: Vec::new(),
        };

        self.log.push(format!(
            "Digital PMU: {} oracle registers from {}",
            self.oracle.registers.len(),
            self.oracle.source,
        ));

        let mut monitor = PriBusMonitor::new(self.bar0).with_fault_threshold(10);

        for &(domain_name, start, end) in APPLY_ORDER {
            let oracle_regs = self.oracle.domain_registers(start, end);
            if oracle_regs.is_empty() {
                continue;
            }

            let mut domain_result = DomainApplyResult {
                name: domain_name.to_string(),
                start,
                end,
                diffs: 0,
                applied: 0,
                stuck: 0,
                pri_skipped: 0,
            };

            self.log.push(format!(
                "  Phase: {} ({:#08x}-{:#08x}, {} oracle regs)",
                domain_name,
                start,
                end,
                oracle_regs.len(),
            ));

            // Clear PRI faults before each domain
            monitor.attempt_recovery();

            for (off, oracle_val) in &oracle_regs {
                if is_dangerous_register(*off) {
                    result.danger_skipped += 1;
                    continue;
                }

                let cold_val = monitor.read_u32(*off);
                if cold_val == *oracle_val {
                    continue;
                }

                domain_result.diffs += 1;
                result.total_diffs += 1;

                match monitor.write_u32(*off, *oracle_val) {
                    WriteOutcome::Applied => {
                        domain_result.applied += 1;
                        result.applied += 1;
                    }
                    WriteOutcome::SkippedFaulted | WriteOutcome::Throttled => {
                        domain_result.pri_skipped += 1;
                        result.pri_skipped += 1;
                    }
                    WriteOutcome::AppliedButFaulted => {
                        domain_result.applied += 1;
                        result.applied += 1;
                        // Periodic recovery for domains with mixed access
                        if domain_result.applied.is_multiple_of(20) {
                            monitor.attempt_recovery();
                        }
                    }
                }
            }

            // Post-domain: verify a sample and check for VRAM unlock
            let verify_count = domain_result.applied.min(50);
            let mut verify_stuck = 0;
            for (off, oracle_val) in oracle_regs.iter().take(verify_count) {
                if is_dangerous_register(*off) {
                    continue;
                }
                let rb = monitor.read_u32(*off);
                if rb != *oracle_val && !pri::is_pri_error(rb) {
                    verify_stuck += 1;
                }
            }
            domain_result.stuck = verify_stuck;
            result.stuck += verify_stuck;

            self.log.push(format!(
                "    {} diffs, {} applied, {} stuck, {} PRI-skipped",
                domain_result.diffs,
                domain_result.applied,
                domain_result.stuck,
                domain_result.pri_skipped,
            ));

            result.domain_results.push(domain_result);

            // Check VRAM after critical domains
            if matches!(
                domain_name,
                "PCLOCK" | "PFB" | "FBPA0" | "FBPA1" | "FBPA_BC" | "LTC0"
            ) && !result.vram_unlocked
                && self.check_vram()
            {
                result.vram_unlocked = true;
                result.vram_unlocked_after = Some(domain_name.to_string());
                self.log
                    .push(format!("    *** VRAM UNLOCKED after {domain_name}! ***"));
            }

            // Small delay between domains for PLL lock / clock propagation
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        // Final VRAM check
        if !result.vram_unlocked && self.check_vram() {
            result.vram_unlocked = true;
            result.vram_unlocked_after = Some("final".to_string());
            self.log
                .push("  *** VRAM UNLOCKED after all phases! ***".into());
        }

        let stats = monitor.stats();
        self.log.push(format!(
            "  PRI stats: {} reads ({} faulted), {} writes ({} applied, {} skipped), {} recoveries",
            stats.reads_total,
            stats.reads_faulted,
            stats.writes_total,
            stats.writes_applied,
            stats.writes_skipped_faulted,
            stats.bus_recoveries,
        ));

        result.log = self.log.clone();
        result
    }

    /// Program only the root PLLs (0x136xxx) from oracle data.
    ///
    /// This is the "crack in the wall" — these registers are in an
    /// always-on power domain and can be written even when PCLOCK is gated.
    /// Setting correct PLL frequencies may cause downstream clock gates to open.
    pub fn program_root_plls(&mut self) -> (usize, usize) {
        let root_plls = self.oracle.root_pll_registers();
        self.log.push(format!(
            "Digital PMU: programming {} root PLL registers from oracle",
            root_plls.len(),
        ));

        let mut monitor = PriBusMonitor::new(self.bar0);
        let mut applied = 0;
        let mut skipped = 0;

        for (off, oracle_val) in &root_plls {
            let cold_val = monitor.read_u32(*off);
            if cold_val == *oracle_val {
                continue;
            }

            match monitor.write_u32(*off, *oracle_val) {
                WriteOutcome::Applied | WriteOutcome::AppliedButFaulted => {
                    applied += 1;
                }
                _ => {
                    skipped += 1;
                }
            }
        }

        // After root PLL programming, wait for PLLs to lock
        std::thread::sleep(std::time::Duration::from_millis(50));

        // Check if PCLOCK responded
        let pclock_ctl = monitor.read_u32(0x137000);
        let pclock_ok = !pri::is_pri_error(pclock_ctl);
        self.log.push(format!(
            "  Root PLLs: {applied} applied, {skipped} skipped. PCLOCK responds: {pclock_ok} ({pclock_ctl:#010x})"
        ));

        (applied, skipped)
    }

    /// Program PCLOCK bypass and enable registers specifically.
    ///
    /// These three registers were found writable even on a cold card:
    /// - 0x137020: PCLOCK_BYPASS
    /// - 0x137050: NVPLL_CTL
    /// - 0x137100: MEMPLL_CTL
    pub fn program_pclock_bypass(&mut self) -> Vec<String> {
        let targets = [
            (0x137020, "PCLOCK_BYPASS"),
            (0x137050, "NVPLL_CTL"),
            (0x137054, "NVPLL_COEFF"),
            (0x137100, "MEMPLL_CTL"),
            (0x137104, "MEMPLL_COEFF"),
        ];

        let mut log = Vec::new();

        for (off, name) in targets {
            if let Some(&oracle_val) = self.oracle.registers.get(&off) {
                let cold_val = self.bar0.read_u32(off).unwrap_or(0xDEAD_DEAD);
                let _ = self.bar0.write_u32(off, oracle_val);
                std::thread::sleep(std::time::Duration::from_millis(5));
                let rb = self.bar0.read_u32(off).unwrap_or(0xDEAD_DEAD);
                log.push(format!(
                    "  {name} [{off:#08x}]: cold={cold_val:#010x} oracle={oracle_val:#010x} readback={rb:#010x}"
                ));
            } else {
                log.push(format!("  {name} [{off:#08x}]: no oracle data"));
            }
        }

        log
    }

    fn check_vram(&self) -> bool {
        use crate::vfio::memory::PraminRegion;
        if let Ok(mut region) = PraminRegion::new(self.bar0, 0x0002_6000, 8) {
            region.probe_sentinel(0, 0xCAFE_DEAD).is_working()
        } else {
            false
        }
    }

    /// Consume and return log messages.
    pub fn take_log(&mut self) -> Vec<String> {
        std::mem::take(&mut self.log)
    }
}
