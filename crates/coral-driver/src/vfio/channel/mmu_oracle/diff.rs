// SPDX-License-Identifier: AGPL-3.0-only
//! Structured comparison of two oracle snapshots.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::capture::{EntryFlags, PageEntry, PageTableDump, decode_entry_addr};

/// Difference between two register values.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterDiff {
    pub name: String,
    pub left: u32,
    pub right: u32,
}

/// Difference between two page table entries at the same position.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntryDiff {
    pub level: String,
    pub index: u32,
    pub left_raw: u64,
    pub right_raw: u64,
    pub addr_match: bool,
    pub flags_match: bool,
    pub left_flags: EntryFlags,
    pub right_flags: EntryFlags,
}

/// Result of comparing two page table dumps.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageTableDiffResult {
    pub left_driver: String,
    pub right_driver: String,
    pub left_bdf: String,
    pub right_bdf: String,
    pub instance_block_diffs: Vec<RegisterDiff>,
    pub entry_diffs: Vec<EntryDiff>,
    pub engine_register_diffs: EngineRegisterDiffs,
    pub summary: DiffSummary,
}

/// Summary statistics for a diff.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffSummary {
    pub total_entries_compared: u32,
    pub entries_matching: u32,
    pub entries_addr_only_diff: u32,
    pub entries_flags_only_diff: u32,
    pub entries_both_diff: u32,
    pub register_diffs: u32,
}

/// Engine register diffs grouped by category.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineRegisterDiffs {
    pub pfifo: Vec<RegisterDiff>,
    pub pmu: Vec<RegisterDiff>,
    pub fecs: Vec<RegisterDiff>,
    pub gpccs: Vec<RegisterDiff>,
    pub sec2: Vec<RegisterDiff>,
    pub mmu: Vec<RegisterDiff>,
    pub misc: Vec<RegisterDiff>,
}

fn diff_register_maps(
    left: &BTreeMap<String, u32>,
    right: &BTreeMap<String, u32>,
) -> Vec<RegisterDiff> {
    let mut diffs = Vec::new();
    for (name, &lval) in left {
        let &rval = right.get(name).unwrap_or(&0xDEAD_DEAD);
        if lval != rval {
            diffs.push(RegisterDiff {
                name: name.clone(),
                left: lval,
                right: rval,
            });
        }
    }
    for (name, &rval) in right {
        if !left.contains_key(name) {
            diffs.push(RegisterDiff {
                name: name.clone(),
                left: 0xDEAD_DEAD,
                right: rval,
            });
        }
    }
    diffs
}

/// Compare two page table dumps and produce a structured diff.
///
/// Compares the first channel from each dump (the primary active channel).
/// Produces entry-level diffs showing address vs flag mismatches, plus
/// engine register diffs for PFIFO, PMU, FECS, GPCCS, SEC2, and MMU.
pub fn diff_page_tables(left: &PageTableDump, right: &PageTableDump) -> PageTableDiffResult {
    let mut entry_diffs = Vec::new();
    let mut instance_block_diffs = Vec::new();
    let mut total = 0u32;
    let mut matching = 0u32;
    let mut addr_only = 0u32;
    let mut flags_only = 0u32;
    let mut both_diff = 0u32;

    if let (Some(lc), Some(rc)) = (left.channels.first(), right.channels.first()) {
        // Instance block comparison
        let li = &lc.info.instance_block;
        let ri = &rc.info.instance_block;
        for &(name, lval, rval) in &[
            ("pdb_lo", li.pdb_lo, ri.pdb_lo),
            ("pdb_hi", li.pdb_hi, ri.pdb_hi),
            ("sc0_pdb_lo", li.sc0_pdb_lo, ri.sc0_pdb_lo),
            ("sc0_pdb_hi", li.sc0_pdb_hi, ri.sc0_pdb_hi),
            ("addr_limit_lo", li.addr_limit_lo, ri.addr_limit_lo),
            ("addr_limit_hi", li.addr_limit_hi, ri.addr_limit_hi),
            ("ramfc_userd_lo", li.ramfc_userd_lo, ri.ramfc_userd_lo),
            ("ramfc_userd_hi", li.ramfc_userd_hi, ri.ramfc_userd_hi),
            ("ramfc_gp_base_lo", li.ramfc_gp_base_lo, ri.ramfc_gp_base_lo),
            ("ramfc_gp_base_hi", li.ramfc_gp_base_hi, ri.ramfc_gp_base_hi),
        ] {
            if lval != rval {
                instance_block_diffs.push(RegisterDiff {
                    name: name.into(),
                    left: lval,
                    right: rval,
                });
            }
        }

        // PD3 entry comparison
        compare_pd_entries(
            "PD3",
            &lc.pd3.entries,
            &rc.pd3.entries,
            &mut entry_diffs,
            &mut total,
            &mut matching,
            &mut addr_only,
            &mut flags_only,
            &mut both_diff,
        );
    }

    let er = &left.engine_registers;
    let rr = &right.engine_registers;
    let engine_register_diffs = EngineRegisterDiffs {
        pfifo: diff_register_maps(&er.pfifo, &rr.pfifo),
        pmu: diff_register_maps(&er.pmu, &rr.pmu),
        fecs: diff_register_maps(&er.fecs, &rr.fecs),
        gpccs: diff_register_maps(&er.gpccs, &rr.gpccs),
        sec2: diff_register_maps(&er.sec2, &rr.sec2),
        mmu: diff_register_maps(&er.mmu, &rr.mmu),
        misc: diff_register_maps(&er.misc, &rr.misc),
    };

    let total_reg_diffs = engine_register_diffs.pfifo.len()
        + engine_register_diffs.pmu.len()
        + engine_register_diffs.fecs.len()
        + engine_register_diffs.gpccs.len()
        + engine_register_diffs.sec2.len()
        + engine_register_diffs.mmu.len()
        + engine_register_diffs.misc.len();

    PageTableDiffResult {
        left_driver: left.driver.clone(),
        right_driver: right.driver.clone(),
        left_bdf: left.bdf.clone(),
        right_bdf: right.bdf.clone(),
        instance_block_diffs,
        entry_diffs,
        engine_register_diffs,
        summary: DiffSummary {
            total_entries_compared: total,
            entries_matching: matching,
            entries_addr_only_diff: addr_only,
            entries_flags_only_diff: flags_only,
            entries_both_diff: both_diff,
            register_diffs: total_reg_diffs as u32,
        },
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "recursive page-table comparison needs all context"
)]
fn compare_pd_entries(
    level: &str,
    left: &[PageEntry],
    right: &[PageEntry],
    diffs: &mut Vec<EntryDiff>,
    total: &mut u32,
    matching: &mut u32,
    addr_only: &mut u32,
    flags_only: &mut u32,
    both_diff: &mut u32,
) {
    let mut left_map: BTreeMap<u32, &PageEntry> = BTreeMap::new();
    for e in left {
        left_map.insert(e.index, e);
    }
    let mut right_map: BTreeMap<u32, &PageEntry> = BTreeMap::new();
    for e in right {
        right_map.insert(e.index, e);
    }

    let all_indices: std::collections::BTreeSet<u32> =
        left_map.keys().chain(right_map.keys()).copied().collect();

    for idx in all_indices {
        let le = left_map.get(&idx).map(|e| e.raw).unwrap_or(0);
        let re = right_map.get(&idx).map(|e| e.raw).unwrap_or(0);
        if le == 0 && re == 0 {
            continue;
        }
        *total += 1;

        let l_addr = decode_entry_addr(le);
        let r_addr = decode_entry_addr(re);
        let l_flags = EntryFlags::decode(le);
        let r_flags = EntryFlags::decode(re);
        let addr_eq = l_addr == r_addr;
        let flags_eq = (le & 0xF) == (re & 0xF);

        if addr_eq && flags_eq {
            *matching += 1;
        } else {
            if !addr_eq && flags_eq {
                *addr_only += 1;
            } else if addr_eq && !flags_eq {
                *flags_only += 1;
            } else {
                *both_diff += 1;
            }
            diffs.push(EntryDiff {
                level: level.into(),
                index: idx,
                left_raw: le,
                right_raw: re,
                addr_match: addr_eq,
                flags_match: flags_eq,
                left_flags: l_flags,
                right_flags: r_flags,
            });
        }
    }
}

