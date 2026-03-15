// SPDX-License-Identifier: AGPL-3.0-only
//! PRI Bus Monitor — backpressure sensor for sovereign GPU register access.
//!
//! NVIDIA GPUs route all BAR0 register access through the PRI (Primary Register
//! Interface) bus. When a target domain is clock-gated, power-gated, or otherwise
//! unresponsive, reads return `0xBADxxxxx` sentinel values and the bus can enter
//! a faulted state where *all* subsequent accesses fail.
//!
//! This module provides:
//! - **Fault detection**: recognize PRI error patterns in read values
//! - **Domain health tracking**: maintain a per-domain alive/faulted map
//! - **Bus recovery**: clear PRI faults via PRIV_RING interrupt ack
//! - **Adaptive write pacing**: slow down when faults accumulate
//!
//! The metaphor is a diesel glowplug's temperature sensor — if the cylinder
//! isn't warm enough, the glowplug holds off rather than flooding with fuel.

use std::collections::HashMap;
use crate::vfio::device::MappedBar;
use super::registers::pri;

/// Health state of a PRI bus domain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DomainHealth {
    /// Domain responds normally to reads.
    Alive,
    /// Domain returned PRI error patterns. Writes will be deferred.
    Faulted {
        /// Number of consecutive faults seen.
        fault_count: u32,
        /// Last error value returned.
        last_error: u32,
    },
    /// Domain was explicitly skipped (known to be unreachable).
    Skipped,
}

/// Outcome of a single register write through the monitor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteOutcome {
    /// Write applied, readback matched or was clean.
    Applied,
    /// Write skipped because the domain is faulted.
    SkippedFaulted,
    /// Write applied but readback showed PRI error (bus may be locking up).
    AppliedButFaulted,
    /// Write skipped because backpressure threshold exceeded.
    Throttled,
}

/// Accumulated PRI bus statistics.
#[derive(Debug, Clone, Default)]
pub struct PriStats {
    pub reads_total: u64,
    pub reads_faulted: u64,
    pub writes_total: u64,
    pub writes_applied: u64,
    pub writes_skipped_faulted: u64,
    pub writes_throttled: u64,
    pub bus_recoveries: u32,
    pub domains_faulted: Vec<String>,
}

/// PRI bus backpressure sensor and domain health tracker.
///
/// Wraps a `MappedBar` reference and intercepts all reads/writes with
/// fault detection. When too many faults accumulate, it pauses writes
/// and attempts bus recovery before continuing.
pub struct PriBusMonitor<'a> {
    bar0: &'a MappedBar,
    domain_health: HashMap<&'static str, DomainHealth>,
    stats: PriStats,
    /// Consecutive faults without a successful read — triggers recovery.
    consecutive_faults: u32,
    /// Maximum consecutive faults before attempting bus recovery.
    fault_threshold: u32,
    /// Delay (ms) inserted after recovery attempt.
    recovery_delay_ms: u64,
    /// If true, writes to faulted domains are skipped entirely.
    skip_faulted_domains: bool,
}

impl<'a> PriBusMonitor<'a> {
    pub fn new(bar0: &'a MappedBar) -> Self {
        Self {
            bar0,
            domain_health: HashMap::new(),
            stats: PriStats::default(),
            consecutive_faults: 0,
            fault_threshold: 5,
            recovery_delay_ms: 10,
            skip_faulted_domains: true,
        }
    }

    /// Tune the fault threshold before recovery is attempted.
    pub fn with_fault_threshold(mut self, threshold: u32) -> Self {
        self.fault_threshold = threshold;
        self
    }

    /// Disable faulted-domain skipping (write anyway, report faults).
    pub fn with_force_writes(mut self) -> Self {
        self.skip_faulted_domains = false;
        self
    }

    /// Read a 32-bit register with PRI fault detection.
    ///
    /// Returns the value and updates domain health tracking.
    /// On fault, increments counters and may trigger bus recovery.
    pub fn read_u32(&mut self, offset: usize) -> u32 {
        self.stats.reads_total += 1;
        let val = self.bar0.read_u32(offset).unwrap_or(0xDEAD_DEAD);

        if pri::is_pri_error(val) {
            self.stats.reads_faulted += 1;
            self.consecutive_faults += 1;
            let domain = pri::domain_name(offset);
            self.mark_domain_faulted(domain, val);

            if self.consecutive_faults >= self.fault_threshold {
                self.attempt_recovery();
            }
        } else {
            self.consecutive_faults = 0;
            let domain = pri::domain_name(offset);
            if self.domain_health.get(domain) != Some(&DomainHealth::Alive) {
                self.domain_health.insert(domain, DomainHealth::Alive);
            }
        }

        val
    }

