// SPDX-License-Identifier: AGPL-3.0-or-later
#![expect(missing_docs, reason = "HBM2 oracle capture/replay; full docs planned")]
//! Differential capture/replay harness for oracle-based HBM2 training.

use crate::error::ChannelError;
use crate::vfio::device::MappedBar;
use crate::vfio::memory::{MemoryRegion, PraminRegion};

/// HBM2-critical register domains for ordered capture and replay.
pub const HBM2_CAPTURE_DOMAINS: &[(&str, usize, usize)] = &[
    ("FBPA0", 0x9A0000, 0x9A1000),
    ("FBPA1", 0x9A4000, 0x9A5000),
    ("FBPA2", 0x9A8000, 0x9A9000),
    ("FBPA3", 0x9AC000, 0x9AD000),
    ("LTC0", 0x17E000, 0x180000),
    ("LTC1", 0x180000, 0x182000),
    ("LTC2", 0x182000, 0x184000),
    ("LTC3", 0x184000, 0x186000),
    ("LTC4", 0x186000, 0x188000),
    ("LTC5", 0x188000, 0x18A000),
    ("PCLOCK", 0x137000, 0x138000),
    ("CLK", 0x132000, 0x133000),
    ("PFB", 0x100000, 0x102000),
    ("PFB_NISO", 0x100C00, 0x100E00),
    ("FBHUB", 0x100800, 0x100A00),
    ("PMU", 0x10A000, 0x10B000),
];

/// A captured register state from a domain.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DomainCapture {
    pub name: String,
    pub registers: Vec<(usize, u32)>,
}

/// Complete golden state captured from an oracle card.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GoldenCapture {
    pub boot0: u32,
    pub pmc_enable: u32,
    pub domains: Vec<DomainCapture>,
    pub timestamp: String,
}

impl GoldenCapture {
    /// Flatten all domain registers into a single sorted list.
    pub fn all_registers(&self) -> Vec<(usize, u32)> {
        let mut all: Vec<(usize, u32)> = self
            .domains
            .iter()
            .flat_map(|d| d.registers.iter().copied())
            .collect();
        all.sort_by_key(|(off, _)| *off);
        all
    }

    /// Total register count across all domains.
    pub fn register_count(&self) -> usize {
        self.domains.iter().map(|d| d.registers.len()).sum()
    }
}

/// Capture the golden register state from an oracle card via sysfs BAR0.
pub fn capture_oracle_state(oracle_bdf: &str) -> Result<GoldenCapture, ChannelError> {
    use crate::vfio::sysfs_bar0::{DEFAULT_BAR0_SIZE, SysfsBar0};

    let bar0 = SysfsBar0::open(oracle_bdf, DEFAULT_BAR0_SIZE)?;

    let is_err =
        |v: u32| v == 0xFFFF_FFFF || v == 0xDEAD_DEAD || (v >> 16) == 0xBADF || (v >> 16) == 0xBAD0;

    let boot0 = bar0.read_u32(0);
    if boot0 == 0xFFFF_FFFF {
        return Err(ChannelError::Bar0ReadsAllOnes);
    }

    let pmc_enable = bar0.read_u32(0x200);
    let mut domains = Vec::new();

    for &(name, start, end) in HBM2_CAPTURE_DOMAINS {
        let mut registers = Vec::new();
        for off in (start..end).step_by(4) {
            let val = bar0.read_u32(off);
            if !is_err(val) {
                registers.push((off, val));
            }
        }
        domains.push(DomainCapture {
            name: name.into(),
            registers,
        });
    }

    drop(bar0);

    let total: usize = domains.iter().map(|d| d.registers.len()).sum();
    tracing::debug!(
        "Oracle capture: {total} registers from {} domains (BOOT0={boot0:#010x})",
        domains.len(),
    );

    Ok(GoldenCapture {
        boot0,
        pmc_enable,
        domains,
        timestamp: chrono_timestamp(),
    })
}

