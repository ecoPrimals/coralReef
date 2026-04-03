// SPDX-License-Identifier: AGPL-3.0-only
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

// ── DomainMap trait — architecture-parameterized register classification ──

/// A contiguous BAR0 address range belonging to a named hardware domain.
#[derive(Debug, Clone, Copy)]
pub struct DomainRange {
    pub name: &'static str,
    pub start: usize,
    pub end: usize,
    pub priority: u32,
}

/// Classify BAR0 offsets into named hardware domains with replay priority.
///
/// Each GPU architecture has different register layouts. Kepler's PGRAPH
/// occupies 0x400000..0x420000 while Volta's extends to 0x420000+. By
/// parameterizing classification behind a trait, the replay engine and
/// recipe loaders work identically across architectures — only the
/// domain table changes.
pub trait DomainMap: Send + Sync + std::fmt::Debug {
    fn classify(&self, offset: usize, region_hint: &str) -> (&'static str, u32);
    fn domain_table(&self) -> &[DomainRange];
}

/// Classify an offset using a `DomainRange` table, falling back to "UNKNOWN".
pub fn classify_from_table(table: &[DomainRange], offset: usize) -> (&'static str, u32) {
    for range in table {
        if offset >= range.start && offset < range.end {
            return (range.name, range.priority);
        }
    }
    ("UNKNOWN", 99)
}

// ── Kepler domain map ────────────────────────────────────────────────────

/// Kepler (GK1xx) BAR0 domain layout — includes PTIMER, PPCI, PCOPY
/// region-tag fallbacks for offsets outside known ranges.
#[derive(Debug, Clone, Copy)]
pub struct KeplerDomainMap;

/// Kepler BAR0 domain ranges with replay priority ordering.
pub const KEPLER_DOMAINS: &[DomainRange] = &[
    DomainRange { name: "ROOT_PLL",   start: 0x136000, end: 0x137000, priority: 0 },
    DomainRange { name: "PCLOCK",     start: 0x137000, end: 0x138000, priority: 1 },
    DomainRange { name: "CLK",        start: 0x130000, end: 0x136000, priority: 2 },
    DomainRange { name: "PMC",        start: 0x000000, end: 0x001000, priority: 3 },
    DomainRange { name: "PRI_MASTER", start: 0x122000, end: 0x123000, priority: 4 },
    DomainRange { name: "PBUS",       start: 0x001000, end: 0x002000, priority: 5 },
    DomainRange { name: "PTIMER",     start: 0x009000, end: 0x00A000, priority: 6 },
    DomainRange { name: "PTOP",       start: 0x020000, end: 0x024000, priority: 7 },
    DomainRange { name: "PPCI",       start: 0x008800, end: 0x009000, priority: 8 },
    DomainRange { name: "PFB",        start: 0x100000, end: 0x100800, priority: 10 },
    DomainRange { name: "FBHUB",      start: 0x100800, end: 0x100C00, priority: 11 },
    DomainRange { name: "PFB_NISO",   start: 0x100C00, end: 0x101000, priority: 12 },
    DomainRange { name: "FBPA",       start: 0x9A0000, end: 0x9B0000, priority: 15 },
    DomainRange { name: "LTC",        start: 0x17E000, end: 0x190000, priority: 16 },
    DomainRange { name: "PMU",        start: 0x10A000, end: 0x10C000, priority: 20 },
    DomainRange { name: "PFIFO",      start: 0x002000, end: 0x004000, priority: 25 },
    DomainRange { name: "PBDMA",      start: 0x040000, end: 0x0A0000, priority: 26 },
    DomainRange { name: "PCOPY",      start: 0x104000, end: 0x105000, priority: 27 },
    DomainRange { name: "PGRAPH",     start: 0x400000, end: 0x420000, priority: 30 },
    DomainRange { name: "PCCSR",      start: 0x800000, end: 0x900000, priority: 35 },
    DomainRange { name: "PRAMIN",     start: 0x700000, end: 0x710000, priority: 40 },
];

impl DomainMap for KeplerDomainMap {
    fn classify(&self, offset: usize, region_hint: &str) -> (&'static str, u32) {
        let (name, prio) = classify_from_table(KEPLER_DOMAINS, offset);
        if name != "UNKNOWN" {
            return (name, prio);
        }
        match region_hint {
            "PCLOCK" => ("PCLOCK", 1),
            "PMC" => ("PMC", 3),
            "PTIMER" => ("PTIMER", 6),
            "PFB" => ("PFB", 10),
            "PFIFO" => ("PFIFO", 25),
            "PGRAPH" => ("PGRAPH", 30),
            "PCOPY" => ("PCOPY", 27),
            "PBUS" => ("PBUS", 5),
            "PROM" => ("PROM", 50),
            "PPCI" => ("PPCI", 8),
            _ => ("UNKNOWN", 99),
        }
    }

