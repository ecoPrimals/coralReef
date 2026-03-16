// SPDX-License-Identifier: AGPL-3.0-only

/// Register ranges that are meaningful for HBM2/FB initialization.
/// These are the domains we read from an oracle (nouveau-warm) card and
/// apply to a cold VFIO card to replicate the trained memory controller state.
pub(crate) const ORACLE_RANGES: &[(&str, usize, usize)] = &[
    ("PMC", 0x000000, 0x001000),
    ("PBUS", 0x001000, 0x002000),
    ("PTOP", 0x022000, 0x023000),
    ("PFB", 0x100000, 0x102000),
    ("FBPA0", 0x9A0000, 0x9A1000),
    ("FBPA1", 0x9A4000, 0x9A5000),
    ("FBPA_BC", 0x9A8000, 0x9A9000),
    ("LTC", 0x17E000, 0x17F000),
    ("PCLOCK", 0x137000, 0x138000),
    ("PMU", 0x10A000, 0x10B000),
    ("PFB_NISO", 0x100C00, 0x100E00),
    ("PMEM", 0x1FA000, 0x1FB000),
    ("FUSE", 0x021000, 0x022000),
    ("FBHUB", 0x100800, 0x100A00),
    ("PRI_MASTER", 0x122000, 0x123000),
];

/// Registers to NEVER write (triggers, invalidations, dynamic counters).
pub(crate) fn is_dangerous_register(off: usize) -> bool {
    matches!(off,
        0x009000..=0x0090FF |  // PTIMER — dynamic
        0x610000..=0x610FFF |  // PDISP — display engine
        0x100CBC | 0x100CB8 | 0x100CEC |  // MMU invalidation triggers
        0x100E24..=0x100E54 |  // Fault buffer registers
        0x10A040..=0x10A048 |  // PMU mailboxes — dynamic
        0x10A100             // PMU CPUCTL — don't stop the PMU
    )
}
