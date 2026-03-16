// SPDX-License-Identifier: AGPL-3.0-only
#![allow(missing_docs)]
//! BAR0 register space cartography — systematic scanning and classification.
//!
//! Scans the entire BAR0 MMIO space in 4-byte strides, classifying each
//! register offset as readable, writable, dynamic, dead, or error-producing.
//! Groups contiguous regions with similar behavior into named domains.
//!
//! This is vendor-agnostic: it discovers what's there by probing, then
//! optionally labels regions using a vendor-specific domain map.

use std::collections::BTreeMap;

use crate::vfio::device::MappedBar;

/// Classification of a single register's access behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegisterAccess {
    /// Register reads and writes back successfully.
    ReadWrite,
    /// Register reads a value but writes don't change it.
    ReadOnly,
    /// Register reads zero or a constant; writes may trigger effects.
    WriteOnly,
    /// Writing changes behavior (interrupt clears, triggers, etc.).
    Trigger,
    /// Register returns an error pattern (0xFFFFFFFF, 0xBADFxxxx, etc.).
    Dead,
}

/// Pattern observed in a register's read value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegisterPattern {
    /// Register always returns the same non-zero value.
    Constant(u32),
    /// Register value changes between reads (counters, timers, status).
    Dynamic,
    /// Register returns an error signature (PRI timeout, dead device).
    ErrorPattern(u32),
    /// Register returns zero.
    Zeros,
}

/// A contiguous region of registers with similar behavior.
#[derive(Debug, Clone)]
pub struct RegisterRegion {
    /// Start offset in BAR space.
    pub start: usize,
    /// End offset (exclusive) in BAR space.
    pub end: usize,
    /// Human-readable domain name (e.g., "PMC", "PFIFO", "PFB").
    pub name: Option<String>,
    /// Predominant access type in this region.
    pub access: RegisterAccess,
    /// Predominant value pattern.
    pub pattern: RegisterPattern,
    /// Number of responsive (non-dead) registers in this region.
    pub responsive_count: usize,
    /// Number of dead/error registers.
    pub dead_count: usize,
}

/// Complete BAR0 scan result.
#[derive(Debug, Clone)]
pub struct BarMap {
    /// Which BAR was scanned (typically 0).
    pub bar_index: u8,
    /// Total BAR size in bytes.
    pub size: usize,
    /// Discovered register regions.
    pub regions: Vec<RegisterRegion>,
    /// Total responsive bytes (non-error, non-dead).
    pub responsive_bytes: usize,
    /// Total error/dead bytes.
    pub error_bytes: usize,
    /// Per-offset classification for detailed queries.
    pub register_map: BTreeMap<usize, RegisterProbe>,
}

/// Result of probing a single register offset.
#[derive(Debug, Clone, Copy)]
pub struct RegisterProbe {
    /// BAR offset.
    pub offset: usize,
    /// Value read on first access.
    pub read1: u32,
    /// Value read on second access (for dynamic detection).
    pub read2: u32,
    /// Whether write-readback succeeded (if safe to test).
    pub writable: Option<bool>,
    /// Classified access type.
    pub access: RegisterAccess,
    /// Classified value pattern.
    pub pattern: RegisterPattern,
}

/// Known domain map entry for labeling discovered regions.
pub struct DomainHint {
    pub start: usize,
    pub end: usize,
    pub name: &'static str,
}