    fn domain_table(&self) -> &[DomainRange] {
        KEPLER_DOMAINS
    }
}

// ── Volta domain map ─────────────────────────────────────────────────────

/// Volta (GV100) BAR0 domain layout — wider PGRAPH, SEC2 at 0x087000,
/// per-runlist PFIFO, no PMC DEVICE_ENABLE at 0x600.
#[derive(Debug, Clone, Copy)]
pub struct VoltaDomainMap;

/// Volta BAR0 domain ranges with replay priority ordering.
pub const VOLTA_DOMAINS: &[DomainRange] = &[
    DomainRange { name: "ROOT_PLL",   start: 0x136000, end: 0x137000, priority: 0 },
    DomainRange { name: "PCLOCK",     start: 0x137000, end: 0x138000, priority: 1 },
    DomainRange { name: "CLK",        start: 0x130000, end: 0x136000, priority: 2 },
    DomainRange { name: "PMC",        start: 0x000000, end: 0x001000, priority: 3 },
    DomainRange { name: "PRI_MASTER", start: 0x122000, end: 0x123000, priority: 4 },
    DomainRange { name: "PBUS",       start: 0x001000, end: 0x002000, priority: 5 },
    DomainRange { name: "PTIMER",     start: 0x009000, end: 0x00A000, priority: 6 },
    DomainRange { name: "PTOP",       start: 0x020000, end: 0x024000, priority: 7 },
    DomainRange { name: "SEC2",       start: 0x087000, end: 0x088000, priority: 9 },
    DomainRange { name: "PFB",        start: 0x100000, end: 0x102000, priority: 10 },
    DomainRange { name: "FBPA",       start: 0x9A0000, end: 0x9B0000, priority: 15 },
    DomainRange { name: "LTC",        start: 0x17E000, end: 0x190000, priority: 16 },
    DomainRange { name: "PMU",        start: 0x10A000, end: 0x10C000, priority: 20 },
    DomainRange { name: "PFIFO",      start: 0x002000, end: 0x004000, priority: 25 },
    DomainRange { name: "PBDMA",      start: 0x040000, end: 0x0A0000, priority: 26 },
    DomainRange { name: "FECS",       start: 0x409000, end: 0x40A000, priority: 31 },
    DomainRange { name: "GPCCS",      start: 0x41A000, end: 0x41B000, priority: 32 },
    DomainRange { name: "PGRAPH",     start: 0x400000, end: 0x420000, priority: 30 },
    DomainRange { name: "PCCSR",      start: 0x800000, end: 0x900000, priority: 35 },
    DomainRange { name: "PRAMIN",     start: 0x700000, end: 0x710000, priority: 40 },
];

impl DomainMap for VoltaDomainMap {
    fn classify(&self, offset: usize, region_hint: &str) -> (&'static str, u32) {
        let (name, prio) = classify_from_table(VOLTA_DOMAINS, offset);
        if name != "UNKNOWN" {
            return (name, prio);
        }
        match region_hint {
            "PCLOCK" => ("PCLOCK", 1),
            "PMC" => ("PMC", 3),
            "PTIMER" => ("PTIMER", 6),
            "SEC2" => ("SEC2", 9),
            "PFB" => ("PFB", 10),
            "PFIFO" => ("PFIFO", 25),
            "PGRAPH" => ("PGRAPH", 30),
            "PBUS" => ("PBUS", 5),
            _ => ("UNKNOWN", 99),
        }
    }

    fn domain_table(&self) -> &[DomainRange] {
        VOLTA_DOMAINS
    }
}

// ── BootSequence trait — architecture-parameterized boot phase ordering ──

/// A discrete phase in a GPU sovereign boot sequence.
#[derive(Debug, Clone)]
pub struct BootPhase {
    pub name: &'static str,
    /// Minimum recipe priority to include in this phase.
    pub priority_min: u32,
    /// Maximum recipe priority (exclusive) for this phase.
    pub priority_max: u32,
    /// Whether this phase is mandatory for boot to proceed.
    pub required: bool,
}

impl BootPhase {
    /// Filter recipe steps belonging to this phase.
    pub fn filter_steps<'a>(&self, recipe: &'a [RecipeStep]) -> Vec<&'a RecipeStep> {
        recipe
            .iter()
            .filter(|s| s.priority >= self.priority_min && s.priority < self.priority_max)
            .collect()
    }
}

