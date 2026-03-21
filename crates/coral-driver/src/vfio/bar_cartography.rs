// SPDX-License-Identifier: AGPL-3.0-only
#![expect(missing_docs, reason = "BAR cartography types; full docs planned")]
//! BAR0 register space cartography — systematic scanning and classification.
//!
//! Scans the entire BAR0 MMIO space in 4-byte strides, classifying each
//! register offset as readable, writable, dynamic, dead, or error-producing.
//! Groups contiguous regions with similar behavior into named domains.
//!
//! This is vendor-agnostic: it discovers what's there by probing, then
//! optionally labels regions using a vendor-specific domain map.

use std::collections::BTreeMap;
use std::fmt::Write as FmtWrite;

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
        let mut s = String::new();
        writeln!(
            &mut s,
            "╠══ BAR{} CARTOGRAPHY ═══════════════════════════════════════╣",
            self.bar_index
        )
        .expect("writing to String is infallible");
        writeln!(
            &mut s,
            "║ Scanned: {} KB | Responsive: {} KB ({pct:.1}%) | Dead: {} KB",
            total / 1024,
            self.responsive_bytes / 1024,
            self.error_bytes / 1024,
        )
        .expect("writing to String is infallible");
        writeln!(&mut s, "║ Regions: {}", self.regions.len())
            .expect("writing to String is infallible");
        for region in &self.regions {
            let name = region.name.as_deref().unwrap_or("???");
            writeln!(
                &mut s,
                "║   {name:<16} {:#08x}–{:#08x} ({} regs) alive={} dead={} {:?}",
                region.start,
                region.end,
                (region.end - region.start) / 4,
                region.responsive_count,
                region.dead_count,
                region.access,
            )
            .expect("writing to String is infallible");
        }
        tracing::info!(summary = %s, bar_index = self.bar_index, "BAR cartography");
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
        let mut s = String::new();
        writeln!(
            &mut s,
            "╠══ BAR MAP DIFF ════════════════════════════════════════════╣"
        )
        .expect("writing to String is infallible");
        writeln!(
            &mut s,
            "║ Woke up:       {} registers (dead → alive)",
            self.woke_up.len()
        )
        .expect("writing to String is infallible");
        writeln!(
            &mut s,
            "║ Went dead:     {} registers (alive → dead)",
            self.went_dead.len()
        )
        .expect("writing to String is infallible");
        writeln!(
            &mut s,
            "║ Value changed: {} registers",
            self.value_changed.len()
        )
        .expect("writing to String is infallible");
        writeln!(&mut s, "║ Unchanged:     {} registers", self.unchanged)
            .expect("writing to String is infallible");
        if !self.woke_up.is_empty() {
            writeln!(&mut s, "║ ─── Woke up (first 20) ───")
                .expect("writing to String is infallible");
            for &(off, val) in self.woke_up.iter().take(20) {
                writeln!(&mut s, "║   [{off:#08x}] → {val:#010x}")
                    .expect("writing to String is infallible");
            }
        }
        if !self.value_changed.is_empty() {
            writeln!(&mut s, "║ ─── Changed (first 20) ───")
                .expect("writing to String is infallible");
            for &(off, before, after) in self.value_changed.iter().take(20) {
                writeln!(&mut s, "║   [{off:#08x}] {before:#010x} → {after:#010x}")
                    .expect("writing to String is infallible");
            }
        }
        tracing::info!(summary = %s, "BAR map diff");
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
                let bp = before_probe.expect("before_probe set in preceding branch");
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