/// Scan BAR0 register space and classify every 4-byte offset.
///
/// `scan_size` limits how much of BAR0 to probe (the full 16MB BAR0 on
/// NVIDIA GPUs would take ~4M reads, so smaller scans are useful for
/// quick probing).
///
/// `safe_write_test` enables write-readback testing on registers that don't
/// look like they'll trigger side effects. Defaults to false for safety.
pub fn scan_bar0(
    bar0: &MappedBar,
    scan_size: usize,
    safe_write_test: bool,
    domain_hints: &[DomainHint],
) -> BarMap {
    let mut register_map = BTreeMap::new();
    let mut responsive_bytes = 0usize;
    let mut error_bytes = 0usize;

    let end = scan_size.min(16 * 1024 * 1024);

    for offset in (0..end).step_by(4) {
        let r1 = bar0.read_u32(offset).unwrap_or(0xDEAD_DEAD);

        // Detect error patterns
        let is_error = r1 == 0xFFFF_FFFF
            || (r1 & 0xFFFF_0000) == 0xBADF_0000
            || (r1 & 0xFFFF_0000) == 0xBAD0_0000
            || r1 == 0xDEAD_DEAD;

        if is_error {
            error_bytes += 4;
            register_map.insert(
                offset,
                RegisterProbe {
                    offset,
                    read1: r1,
                    read2: r1,
                    writable: None,
                    access: RegisterAccess::Dead,
                    pattern: RegisterPattern::ErrorPattern(r1),
                },
            );
            continue;
        }

        responsive_bytes += 4;

        // Second read for dynamic detection
        let r2 = bar0.read_u32(offset).unwrap_or(0xDEAD_DEAD);

        let pattern = if r1 == 0 && r2 == 0 {
            RegisterPattern::Zeros
        } else if r1 != r2 {
            RegisterPattern::Dynamic
        } else {
            RegisterPattern::Constant(r1)
        };

        // Optional write-readback test (only on non-dangerous offsets)
        let writable = if safe_write_test && !is_dangerous_offset(offset) && r1 == r2 {
            let saved = r1;
            let test_val = saved ^ 0x0000_0001;
            let _ = bar0.write_u32(offset, test_val);
            let rb = bar0.read_u32(offset).unwrap_or(saved);
            let _ = bar0.write_u32(offset, saved);
            Some(rb == test_val)
        } else {
            None
        };

        let access = match (pattern, writable) {
            (RegisterPattern::Dynamic, _) => RegisterAccess::ReadOnly,
            (_, Some(true)) => RegisterAccess::ReadWrite,
            (_, Some(false)) => RegisterAccess::ReadOnly,
            (RegisterPattern::Zeros, _) => RegisterAccess::WriteOnly,
            _ => RegisterAccess::ReadOnly,
        };

        register_map.insert(
            offset,
            RegisterProbe {
                offset,
                read1: r1,
                read2: r2,
                writable,
                access,
                pattern,
            },
        );
    }

    // Group into regions
    let regions = group_into_regions(&register_map, domain_hints);

    BarMap {
        bar_index: 0,
        size: end,
        regions,
        responsive_bytes,
        error_bytes,
        register_map,
    }
}

/// Quick scan of specific register ranges rather than the full BAR.
///
/// Much faster — only probes the ranges provided, which is useful for
/// targeted domain analysis.
pub fn scan_ranges(bar0: &MappedBar, ranges: &[(&str, usize, usize)]) -> BarMap {
    let mut register_map = BTreeMap::new();
    let mut responsive_bytes = 0usize;
    let mut error_bytes = 0usize;
    let mut regions = Vec::new();

    for &(name, start, end) in ranges {
        let mut region_responsive = 0;
        let mut region_dead = 0;

        for offset in (start..end).step_by(4) {
            let r1 = bar0.read_u32(offset).unwrap_or(0xDEAD_DEAD);
            let is_error = r1 == 0xFFFF_FFFF
                || (r1 & 0xFFFF_0000) == 0xBADF_0000
                || (r1 & 0xFFFF_0000) == 0xBAD0_0000
                || r1 == 0xDEAD_DEAD;

            if is_error {
                error_bytes += 4;
                region_dead += 1;
                register_map.insert(
                    offset,
                    RegisterProbe {
                        offset,
                        read1: r1,
                        read2: r1,
                        writable: None,
                        access: RegisterAccess::Dead,
                        pattern: RegisterPattern::ErrorPattern(r1),
                    },
                );
            } else {
                responsive_bytes += 4;
                region_responsive += 1;
                let r2 = bar0.read_u32(offset).unwrap_or(0xDEAD_DEAD);
                let pattern = if r1 == 0 && r2 == 0 {
                    RegisterPattern::Zeros
                } else if r1 != r2 {
                    RegisterPattern::Dynamic
                } else {
                    RegisterPattern::Constant(r1)
                };
                let access = match pattern {
                    RegisterPattern::Dynamic => RegisterAccess::ReadOnly,
                    RegisterPattern::Zeros => RegisterAccess::WriteOnly,
                    _ => RegisterAccess::ReadOnly,
                };
                register_map.insert(
                    offset,
                    RegisterProbe {
                        offset,
                        read1: r1,
                        read2: r2,
                        writable: None,
                        access,
                        pattern,
                    },
                );
            }
        }

        let predominant_access = if region_responsive > region_dead {
            RegisterAccess::ReadOnly
        } else {
            RegisterAccess::Dead
        };
        let predominant_pattern = if region_dead > region_responsive {
            RegisterPattern::ErrorPattern(0xBADF_0000)
        } else {
            RegisterPattern::Constant(0)
        };

        regions.push(RegisterRegion {
            start,
            end,
            name: Some(name.to_string()),
            access: predominant_access,
            pattern: predominant_pattern,
            responsive_count: region_responsive,
            dead_count: region_dead,
        });
    }

    BarMap {
        bar_index: 0,
        size: regions.iter().map(|r| r.end).max().unwrap_or(0),
        regions,
        responsive_bytes,
        error_bytes,
        register_map,
    }
}