/// Ordered boot sequence for a GPU architecture.
///
/// Each architecture defines its phases (clock init, devinit, falcon boot)
/// and a set of replay hooks for inter-phase hardware polling.
pub trait BootSequence: Send + Sync + std::fmt::Debug {
    fn phases(&self) -> &[BootPhase];
    fn domain_map(&self) -> &dyn DomainMap;
    fn description(&self) -> &str;
}

/// Kepler boot sequence: Clock -> Devinit -> PGRAPH -> FECS PIO.
#[derive(Debug)]
pub struct KeplerBootSequence;

/// Kepler boot phases in dependency order.
pub const KEPLER_PHASES: &[BootPhase] = &[
    BootPhase {
        name: "clock",
        priority_min: 0,
        priority_max: 3,
        required: true,
    },
    BootPhase {
        name: "devinit",
        priority_min: 3,
        priority_max: 30,
        required: true,
    },
    BootPhase {
        name: "pgraph",
        priority_min: 30,
        priority_max: 35,
        required: false,
    },
    BootPhase {
        name: "extended",
        priority_min: 35,
        priority_max: 100,
        required: false,
    },
];

impl BootSequence for KeplerBootSequence {
    fn phases(&self) -> &[BootPhase] {
        KEPLER_PHASES
    }

    fn domain_map(&self) -> &dyn DomainMap {
        &KeplerDomainMap
    }

    fn description(&self) -> &str {
        "Kepler (GK1xx): PLL clock -> devinit -> PGRAPH -> FECS PIO boot"
    }
}

/// Volta boot sequence: (clock probe) -> Devinit -> ACR/SEC2 -> FECS/GPCCS.
///
/// Volta has HS+ firmware security — FECS/GPCCS require signed firmware
/// loaded via the ACR (Authenticated Code Runtime) through SEC2. The clock
/// phase is conditional: if PTIMER is already ticking (warm from previous
/// driver session), clock writes are skipped.
#[derive(Debug)]
pub struct VoltaBootSequence;

/// Volta boot phases in dependency order.
pub const VOLTA_PHASES: &[BootPhase] = &[
    BootPhase {
        name: "clock",
        priority_min: 0,
        priority_max: 3,
        required: false, // Often already alive from prior nvidia session
    },
    BootPhase {
        name: "devinit",
        priority_min: 3,
        priority_max: 30,
        required: true,
    },
    BootPhase {
        name: "pgraph",
        priority_min: 30,
        priority_max: 33,
        required: true,
    },
    BootPhase {
        name: "acr_boot",
        priority_min: 33,
        priority_max: 35,
        required: true,
    },
    BootPhase {
        name: "extended",
        priority_min: 35,
        priority_max: 100,
        required: false,
    },
];

impl BootSequence for VoltaBootSequence {
    fn phases(&self) -> &[BootPhase] {
        VOLTA_PHASES
    }

    fn domain_map(&self) -> &dyn DomainMap {
        &VoltaDomainMap
    }

    fn description(&self) -> &str {
        "Volta (GV1xx): clock probe -> devinit -> PGRAPH -> ACR/SEC2 -> FECS signed boot"
    }
}

// ── Legacy compatibility ─────────────────────────────────────────────────