/// Compute the diff between an oracle's golden state and the current cold card.
pub fn diff_golden_vs_cold(bar0: &MappedBar, golden: &GoldenCapture) -> Vec<DomainCapture> {
    let r = |off: usize| bar0.read_u32(off).unwrap_or(0xDEAD_DEAD);

    let mut diffs = Vec::new();
    for domain in &golden.domains {
        let mut domain_diffs = Vec::new();
        for &(off, golden_val) in &domain.registers {
            let cold_val = r(off);
            if cold_val != golden_val {
                domain_diffs.push((off, golden_val));
            }
        }
        if !domain_diffs.is_empty() {
            diffs.push(DomainCapture {
                name: domain.name.clone(),
                registers: domain_diffs,
            });
        }
    }

    let total_diffs: usize = diffs.iter().map(|d| d.registers.len()).sum();
    tracing::debug!(
        "Golden diff: {total_diffs} registers differ across {} domains",
        diffs.len(),
    );
    for d in &diffs {
        tracing::debug!("{}: {} diffs", d.name, d.registers.len());
    }

    diffs
}

/// Replay a domain-ordered diff with per-domain VRAM verification and PRI backpressure.
pub fn replay_golden_diff(bar0: &MappedBar, diffs: &[DomainCapture]) -> ReplayResult {
    use super::super::pri_monitor::{PriBusMonitor, WriteOutcome};

    let mut result = ReplayResult::default();
    let mut monitor = PriBusMonitor::new(bar0).with_fault_threshold(5);

    for domain in diffs {
        let first_reg = domain.registers.first().map(|(off, _)| *off).unwrap_or(0);
        let health = monitor.probe_domain(first_reg);
        let domain_faulted = matches!(
            health,
            super::super::pri_monitor::DomainHealth::Faulted { .. }
        );

        if domain_faulted {
            tracing::debug!(
                "Replay: {}: domain faulted, attempting PRI recovery before writes",
                domain.name,
            );
            monitor.attempt_recovery();
        }

        let mut applied = 0;
        let mut skipped = 0;
        for &(off, val) in &domain.registers {
            match monitor.write_u32(off, val) {
                WriteOutcome::Applied => applied += 1,
                WriteOutcome::SkippedFaulted | WriteOutcome::Throttled => skipped += 1,
                WriteOutcome::AppliedButFaulted => {
                    applied += 1;
                    if applied % 10 == 0 {
                        monitor.attempt_recovery();
                    }
                }
            }
        }
        result.domains_applied.push((domain.name.clone(), applied));

        if skipped > 0 {
            tracing::debug!(
                "Replay: {}: {applied} applied, {skipped} PRI-skipped",
                domain.name,
            );
        }

        std::thread::sleep(std::time::Duration::from_millis(50));

        let vram_ok = if let Ok(mut region) = PraminRegion::new(bar0, 0x0002_6000, 8) {
            region.probe_sentinel(0, 0xCAFE_DEAD).is_working()
        } else {
            false
        };

        if vram_ok {
            result.vram_unlocked_after = Some(domain.name.clone());
            result.success = true;
            tracing::debug!("Replay: VRAM alive after {} domain!", domain.name);
            break;
        }

        monitor.attempt_recovery();
    }

    let stats = monitor.into_report();
    tracing::debug!(
        "Replay PRI stats: {} reads ({} faulted), {} writes ({} applied, {} skipped), {} recoveries",
        stats.reads_total,
        stats.reads_faulted,
        stats.writes_total,
        stats.writes_applied,
        stats.writes_skipped_faulted,
        stats.bus_recoveries,
    );

    result
}

/// Result of a golden state replay attempt.
#[derive(Debug, Clone, Default)]
pub struct ReplayResult {
    pub domains_applied: Vec<(String, usize)>,
    pub vram_unlocked_after: Option<String>,
    pub success: bool,
}

/// Perform the complete differential capture → diff → replay pipeline.
pub fn differential_training(
    bar0: &MappedBar,
    oracle_bdf: &str,
) -> Result<ReplayResult, ChannelError> {
    let golden = capture_oracle_state(oracle_bdf)?;

    let target_boot0 = bar0.read_u32(0).unwrap_or(0xDEAD_DEAD);
    if golden.boot0 != target_boot0 {
        return Err(ChannelError::Boot0Mismatch {
            oracle: golden.boot0,
            target: target_boot0,
        });
    }

    let diffs = diff_golden_vs_cold(bar0, &golden);
    if diffs.is_empty() {
        return Ok(ReplayResult {
            domains_applied: vec![],
            vram_unlocked_after: None,
            success: true,
        });
    }

    Ok(replay_golden_diff(bar0, &diffs))
}

fn chrono_timestamp() -> String {
    let now = std::time::SystemTime::now();
    let dur = now
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}s", dur.as_secs())
}