    /// Write a 32-bit register with backpressure awareness.
    ///
    /// Checks domain health before writing. If the domain is faulted and
    /// `skip_faulted_domains` is true, the write is deferred.
    pub fn write_u32(&mut self, offset: usize, value: u32) -> WriteOutcome {
        self.stats.writes_total += 1;
        let domain = pri::domain_name(offset);

        // Check if we should skip this domain
        if self.skip_faulted_domains {
            if let Some(DomainHealth::Faulted { fault_count, .. }) = self.domain_health.get(domain) {
                if *fault_count >= 3 {
                    self.stats.writes_skipped_faulted += 1;
                    return WriteOutcome::SkippedFaulted;
                }
            }
        }

        // Check backpressure threshold
        if self.consecutive_faults >= self.fault_threshold * 2 {
            self.stats.writes_throttled += 1;
            return WriteOutcome::Throttled;
        }

        let _ = self.bar0.write_u32(offset, value);
        self.stats.writes_applied += 1;

        // Readback to check if the write landed
        let readback = self.bar0.read_u32(offset).unwrap_or(0xDEAD_DEAD);
        if pri::is_pri_error(readback) {
            self.consecutive_faults += 1;
            self.mark_domain_faulted(domain, readback);

            if self.consecutive_faults >= self.fault_threshold {
                self.attempt_recovery();
            }

            WriteOutcome::AppliedButFaulted
        } else {
            self.consecutive_faults = 0;
            WriteOutcome::Applied
        }
    }

    /// Monitored read-modify-write: read, apply mask+value, write back.
    pub fn mask_write(&mut self, offset: usize, and_mask: u32, or_value: u32) -> (u32, WriteOutcome) {
        let cur = self.read_u32(offset);
        if pri::is_pri_error(cur) {
            return (cur, WriteOutcome::SkippedFaulted);
        }
        let new_val = (cur & and_mask) | or_value;
        let outcome = self.write_u32(offset, new_val);
        (cur, outcome)
    }

    /// Probe a register domain's health by reading a known register.
    pub fn probe_domain(&mut self, offset: usize) -> DomainHealth {
        let val = self.read_u32(offset);
        let domain = pri::domain_name(offset);
        if pri::is_pri_error(val) {
            DomainHealth::Faulted {
                fault_count: self.domain_health.get(domain)
                    .and_then(|h| match h {
                        DomainHealth::Faulted { fault_count, .. } => Some(*fault_count),
                        _ => None,
                    })
                    .unwrap_or(1),
                last_error: val,
            }
        } else {
            DomainHealth::Alive
        }
    }

    /// Probe all HBM2-critical domains and return a health map.
    pub fn probe_all_domains(&mut self) -> Vec<(&'static str, usize, DomainHealth)> {
        let probes: &[(&str, usize)] = &[
            ("PMC",       0x000200),
            ("PBUS",      0x001200),
            ("PFIFO",     0x002004),
            ("PFB",       0x100000),
            ("FBHUB",     0x100800),
            ("PFB_NISO",  0x100C80),
            ("PMU_FALCON",0x10A000),
            ("LTC",       0x17E200),
            ("FBPA0",     0x9A0000),
            ("FBPA1",     0x9A4000),
            ("FBPA2",     0x9A8000),
            ("FBPA3",     0x9AC000),
            ("PCLOCK",    0x137000),
        ];

        probes.iter().map(|&(name, off)| {
            let health = self.probe_domain(off);
            (name, off, health)
        }).collect()
    }

    /// Probe all domains and return a detailed report with decoded PRI errors.
    pub fn full_diagnostic(&mut self) -> Vec<String> {
        let health = self.probe_all_domains();
        let mut report = Vec::new();

        for (name, off, h) in &health {
            match h {
                DomainHealth::Alive => {
                    let val = self.bar0.read_u32(*off).unwrap_or(0);
                    report.push(format!("{name:14} [{off:#010x}]: ALIVE ({val:#010x})"));
                }
                DomainHealth::Faulted { fault_count, last_error } => {
                    let decoded = pri::decode_pri_error(*last_error);
                    report.push(format!(
                        "{name:14} [{off:#010x}]: FAULTED ({fault_count}x) {last_error:#010x} = {decoded}"
                    ));
                }
                DomainHealth::Skipped => {
                    report.push(format!("{name:14} [{off:#010x}]: SKIPPED"));
                }
            }
        }

        report
    }

