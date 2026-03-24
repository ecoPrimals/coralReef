// SPDX-License-Identifier: AGPL-3.0-only
//! Driver-agnostic MMU page table oracle for Volta+ (V2 MMU).
//!
//! Captures the full GPU page table hierarchy from any driver state (nouveau,
//! nvidia, vfio-pci, or unbound) via BAR0 PRAMIN window. The oracle walks
//! PD3 → PD2 → PD1 → PD0 → PT and serializes the result as JSON for
//! cross-driver comparison.
//!
//! Also captures key engine registers (PFIFO, PMU, FECS, GPCCS, SEC2) to
//! enable reverse-engineering of firmware initialization sequences.
//!
//! This module supersedes the per-entry capture in `nouveau_oracle.rs` with
//! full-directory scans and structured diff output.

pub mod capture;
pub mod diff;

pub use capture::{
    Bar0Handle, ChannelCapture, ChannelInfo, EngineRegisters, EntryFlags, InstanceBlock,
    PageDirectory, PageEntry, PageTable, PageTableDump, Pd0Directory, Pd0Entry,
    capture_page_tables, capture_page_tables_via_mapped_bar, decode_entry_addr, detect_driver,
};
pub use diff::{
    DiffSummary, EngineRegisterDiffs, EntryDiff, PageTableDiffResult, RegisterDiff,
    diff_page_tables, print_diff_report,
};