/// Exposes [`is_dangerous_offset`] for unit tests (`tests` is a child module).
#[cfg(test)]
pub(crate) fn is_dangerous_offset_for_test(offset: usize) -> bool {
    is_dangerous_offset(offset)
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::BTreeMap;

    #[test]
    fn diff_snapshots_empty() {
        assert!(diff_snapshots(&[], &[]).is_empty());
    }

    #[test]
    fn diff_snapshots_no_changes() {
        let before = [(0x100usize, 1u32), (0x104, 2)];
        let after = [(0x100, 1), (0x104, 2)];
        assert!(diff_snapshots(&before, &after).is_empty());
    }

    #[test]
    fn diff_snapshots_some_changes() {
        let before = [(0x200, 10), (0x204, 20), (0x208, 30)];
        let after = [(0x200, 11), (0x204, 20), (0x208, 31)];
        let d = diff_snapshots(&before, &after);
        assert_eq!(d, vec![(0x200, 10, 11), (0x208, 30, 31)]);
    }

    #[test]
    fn diff_snapshots_all_changes() {
        let before = [(0x0, 0u32), (0x4, 1)];
        let after = [(0x0, 0xFFFF_FFFF), (0x4, 0xDEAD_BEEF)];
        let d = diff_snapshots(&before, &after);
        assert_eq!(d, vec![(0x0, 0, 0xFFFF_FFFF), (0x4, 1, 0xDEAD_BEEF)]);
    }

    #[test]
    fn diff_snapshots_mismatched_offsets_no_delta() {
        let before = [(0x0, 1u32)];
        let after = [(0x4, 2u32)];
        assert!(diff_snapshots(&before, &after).is_empty());
    }

    fn probe_dead(offset: usize, sig: u32) -> RegisterProbe {
        RegisterProbe {
            offset,
            read1: sig,
            read2: sig,
            writable: None,
            access: RegisterAccess::Dead,
            pattern: RegisterPattern::ErrorPattern(sig),
        }
    }

    fn probe_alive(offset: usize, v: u32) -> RegisterProbe {
        RegisterProbe {
            offset,
            read1: v,
            read2: v,
            writable: None,
            access: RegisterAccess::ReadOnly,
            pattern: RegisterPattern::Constant(v),
        }
    }

    fn synthetic_bar_map(register_map: BTreeMap<usize, RegisterProbe>) -> BarMap {
        BarMap {
            bar_index: 0,
            size: register_map
                .keys()
                .next_back()
                .map_or(0, |k| k + 4)
                .min(16 * 1024 * 1024),
            regions: Vec::new(),
            responsive_bytes: register_map
                .values()
                .filter(|p| p.access != RegisterAccess::Dead)
                .count()
                * 4,
            error_bytes: register_map
                .values()
                .filter(|p| p.access == RegisterAccess::Dead)
                .count()
                * 4,
            register_map,
        }
    }

    #[test]
    fn diff_bar_maps_woke_up_and_went_dead() {
        let mut before_map = BTreeMap::new();
        before_map.insert(0x100, probe_dead(0x100, 0xBADF_0000));
        before_map.insert(0x104, probe_alive(0x104, 0xA));

        let mut after_map = BTreeMap::new();
        after_map.insert(0x100, probe_alive(0x100, 0x1));
        after_map.insert(0x104, probe_dead(0x104, 0xFFFF_FFFF));

        let before = synthetic_bar_map(before_map);
        let after = synthetic_bar_map(after_map);
        let d = diff_bar_maps(&before, &after);
        assert_eq!(d.woke_up, vec![(0x100, 0x1)]);
        assert_eq!(d.went_dead, vec![(0x104, 0xA)]);
        assert!(d.value_changed.is_empty());
        assert_eq!(d.unchanged, 0);
    }

    #[test]
    fn diff_bar_maps_value_changed_and_unchanged() {
        let mut before_map = BTreeMap::new();
        before_map.insert(0x200, probe_alive(0x200, 1));
        before_map.insert(0x204, probe_alive(0x204, 2));

        let mut after_map = BTreeMap::new();
        after_map.insert(0x200, probe_alive(0x200, 9));
        after_map.insert(0x204, probe_alive(0x204, 2));

        let before = synthetic_bar_map(before_map);
        let after = synthetic_bar_map(after_map);
        let d = diff_bar_maps(&before, &after);
        assert_eq!(d.value_changed, vec![(0x200, 1, 9)]);
        assert_eq!(d.unchanged, 1);
        assert!(d.woke_up.is_empty());
        assert!(d.went_dead.is_empty());
    }

    #[test]
    fn bar_map_to_json_value_shape() {
        let mut m = BTreeMap::new();
        m.insert(0x0, probe_alive(0x0, 0));
        let map = synthetic_bar_map(m);
        let v = map.to_json_value();
        assert_eq!(v["bar_index"], json!(0));
        assert_eq!(v["responsive_bytes"], json!(4));
        assert_eq!(v["error_bytes"], json!(0));
        assert_eq!(v["region_count"], json!(0));
        assert!(v["regions"].is_array());
    }

    #[test]
    fn bar_map_diff_to_json_value_shape() {
        let d = BarMapDiff {
            woke_up: vec![(0x10, 0x1)],
            went_dead: vec![],
            value_changed: vec![(0x20, 0x2, 0x3)],
            unchanged: 4,
        };
        let v = d.to_json_value();
        assert_eq!(v["woke_up"], json!(1));
        assert_eq!(v["went_dead"], json!(0));
        assert_eq!(v["value_changed"], json!(1));
        assert_eq!(v["unchanged"], json!(4));
        let woke = v["woke_up_registers"].as_array().expect("array");
        assert_eq!(woke[0]["offset"], json!("0x10"));
        let ch = v["value_changed_registers"].as_array().expect("array");
        assert_eq!(ch[0]["before"], json!("0x00000002"));
        assert_eq!(ch[0]["after"], json!("0x00000003"));
    }

    #[test]
    fn is_dangerous_offset_for_test_matches_policy() {
        assert!(super::is_dangerous_offset_for_test(0x0000_0200));
        assert!(super::is_dangerous_offset_for_test(0x0000_2200));
        assert!(super::is_dangerous_offset_for_test(0x0000_9000));
        assert!(super::is_dangerous_offset_for_test(0x0010_A100));
        assert!(super::is_dangerous_offset_for_test(0x0061_0500));
        assert!(!super::is_dangerous_offset_for_test(0x0000_0204));
        // `0x1_0000` is inside the PTIMER guard range (`0x9000..=0x9_00FF`).
        assert!(!super::is_dangerous_offset_for_test(0x0000_0800));
    }

    #[test]
    fn error_signatures_classify_as_dead_like_scan_bar0() {
        let sigs = [0xBADF_1234, 0xBAD0_5678, 0xDEAD_DEAD, 0xFFFF_FFFF];
        for sig in sigs {
            let mut m = BTreeMap::new();
            m.insert(0, probe_dead(0, sig));
            let map = synthetic_bar_map(m);
            assert_eq!(map.register_map[&0].access, RegisterAccess::Dead);
            assert_eq!(map.error_bytes, 4);
        }
    }
}