/// Print a human-readable diff report to stdout.
pub fn print_diff_report(diff: &PageTableDiffResult) {
    println!("=== Page Table Oracle Diff ===\n");
    println!(
        "Left:  {} on {} (BOOT0 capture)",
        diff.left_driver, diff.left_bdf
    );
    println!("Right: {} on {}\n", diff.right_driver, diff.right_bdf);

    println!("--- Instance Block ---");
    if diff.instance_block_diffs.is_empty() {
        println!("  (identical)\n");
    } else {
        for d in &diff.instance_block_diffs {
            println!("  {}: {:#010x} vs {:#010x}", d.name, d.left, d.right);
        }
        println!();
    }

    println!("--- Page Table Entries ---");
    if diff.entry_diffs.is_empty() {
        println!("  (no differences in populated entries)\n");
    } else {
        for d in &diff.entry_diffs {
            println!(
                "  {}[{}]: {:#018x} vs {:#018x}  addr_match={} flags_match={}",
                d.level, d.index, d.left_raw, d.right_raw, d.addr_match, d.flags_match
            );
            if !d.flags_match {
                println!(
                    "    left:  aper={} vol={}",
                    d.left_flags.aperture_name, d.left_flags.vol
                );
                println!(
                    "    right: aper={} vol={}",
                    d.right_flags.aperture_name, d.right_flags.vol
                );
            }
        }
        println!();
    }

    for (name, diffs) in &[
        ("PFIFO", &diff.engine_register_diffs.pfifo),
        ("PMU", &diff.engine_register_diffs.pmu),
        ("FECS", &diff.engine_register_diffs.fecs),
        ("GPCCS", &diff.engine_register_diffs.gpccs),
        ("SEC2", &diff.engine_register_diffs.sec2),
        ("MMU", &diff.engine_register_diffs.mmu),
        ("MISC", &diff.engine_register_diffs.misc),
    ] {
        if diffs.is_empty() {
            continue;
        }
        println!("--- {name} Register Diffs ---");
        for d in *diffs {
            println!("  {}: {:#010x} vs {:#010x}", d.name, d.left, d.right);
        }
        println!();
    }

    let s = &diff.summary;
    println!("--- Summary ---");
    println!("  PT entries compared: {}", s.total_entries_compared);
    println!("  Matching:            {}", s.entries_matching);
    println!("  Addr-only diff:      {}", s.entries_addr_only_diff);
    println!("  Flags-only diff:     {}", s.entries_flags_only_diff);
    println!("  Both diff:           {}", s.entries_both_diff);
    println!("  Register diffs:      {}", s.register_diffs);
    println!("\n=== End Diff ===");
}
