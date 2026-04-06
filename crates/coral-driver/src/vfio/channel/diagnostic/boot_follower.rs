// SPDX-License-Identifier: AGPL-3.0-or-later
#![expect(missing_docs, reason = "boot follower types; full docs planned")]
//! Boot sequence follower — diff driver boot sequences in real-time.
//!
//! Parses mmiotrace or oracle BAR0 data and compares it against the diagnostic
//! matrix's register expectations. This lets the matrix "follow" a driver boot
//! and learn the correct init sequence for each domain.
//!
//! ## Modes
//!
//! 1. **Offline diff**: Load an mmiotrace file and a cold-card register snapshot,
//!    produce a domain-ordered diff showing what the driver changed.
//! 2. **Live shadow**: While a driver boots on the oracle card, continuously read
//!    its BAR0 and record register deltas (for real-time following).
//! 3. **Recipe extraction**: Convert diffs into GlowPlug-compatible init recipes
//!    that can be replayed on a cold card.

use std::collections::BTreeMap;
use std::path::Path;

/// A single MMIO write operation parsed from an mmiotrace log.
#[derive(Debug, Clone)]
pub struct MmioWrite {
    pub timestamp_us: u64,
    pub offset: usize,
    pub value: u32,
    pub width: u8,
}

/// A single MMIO read operation parsed from an mmiotrace log.
#[derive(Debug, Clone)]
pub struct MmioRead {
    pub timestamp_us: u64,
    pub offset: usize,
    pub value: u32,
}

/// Parsed mmiotrace boot sequence.
#[derive(Debug, Clone)]
pub struct BootTrace {
    /// All write operations in order.
    pub writes: Vec<MmioWrite>,
    /// All read operations in order.
    pub reads: Vec<MmioRead>,
    /// Driver that produced this trace.
    pub driver: String,
    /// Total duration in microseconds.
    pub duration_us: u64,
}

/// Domain-classified register change.
#[derive(Debug, Clone)]
pub struct DomainDelta {
    pub domain: String,
    pub offset: usize,
    pub cold_value: u32,
    pub warm_value: u32,
}

/// Result of comparing cold vs warm register states.
#[derive(Debug)]
pub struct BootDiff {
    /// Ordered list of all register changes, grouped by domain.
    pub deltas: Vec<DomainDelta>,
    /// Per-domain statistics.
    pub domain_stats: BTreeMap<String, DomainStats>,
    /// Total registers compared.
    pub total_compared: usize,
    /// Total registers that differ.
    pub total_changed: usize,
}

/// Statistics for a single register domain.
#[derive(Debug, Clone, Default)]
pub struct DomainStats {
    pub compared: usize,
    pub changed: usize,
    pub cold_dead: usize,
    pub warm_alive: usize,
}

/// Init recipe step extracted from a boot diff.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RecipeStep {
    pub domain: String,
    pub offset: usize,
    pub value: u32,
    pub priority: u32,
}

/// Domain ranges and their dependency priority (lower = write first).
const DOMAIN_PRIORITY: &[(&str, usize, usize, u32)] = &[
    ("ROOT_PLL", 0x136000, 0x137000, 0),
    ("PCLOCK", 0x137000, 0x138000, 1),
    ("CLK", 0x130000, 0x136000, 2),
    ("PMC", 0x000000, 0x001000, 3),
    ("PRI_MASTER", 0x122000, 0x123000, 4),
    ("PBUS", 0x001000, 0x002000, 5),
    ("PTOP", 0x020000, 0x024000, 6),
    ("PFB", 0x100000, 0x100800, 10),
    ("FBHUB", 0x100800, 0x100C00, 11),
    ("PFB_NISO", 0x100C00, 0x101000, 12),
    ("FBPA", 0x9A0000, 0x9B0000, 15),
    ("LTC", 0x17E000, 0x190000, 16),
    ("PMU", 0x10A000, 0x10C000, 20),
    ("PFIFO", 0x002000, 0x004000, 25),
    ("PBDMA", 0x040000, 0x0A0000, 26),
    ("PCCSR", 0x800000, 0x900000, 30),
    ("PRAMIN", 0x700000, 0x710000, 35),
];