/// Snapshot specific registers and return (offset, value) pairs.
///
/// Useful for before/after comparison across power state transitions.
pub fn snapshot_registers(bar0: &MappedBar, offsets: &[usize]) -> Vec<(usize, u32)> {
    offsets
        .iter()
        .map(|&off| (off, bar0.read_u32(off).unwrap_or(0xDEAD_DEAD)))
        .collect()
}

/// Compare two register snapshots and return deltas.
pub fn diff_snapshots(before: &[(usize, u32)], after: &[(usize, u32)]) -> Vec<(usize, u32, u32)> {
    before
        .iter()
        .zip(after.iter())
        .filter(|((o1, v1), (o2, v2))| o1 == o2 && v1 != v2)
        .map(|((off, v_before), (_, v_after))| (*off, *v_before, *v_after))
        .collect()
}

impl BarMap {
    /// Print a human-readable summary.
    pub fn print_summary(&self) {
        let total = self.responsive_bytes + self.error_bytes;
        let pct = if total > 0 {
            (self.responsive_bytes as f64 / total as f64) * 100.0
        } else {
            0.0
        };
        eprintln!(
            "╠══ BAR{} CARTOGRAPHY ═══════════════════════════════════════╣",
            self.bar_index
        );
        eprintln!(
            "║ Scanned: {} KB | Responsive: {} KB ({pct:.1}%) | Dead: {} KB",
            total / 1024,
            self.responsive_bytes / 1024,
            self.error_bytes / 1024,
        );
        eprintln!("║ Regions: {}", self.regions.len());
        for region in &self.regions {
            let name = region.name.as_deref().unwrap_or("???");
            eprintln!(
                "║   {name:<16} {:#08x}–{:#08x} ({} regs) alive={} dead={} {:?}",
                region.start,
                region.end,
                (region.end - region.start) / 4,
                region.responsive_count,
                region.dead_count,
                region.access,
            );
        }
    }

    /// Export as a serializable map for JSON persistence.
    pub fn to_json_value(&self) -> serde_json::Value {
        use serde_json::json;
        let regions: Vec<serde_json::Value> = self
            .regions
            .iter()
            .map(|r| {
                json!({
                    "start": format!("{:#x}", r.start),
                    "end": format!("{:#x}", r.end),
                    "name": r.name,
                    "responsive": r.responsive_count,
                    "dead": r.dead_count,
                    "access": format!("{:?}", r.access),
                })
            })
            .collect();
        json!({
            "bar_index": self.bar_index,
            "size": self.size,
            "responsive_bytes": self.responsive_bytes,
            "error_bytes": self.error_bytes,
            "region_count": self.regions.len(),
            "regions": regions,
        })
    }
}

/// Difference between two BarMap scans (e.g., cold vs warm).
#[derive(Debug, Clone)]
pub struct BarMapDiff {
    /// Registers that were dead in `before` but alive in `after`.
    pub woke_up: Vec<(usize, u32)>,
    /// Registers that were alive in `before` but dead in `after`.
    pub went_dead: Vec<(usize, u32)>,
    /// Registers alive in both but with different values.
    pub value_changed: Vec<(usize, u32, u32)>,
    /// Registers alive in both with same values.
    pub unchanged: usize,
}

impl BarMapDiff {
    /// Print a human-readable summary.
    pub fn print_summary(&self) {
        eprintln!("╠══ BAR MAP DIFF ════════════════════════════════════════════╣");
        eprintln!(
            "║ Woke up:       {} registers (dead → alive)",
            self.woke_up.len()
        );
        eprintln!(
            "║ Went dead:     {} registers (alive → dead)",
            self.went_dead.len()
        );
        eprintln!("║ Value changed: {} registers", self.value_changed.len());
        eprintln!("║ Unchanged:     {} registers", self.unchanged);
        if !self.woke_up.is_empty() {
            eprintln!("║ ─── Woke up (first 20) ───");
            for &(off, val) in self.woke_up.iter().take(20) {
                eprintln!("║   [{off:#08x}] → {val:#010x}");
            }
        }
        if !self.value_changed.is_empty() {
            eprintln!("║ ─── Changed (first 20) ───");
            for &(off, before, after) in self.value_changed.iter().take(20) {
                eprintln!("║   [{off:#08x}] {before:#010x} → {after:#010x}");
            }
        }
    }

