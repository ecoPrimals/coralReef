// SPDX-License-Identifier: AGPL-3.0-or-later

//! SEC2 TRACEPC buffer and unified exit diagnostics.

use crate::vfio::channel::registers::falcon;
use crate::vfio::device::MappedBar;

use super::emem;

/// Read SEC2 TRACEPC circular buffer via indexed `EXCI`/`TRACEPC` registers.
///
/// The falcon TRACEPC buffer stores recent PC values. The count lives in
/// `EXCI[23:16]` (upper byte of the index field). To read entry `i`, write
/// `i` to `EXCI` and read `TRACEPC`.
///
/// Returns `(entry_count, entries)`.
pub fn sec2_tracepc_dump(bar0: &MappedBar) -> (u32, Vec<u32>) {
    let base = falcon::SEC2_BASE;
    let r = |off: usize| bar0.read_u32(base + off).unwrap_or(0xDEAD_DEAD);
    let w = |off: usize, val: u32| {
        let _ = bar0.write_u32(base + off, val);
    };

    let tidx = r(falcon::EXCI);
    let count = ((tidx & 0x00FF_0000) >> 16).min(32);

    let entries: Vec<u32> = (0..count)
        .map(|i| {
            w(falcon::EXCI, i);
            r(falcon::TRACEPC)
        })
        .collect();

    (count, entries)
}

/// Unified SEC2 exit diagnostics — captures SCTL, EMEM, TRACEPC, and EXCI.
///
/// Called from `sec2_queue::probe_and_bootstrap` so all 13 strategy
/// exits get the same diagnostic data for cross-strategy comparison.
pub fn sec2_exit_diagnostics(bar0: &MappedBar, notes: &mut Vec<String>) {
    let base = falcon::SEC2_BASE;
    let r = |off: usize| bar0.read_u32(base + off).unwrap_or(0xDEAD_DEAD);

    let sctl = r(falcon::SCTL);
    let hs_mode = sctl & 0x02 != 0;
    let exci = r(falcon::EXCI);
    let pc = r(falcon::PC);
    notes.push(format!(
        "Exit diag: SCTL={sctl:#010x} HS={hs_mode} EXCI={exci:#010x} PC={pc:#06x}"
    ));

    let (trace_count, traces) = sec2_tracepc_dump(bar0);
    if trace_count > 0 {
        let trace_str: Vec<String> = traces.iter().map(|t| format!("{t:#06x}")).collect();
        notes.push(format!(
            "TRACEPC[0..{trace_count}]: {}",
            trace_str.join(" ")
        ));
    }

    let emem = emem::sec2_emem_read(bar0, 0, 256);
    let nz_emem: Vec<String> = emem
        .iter()
        .enumerate()
        .filter(|&(_, &w)| w != 0 && w != 0xDEAD_DEAD)
        .take(24)
        .map(|(i, &w)| format!("[{:#05x}]={w:#010x}", i * 4))
        .collect();
    if !nz_emem.is_empty() {
        notes.push(format!("EMEM(64w): {}", nz_emem.join(" ")));
    }
}