fn classify_domain(offset: usize) -> (&'static str, u32) {
    for &(name, start, end, prio) in DOMAIN_PRIORITY {
        if offset >= start && offset < end {
            return (name, prio);
        }
    }
    ("UNKNOWN", 99)
}

impl BootTrace {
    /// Parse an mmiotrace log file into a boot trace.
    ///
    /// mmiotrace format:
    /// ```text
    /// W 4 0.123456 1 0xfee00000 0x00000001 0x00000000 0x0
    /// R 4 0.123460 1 0xfee00004 0x00000042 0x0
    /// ```
    pub fn from_mmiotrace(path: &Path) -> Result<Self, String> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("cannot read mmiotrace {}: {e}", path.display()))?;

        let mut writes = Vec::new();
        let mut reads = Vec::new();
        let mut first_ts: Option<f64> = None;
        let mut last_ts: f64 = 0.0;
        let mut bar0_base: Option<u64> = None;

        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() < 6 {
                continue;
            }

            // Detect MAP lines to find BAR0 base address
            if parts[0] == "MAP" && parts.len() >= 4 {
                if let Ok(addr) = u64::from_str_radix(parts[2].trim_start_matches("0x"), 16)
                    && bar0_base.is_none()
                {
                    bar0_base = Some(addr);
                }
                continue;
            }

            let kind = parts[0];
            if kind != "W" && kind != "R" {
                continue;
            }

            let width = parts[1].parse::<u8>().unwrap_or(4);
            let ts = parts[2].parse::<f64>().unwrap_or(0.0);
            let addr = u64::from_str_radix(parts[4].trim_start_matches("0x"), 16).unwrap_or(0);
            let value = u32::from_str_radix(parts[5].trim_start_matches("0x"), 16).unwrap_or(0);

            if first_ts.is_none() {
                first_ts = Some(ts);
            }
            last_ts = ts;

            let base = bar0_base.unwrap_or(0);
            let offset = if addr >= base && base > 0 {
                (addr - base) as usize
            } else {
                addr as usize
            };

            // Only include BAR0 range (0-16MB)
            if offset > 0x0100_0000 {
                continue;
            }

            let ts_us = ((ts - first_ts.unwrap_or(0.0)) * 1_000_000.0) as u64;

            match kind {
                "W" => writes.push(MmioWrite {
                    timestamp_us: ts_us,
                    offset,
                    value,
                    width,
                }),
                "R" => reads.push(MmioRead {
                    timestamp_us: ts_us,
                    offset,
                    value,
                }),
                _ => {}
            }
        }

        let duration_us = ((last_ts - first_ts.unwrap_or(0.0)) * 1_000_000.0) as u64;

        Ok(Self {
            writes,
            reads,
            driver: "nouveau".to_string(),
            duration_us,
        })
    }

    /// Extract domain-classified write sequence.
    ///
    /// Returns writes grouped by domain in dependency order,
    /// suitable for replay via the digital PMU.
    pub fn to_recipe(&self) -> Vec<RecipeStep> {
        let mut steps: Vec<RecipeStep> = self
            .writes
            .iter()
            .map(|w| {
                let (domain, priority) = classify_domain(w.offset);
                RecipeStep {
                    domain: domain.to_string(),
                    offset: w.offset,
                    value: w.value,
                    priority,
                }
            })
            .collect();

        // Stable sort by priority (preserves original order within each domain)
        steps.sort_by_key(|s| s.priority);
        steps
    }

    /// Get writes for a specific domain.
    pub fn domain_writes(&self, start: usize, end: usize) -> Vec<&MmioWrite> {
        self.writes
            .iter()
            .filter(|w| w.offset >= start && w.offset < end)
            .collect()
    }

    /// Summary of write counts per domain.
    pub fn domain_summary(&self) -> BTreeMap<String, usize> {
        let mut counts = BTreeMap::new();
        for w in &self.writes {
            let (domain, _) = classify_domain(w.offset);
            *counts.entry(domain.to_string()).or_default() += 1;
        }
        counts
    }
}