    /// Export as JSON.
    pub fn to_json_value(&self) -> serde_json::Value {
        use serde_json::json;
        json!({
            "woke_up": self.woke_up.len(),
            "went_dead": self.went_dead.len(),
            "value_changed": self.value_changed.len(),
            "unchanged": self.unchanged,
            "woke_up_registers": self.woke_up.iter().map(|(o, v)| json!({
                "offset": format!("{o:#x}"),
                "value": format!("{v:#010x}"),
            })).collect::<Vec<_>>(),
            "value_changed_registers": self.value_changed.iter().map(|(o, b, a)| json!({
                "offset": format!("{o:#x}"),
                "before": format!("{b:#010x}"),
                "after": format!("{a:#010x}"),
            })).collect::<Vec<_>>(),
        })
    }
}

/// Diff two BarMap scans to discover what changes between states.
pub fn diff_bar_maps(before: &BarMap, after: &BarMap) -> BarMapDiff {
    let mut woke_up = Vec::new();
    let mut went_dead = Vec::new();
    let mut value_changed = Vec::new();
    let mut unchanged = 0usize;

    for (&offset, after_probe) in &after.register_map {
        let before_probe = before.register_map.get(&offset);
        let before_dead = before_probe.is_none_or(|bp| bp.access == RegisterAccess::Dead);
        let after_dead = after_probe.access == RegisterAccess::Dead;

        match (before_dead, after_dead) {
            (true, false) => {
                woke_up.push((offset, after_probe.read1));
            }
            (false, true) => {
                went_dead.push((offset, before_probe.map_or(0, |bp| bp.read1)));
            }
            (false, false) => {
                let bp = before_probe.unwrap();
                if bp.read1 != after_probe.read1 {
                    value_changed.push((offset, bp.read1, after_probe.read1));
                } else {
                    unchanged += 1;
                }
            }
            (true, true) => {} // both dead, ignore
        }
    }

    BarMapDiff {
        woke_up,
        went_dead,
        value_changed,
        unchanged,
    }
}

// ── Internal helpers ──────────────────────────────────────────────────

fn is_dangerous_offset(offset: usize) -> bool {
    matches!(
        offset,
        0x0000_9000..=0x0009_00FF   // PTIMER
        | 0x0010_0CBC              // MMU invalidation
        | 0x0010_0CB8              // MMU invalidation PDB
        | 0x0010_0CEC              // MMU invalidation PDB HI
        | 0x0010_0E24..=0x0010_0E54 // Fault buffer registers
        | 0x0010_A040..=0x0010_A048 // PMU mailboxes
        | 0x0010_A100              // PMU CPUCTL
        | 0x0061_0000..=0x0061_0FFF // PDISP
        | 0x0000_2200              // PFIFO_ENABLE
        | 0x0000_0200              // PMC_ENABLE
    )
}

fn group_into_regions(
    register_map: &BTreeMap<usize, RegisterProbe>,
    domain_hints: &[DomainHint],
) -> Vec<RegisterRegion> {
    if register_map.is_empty() {
        return Vec::new();
    }

    let mut regions = Vec::new();
    let mut current_start: Option<usize> = None;
    let mut current_access = RegisterAccess::Dead;
    let mut responsive = 0usize;
    let mut dead = 0usize;
    let mut prev_offset: Option<usize> = None;

    for (&offset, probe) in register_map {
        let is_contiguous = prev_offset.is_none_or(|p| offset == p + 4);
        let same_type = current_start.is_some() && probe.access == current_access && is_contiguous;

        if !same_type {
            if let Some(start) = current_start {
                let end = prev_offset.unwrap_or(start) + 4;
                let name = find_domain_name(start, domain_hints);
                regions.push(RegisterRegion {
                    start,
                    end,
                    name,
                    access: current_access,
                    pattern: RegisterPattern::Constant(0),
                    responsive_count: responsive,
                    dead_count: dead,
                });
            }
            current_start = Some(offset);
            current_access = probe.access;
            responsive = 0;
            dead = 0;
        }

        if probe.access == RegisterAccess::Dead {
            dead += 1;
        } else {
            responsive += 1;
        }
        prev_offset = Some(offset);
    }

    // Flush last region
    if let Some(start) = current_start {
        let end = prev_offset.unwrap_or(start) + 4;
        let name = find_domain_name(start, domain_hints);
        regions.push(RegisterRegion {
            start,
            end,
            name,
            access: current_access,
            pattern: RegisterPattern::Constant(0),
            responsive_count: responsive,
            dead_count: dead,
        });
    }

    regions
}

fn find_domain_name(offset: usize, hints: &[DomainHint]) -> Option<String> {
    hints
        .iter()
        .find(|h| offset >= h.start && offset < h.end)
        .map(|h| h.name.to_string())
}