/// Legacy domain table used by boot_follower (subset of Kepler, no PTIMER/PPCI/PCOPY/PGRAPH).
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

    // ── DomainMap trait tests ───────────────────────────────────────────

    #[test]
    fn kepler_domain_map_classifies_all_known_ranges() {
        let map = KeplerDomainMap;
        assert_eq!(map.classify(0x136400, "").0, "ROOT_PLL");
        assert_eq!(map.classify(0x137020, "").0, "PCLOCK");
        assert_eq!(map.classify(0x132000, "").0, "CLK");
        assert_eq!(map.classify(0x000200, "").0, "PMC");
        assert_eq!(map.classify(0x009400, "").0, "PTIMER");
        assert_eq!(map.classify(0x002504, "").0, "PFIFO");
        assert_eq!(map.classify(0x400100, "").0, "PGRAPH");
    }

    #[test]
    fn kepler_domain_map_falls_back_to_region_hint() {
        let map = KeplerDomainMap;
        assert_eq!(map.classify(0xFFF000, "PCLOCK").0, "PCLOCK");
        assert_eq!(map.classify(0xFFF000, "PROM").0, "PROM");
        assert_eq!(map.classify(0xFFF000, "MYSTERY").0, "UNKNOWN");
    }

    #[test]
    fn kepler_domain_table_is_nonempty() {
        let map = KeplerDomainMap;
        assert!(map.domain_table().len() >= 17);
    }

    #[test]
    fn classify_from_table_returns_unknown_for_gap() {
        let result = classify_from_table(KEPLER_DOMAINS, 0xFFF_FFFF);
        assert_eq!(result.0, "UNKNOWN");
        assert_eq!(result.1, 99);
    }

    // ── BootSequence trait tests ────────────────────────────────────────

    #[test]
    fn kepler_boot_sequence_has_four_phases() {
        let seq = KeplerBootSequence;
        assert_eq!(seq.phases().len(), 4);
        assert_eq!(seq.phases()[0].name, "clock");
        assert_eq!(seq.phases()[1].name, "devinit");
        assert_eq!(seq.phases()[2].name, "pgraph");
        assert_eq!(seq.phases()[3].name, "extended");
    }

    #[test]
    fn boot_phase_filter_steps() {
        let recipe = vec![
            RecipeStep { domain: "ROOT_PLL".into(), offset: 0x136400, value: 0xFF, priority: 0 },
            RecipeStep { domain: "CLK".into(), offset: 0x130000, value: 0x01, priority: 2 },
            RecipeStep { domain: "PMC".into(), offset: 0x000200, value: 0x1100, priority: 3 },
            RecipeStep { domain: "PFIFO".into(), offset: 0x002504, value: 0x01, priority: 25 },
            RecipeStep { domain: "PGRAPH".into(), offset: 0x400100, value: 0x01, priority: 30 },
        ];

        let clock_phase = &KEPLER_PHASES[0];
        let clock_steps = clock_phase.filter_steps(&recipe);
        assert_eq!(clock_steps.len(), 2);
        assert_eq!(clock_steps[0].domain, "ROOT_PLL");
        assert_eq!(clock_steps[1].domain, "CLK");

        let devinit_phase = &KEPLER_PHASES[1];
        let devinit_steps = devinit_phase.filter_steps(&recipe);
        assert_eq!(devinit_steps.len(), 2);
        assert_eq!(devinit_steps[0].domain, "PMC");
        assert_eq!(devinit_steps[1].domain, "PFIFO");

        let pgraph_phase = &KEPLER_PHASES[2];
        let pgraph_steps = pgraph_phase.filter_steps(&recipe);
        assert_eq!(pgraph_steps.len(), 1);
        assert_eq!(pgraph_steps[0].domain, "PGRAPH");
    }

    #[test]
    fn kepler_boot_sequence_is_object_safe() {
        let seq: &dyn BootSequence = &KeplerBootSequence;
        assert!(!seq.description().is_empty());
        assert!(!seq.phases().is_empty());
    }

    // ── Volta tests ─────────────────────────────────────────────────────

    #[test]
    fn volta_domain_map_classifies_all_known_ranges() {
        let map = VoltaDomainMap;
        assert_eq!(map.classify(0x136400, "").0, "ROOT_PLL");
        assert_eq!(map.classify(0x137020, "").0, "PCLOCK");
        assert_eq!(map.classify(0x132000, "").0, "CLK");
        assert_eq!(map.classify(0x000200, "").0, "PMC");
        assert_eq!(map.classify(0x009400, "").0, "PTIMER");
        assert_eq!(map.classify(0x087500, "").0, "SEC2");
        assert_eq!(map.classify(0x409100, "").0, "FECS");
        assert_eq!(map.classify(0x41A100, "").0, "GPCCS");
        assert_eq!(map.classify(0x400100, "").0, "PGRAPH");
    }

    #[test]
    fn volta_domain_map_sec2_at_087000() {
        let map = VoltaDomainMap;
        let (name, prio) = map.classify(0x087000, "");
        assert_eq!(name, "SEC2");
        assert!(prio < 10, "SEC2 should be early in boot");
    }

    #[test]
    fn volta_boot_sequence_has_five_phases() {
        let seq = VoltaBootSequence;
        assert_eq!(seq.phases().len(), 5);
        assert_eq!(seq.phases()[0].name, "clock");
        assert_eq!(seq.phases()[3].name, "acr_boot");
    }

    #[test]
    fn volta_boot_sequence_clock_not_required() {
        let seq = VoltaBootSequence;
        assert!(
            !seq.phases()[0].required,
            "Volta clock phase should be optional (often warm)"
        );
    }

    #[test]
    fn volta_boot_sequence_is_object_safe() {
        let seq: &dyn BootSequence = &VoltaBootSequence;
        assert!(seq.description().contains("Volta"));
        assert!(!seq.phases().is_empty());
    }
}