impl BootDiff {
    /// Compare an oracle (warm) register snapshot against a cold card's registers.
    ///
    /// The oracle snapshot is a `BTreeMap<usize, u32>` (offset → value),
    /// and the cold snapshot is the same format from the diagnostic matrix.
    pub fn compare(oracle: &BTreeMap<usize, u32>, cold: &BTreeMap<usize, u32>) -> Self {
        let mut deltas = Vec::new();
        let mut domain_stats: BTreeMap<String, DomainStats> = BTreeMap::new();
        let mut total_compared = 0;
        let mut total_changed = 0;

        // Compare all offsets present in either snapshot
        let all_offsets: std::collections::BTreeSet<usize> =
            oracle.keys().chain(cold.keys()).copied().collect();

        for &offset in &all_offsets {
            let warm_val = oracle.get(&offset).copied().unwrap_or(0);
            let cold_val = cold.get(&offset).copied().unwrap_or(0);

            // Skip PRI errors in either snapshot
            if super::super::registers::pri::is_pri_error(warm_val) {
                continue;
            }
            if super::super::registers::pri::is_pri_error(cold_val) && cold_val != 0 {
                let (domain, _) = classify_domain(offset);
                let stats = domain_stats.entry(domain.to_string()).or_default();
                stats.cold_dead += 1;
                if warm_val != 0 {
                    stats.warm_alive += 1;
                    deltas.push(DomainDelta {
                        domain: domain.to_string(),
                        offset,
                        cold_value: cold_val,
                        warm_value: warm_val,
                    });
                    total_changed += 1;
                }
                continue;
            }

            total_compared += 1;
            let (domain, _) = classify_domain(offset);
            let stats = domain_stats.entry(domain.to_string()).or_default();
            stats.compared += 1;

            if warm_val != cold_val {
                stats.changed += 1;
                total_changed += 1;
                deltas.push(DomainDelta {
                    domain: domain.to_string(),
                    offset,
                    cold_value: cold_val,
                    warm_value: warm_val,
                });
            }
        }

        Self {
            deltas,
            domain_stats,
            total_compared,
            total_changed,
        }
    }

    /// Convert the diff into a priority-ordered recipe for GlowPlug replay.
    pub fn to_recipe(&self) -> Vec<RecipeStep> {
        let mut steps: Vec<RecipeStep> = self
            .deltas
            .iter()
            .map(|d| {
                let (_, priority) = classify_domain(d.offset);
                RecipeStep {
                    domain: d.domain.clone(),
                    offset: d.offset,
                    value: d.warm_value,
                    priority,
                }
            })
            .collect();

        steps.sort_by_key(|s| s.priority);
        steps
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_domains() {
        assert_eq!(classify_domain(0x136400).0, "ROOT_PLL");
        assert_eq!(classify_domain(0x137000).0, "PCLOCK");
        assert_eq!(classify_domain(0x000200).0, "PMC");
        assert_eq!(classify_domain(0x9A0000).0, "FBPA");
        assert_eq!(classify_domain(0x17E200).0, "LTC");
        assert_eq!(classify_domain(0x100000).0, "PFB");
    }

    #[test]
    fn boot_diff_detects_changes() {
        let mut oracle = BTreeMap::new();
        let mut cold = BTreeMap::new();

        oracle.insert(0x000200, 0xFFFF_FFFF_u32);
        cold.insert(0x000200, 0x4000_0020_u32);

        oracle.insert(0x137000, 0x0000_0001_u32);
        cold.insert(0x137000, 0xBADF_5040_u32);

        let diff = BootDiff::compare(&oracle, &cold);
        assert_eq!(diff.total_changed, 2);
        assert!(diff.domain_stats.contains_key("PMC"));
    }

    #[test]
    fn recipe_priority_order() {
        let mut oracle = BTreeMap::new();
        let mut cold = BTreeMap::new();

        oracle.insert(0x002200, 0x01_u32); // PFIFO (priority 25)
        cold.insert(0x002200, 0x00_u32);

        oracle.insert(0x136400, 0xFF_u32); // ROOT_PLL (priority 0)
        cold.insert(0x136400, 0x00_u32);

        oracle.insert(0x000200, 0xFF_u32); // PMC (priority 3)
        cold.insert(0x000200, 0x00_u32);

        let diff = BootDiff::compare(&oracle, &cold);
        let recipe = diff.to_recipe();
        assert_eq!(recipe[0].domain, "ROOT_PLL");
        assert_eq!(recipe[1].domain, "PMC");
        assert_eq!(recipe[2].domain, "PFIFO");
    }
}