    /// Attempt to clear PRI bus faults via PRIV_RING interrupt ack.
    ///
    /// This is the "drain" operation — clear accumulated faults so the
    /// bus can accept new transactions.
    pub fn attempt_recovery(&mut self) -> bool {
        self.stats.bus_recoveries += 1;

        // Read PRIV_RING interrupt status
        let intr_status = self.bar0.read_u32(pri::PRIV_RING_INTR_STATUS).unwrap_or(0);

        // Acknowledge all pending PRIV_RING faults
        if intr_status != 0 {
            let _ = self.bar0.write_u32(pri::PRIV_RING_COMMAND, pri::PRIV_RING_CMD_ACK);
        }

        // Also clear PMC INTR PRIV_RING bit
        let pmc_intr = self.bar0.read_u32(pri::PMC_INTR).unwrap_or(0);
        if pmc_intr & pri::PMC_INTR_PRIV_RING_BIT != 0 {
            let _ = self.bar0.write_u32(pri::PMC_INTR, pri::PMC_INTR_PRIV_RING_BIT);
        }

        std::thread::sleep(std::time::Duration::from_millis(self.recovery_delay_ms));

        // Reset consecutive fault counter
        self.consecutive_faults = 0;

        // Re-probe a known-good register to see if the bus recovered
        let boot0 = self.bar0.read_u32(0).unwrap_or(0xFFFF_FFFF);
        let recovered = !pri::is_pri_error(boot0) && boot0 != 0xFFFF_FFFF;

        if recovered {
            // Clear faulted status for domains that might now respond
            let faulted_domains: Vec<&'static str> = self.domain_health.iter()
                .filter(|(_, h)| matches!(h, DomainHealth::Faulted { .. }))
                .map(|(name, _)| *name)
                .collect();

            for domain in faulted_domains {
                self.domain_health.insert(domain, DomainHealth::Alive);
            }
        }

        recovered
    }

    /// Force-clear all domain health tracking (reset to unknown).
    pub fn reset_health(&mut self) {
        self.domain_health.clear();
        self.consecutive_faults = 0;
    }

    /// Get current domain health map.
    pub fn domain_health(&self) -> &HashMap<&'static str, DomainHealth> {
        &self.domain_health
    }

    /// Get accumulated statistics.
    pub fn stats(&self) -> &PriStats {
        &self.stats
    }

    /// Finalize and return stats + domain health summary.
    pub fn into_report(mut self) -> PriStats {
        self.stats.domains_faulted = self.domain_health.iter()
            .filter(|(_, h)| matches!(h, DomainHealth::Faulted { .. }))
            .map(|(name, h)| match h {
                DomainHealth::Faulted { fault_count, last_error } => {
                    format!("{name}: {fault_count} faults (last={last_error:#010x})")
                }
                _ => name.to_string(),
            })
            .collect();
        self.stats
    }

    /// Access the underlying BAR0 directly (for operations that bypass monitoring).
    pub fn bar0(&self) -> &MappedBar {
        self.bar0
    }

    fn mark_domain_faulted(&mut self, domain: &'static str, error_val: u32) {
        let new_count = match self.domain_health.get(domain) {
            Some(DomainHealth::Faulted { fault_count, .. }) => fault_count + 1,
            _ => 1,
        };
        self.domain_health.insert(domain, DomainHealth::Faulted {
            fault_count: new_count,
            last_error: error_val,
        });
    }
}

impl std::fmt::Debug for PriBusMonitor<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let alive = self.domain_health.values().filter(|h| matches!(h, DomainHealth::Alive)).count();
        let faulted = self.domain_health.values().filter(|h| matches!(h, DomainHealth::Faulted { .. })).count();
        f.debug_struct("PriBusMonitor")
            .field("domains_alive", &alive)
            .field("domains_faulted", &faulted)
            .field("consecutive_faults", &self.consecutive_faults)
            .field("total_reads", &self.stats.reads_total)
            .field("total_writes", &self.stats.writes_total)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pri_error_detection() {
        assert!(pri::is_pri_error(0xBADF1234));
        assert!(pri::is_pri_error(0xBAD0AC01));
        assert!(pri::is_pri_error(0xBAD10000));
        assert!(!pri::is_pri_error(0x12345678));
        assert!(!pri::is_pri_error(0));
        assert!(!pri::is_pri_error(0xFFFF_FFFF));
    }

    #[test]
    fn domain_classification() {
        assert_eq!(pri::domain_name(0x000200), "PMC");
        assert_eq!(pri::domain_name(0x100000), "PFB");
        assert_eq!(pri::domain_name(0x9A0000), "FBPA");
        assert_eq!(pri::domain_name(0x17E000), "LTC");
        assert_eq!(pri::domain_name(0x10A000), "PMU_FALCON");
    }
}
