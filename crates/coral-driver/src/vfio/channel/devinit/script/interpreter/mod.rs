// SPDX-License-Identifier: AGPL-3.0-or-later
#![expect(missing_docs, reason = "VBIOS script interpreter; full docs planned")]
//! VBIOS init script host-side interpreter — executes opcode stream via BAR0.
//!
//! Reference: nouveau nvkm/subdev/bios/init.c (Ben Skeggs, Red Hat)

mod opcodes;
mod pri;

use crate::error::DevinitError;
use crate::vfio::device::MappedBar;

use super::super::vbios::BitTable;

/// Statistics from a VBIOS interpreter run.
#[derive(Debug, Clone, Default)]
pub struct InterpreterStats {
    pub ops_executed: usize,
    pub writes_applied: usize,
    pub writes_skipped_pri: usize,
    pub ops_skipped: usize,
    pub conditions_evaluated: usize,
    pub delays_total_us: u64,
    pub unknown_opcodes: Vec<(usize, u8)>,
    pub pri_faults: usize,
    pub pri_recoveries: usize,
    pub faulted_domains: Vec<String>,
}

/// State for the VBIOS init script interpreter.
struct VbiosInterpreter<'a> {
    bar0: &'a MappedBar,
    rom: &'a [u8],
    offset: usize,
    execute: bool,
    repeat_count: u8,
    repeat_offset: usize,
    nested: u32,
    stats: InterpreterStats,
    /// PRI backpressure: consecutive faults without a clean read.
    pri_consecutive_faults: u32,
    /// PRI backpressure: threshold before attempting bus recovery.
    pri_fault_threshold: u32,
    /// PRI backpressure: domain -> fault count. Domains with 3+ faults are skipped.
    pri_domain_faults: std::collections::HashMap<String, u32>,
}

impl<'a> VbiosInterpreter<'a> {
    fn new(bar0: &'a MappedBar, rom: &'a [u8], start: usize) -> Self {
        Self {
            bar0,
            rom,
            offset: start,
            execute: true,
            repeat_count: 0,
            repeat_offset: 0,
            nested: 0,
            stats: InterpreterStats::default(),
            pri_consecutive_faults: 0,
            pri_fault_threshold: 5,
            pri_domain_faults: std::collections::HashMap::new(),
        }
    }

    fn rd08(&self, off: usize) -> u8 {
        self.rom.get(off).copied().unwrap_or(0)
    }

    fn rd16(&self, off: usize) -> u16 {
        if off + 2 <= self.rom.len() {
            u16::from_le_bytes([self.rom[off], self.rom[off + 1]])
        } else {
            0
        }
    }

    fn rd32(&self, off: usize) -> u32 {
        if off + 4 <= self.rom.len() {
            u32::from_le_bytes([
                self.rom[off],
                self.rom[off + 1],
                self.rom[off + 2],
                self.rom[off + 3],
            ])
        } else {
            0
        }
    }

    /// Look up a condition from the VBIOS condition table.
    /// Returns true if the condition is met (register & mask == value).
    fn condition_met(&mut self, cond_table_off: usize, cond_idx: u8) -> bool {
        let entry_off = cond_table_off + (cond_idx as usize) * 12;
        if entry_off + 12 > self.rom.len() {
            return true; // unknown condition → execute anyway
        }
        let reg = self.rd32(entry_off);
        let mask = self.rd32(entry_off + 4);
        let value = self.rd32(entry_off + 8);
        if reg == 0 {
            return true;
        }
        let actual = self.bar0_rd32(reg);
        (actual & mask) == value
    }

    /// Resolve the "init tables" base pointer from BIT 'I' offset 0x00.
    fn find_init_tables_base(&self) -> usize {
        if let Ok(bit) = BitTable::parse(self.rom)
            && let Some(i_entry) = bit.find(b'I')
        {
            let i_off = i_entry.data_offset as usize;
            if i_off + 2 <= self.rom.len() {
                return self.rd16(i_off) as usize;
            }
        }
        0
    }

    /// Find the condition table offset from the init tables base.
    fn find_condition_table(&self) -> usize {
        let base = self.find_init_tables_base();
        if base == 0 || base + 8 > self.rom.len() {
            return 0;
        }
        self.rd16(base + 0x06) as usize
    }

    fn run(&mut self) -> Result<(), DevinitError> {
        let cond_table = self.find_condition_table();
        self.nested += 1;
        let max_ops = 50_000;

        while self.offset != 0 && self.stats.ops_executed < max_ops {
            let op = self.rd08(self.offset);
            self.stats.ops_executed += 1;

            opcodes::dispatch_opcode(self, op, cond_table)?;
        }

        self.nested -= 1;
        Ok(())
    }
}

/// Number of RAM-restrict groups from VBIOS strap info.
pub(crate) fn ram_restrict_group_count(rom: &[u8]) -> usize {
    if let Ok(bit) = BitTable::parse(rom)
        && let Some(m) = bit.find(b'M')
    {
        let m_off = m.data_offset as usize;
        if m_off + 3 <= rom.len() {
            let count = rom[m_off + 2] as usize;
            if count > 0 && count <= 16 {
                return count;
            }
        }
    }
    4
}

/// Execute VBIOS init scripts from the host CPU via BAR0.
///
/// This is the sovereign alternative to PMU FALCON execution. It interprets
/// the boot script opcode stream directly, respecting control flow, conditions,
/// and delays. Approximately 50 opcodes are handled.
pub fn interpret_boot_scripts(
    bar0: &MappedBar,
    rom: &[u8],
) -> Result<InterpreterStats, DevinitError> {
    let bit = BitTable::parse(rom)?;
    let bit_i = bit.find(b'I').ok_or(DevinitError::BitINotFound)?;

    let i_off = bit_i.data_offset as usize;
    if i_off + 2 > rom.len() {
        return Err(DevinitError::BitIDataTooShort);
    }

    let init_tables_base = u16::from_le_bytes([rom[i_off], rom[i_off + 1]]) as usize;

    if init_tables_base == 0 || init_tables_base + 2 > rom.len() {
        return Err(DevinitError::InterpreterInitTablesInvalid);
    }

    let script_table_ptr =
        u16::from_le_bytes([rom[init_tables_base], rom[init_tables_base + 1]]) as usize;

    if script_table_ptr == 0 || script_table_ptr >= rom.len() {
        return Err(DevinitError::InterpreterScriptTableInvalid);
    }

    tracing::debug!(
        init_tables_base = format!("{init_tables_base:#06x}"),
        script_table = format!("{script_table_ptr:#06x}"),
        "VBIOS interpreter entry points"
    );

    let mut combined_stats = InterpreterStats::default();
    let mut script_idx = 0;

    loop {
        let entry_off = script_table_ptr + script_idx * 2;
        if entry_off + 2 > rom.len() {
            break;
        }
        let script_off = u16::from_le_bytes([rom[entry_off], rom[entry_off + 1]]) as usize;
        if script_off == 0 || script_off >= rom.len() {
            break;
        }

        tracing::debug!(
            script_idx,
            script_off = format!("{script_off:#06x}"),
            "VBIOS interpreter running init script"
        );

        let mut interp = VbiosInterpreter::new(bar0, rom, script_off);
        match interp.run() {
            Ok(()) => {
                tracing::info!(
                    script_idx,
                    ops = interp.stats.ops_executed,
                    writes = interp.stats.writes_applied,
                    pri_skipped = interp.stats.writes_skipped_pri,
                    unknown = interp.stats.unknown_opcodes.len(),
                    pri_faults = interp.stats.pri_faults,
                    pri_recoveries = interp.stats.pri_recoveries,
                    "VBIOS init script completed"
                );
            }
            Err(e) => {
                tracing::error!(
                    script_idx,
                    error = %e,
                    pri_faults = interp.stats.pri_faults,
                    pri_recoveries = interp.stats.pri_recoveries,
                    "VBIOS init script failed"
                );
            }
        }
        combined_stats.ops_executed += interp.stats.ops_executed;
        combined_stats.writes_applied += interp.stats.writes_applied;
        combined_stats.writes_skipped_pri += interp.stats.writes_skipped_pri;
        combined_stats.ops_skipped += interp.stats.ops_skipped;
        combined_stats.conditions_evaluated += interp.stats.conditions_evaluated;
        combined_stats.delays_total_us += interp.stats.delays_total_us;
        combined_stats
            .unknown_opcodes
            .extend(interp.stats.unknown_opcodes.clone());
        combined_stats.pri_faults += interp.stats.pri_faults;
        combined_stats.pri_recoveries += interp.stats.pri_recoveries;
        for (domain, &count) in &interp.pri_domain_faults {
            if count >= 3 && !combined_stats.faulted_domains.contains(domain) {
                combined_stats.faulted_domains.push(domain.clone());
            }
        }

        script_idx += 1;
        if script_idx > 50 {
            break;
        }
    }

    tracing::info!(
        scripts = script_idx,
        ops = combined_stats.ops_executed,
        writes = combined_stats.writes_applied,
        pri_skipped = combined_stats.writes_skipped_pri,
        delays_ms = combined_stats.delays_total_us as f64 / 1000.0,
        unknown = combined_stats.unknown_opcodes.len(),
        "VBIOS interpreter total"
    );

    if combined_stats.pri_faults > 0 {
        tracing::warn!(
            faults = combined_stats.pri_faults,
            recoveries = combined_stats.pri_recoveries,
            faulted_domains = combined_stats.faulted_domains.len(),
            domains = ?combined_stats.faulted_domains,
            "PRI backpressure"
        );
    }

    if !combined_stats.unknown_opcodes.is_empty() {
        let first_few: Vec<_> = combined_stats.unknown_opcodes.iter().take(10).collect();
        tracing::debug!(opcodes = ?first_few, "unknown VBIOS opcodes");
    }

    Ok(combined_stats)
}

#[cfg(test)]
mod ram_restrict_tests {
    use super::ram_restrict_group_count;
    use crate::vfio::channel::devinit::vbios::BitTable;

    fn rom_with_bit_m(m_data_off: usize, count: u8) -> Vec<u8> {
        let bit_off = 0x100;
        let mut rom = vec![0u8; m_data_off + 16];
        rom[bit_off..bit_off + 5].copy_from_slice(&[0xFF, 0xB8, b'B', b'I', b'T']);
        rom[bit_off + 9] = 6;
        rom[bit_off + 10] = 1;
        let e0 = bit_off + 12;
        rom[e0] = b'M';
        rom[e0 + 1] = 1;
        rom[e0 + 2..e0 + 4].copy_from_slice(&0x10u16.to_le_bytes());
        rom[e0 + 4..e0 + 6].copy_from_slice(&(m_data_off as u16).to_le_bytes());
        rom[m_data_off + 2] = count;
        rom
    }

    #[test]
    fn ram_restrict_default_without_m() {
        let rom = vec![0u8; 4096];
        assert_eq!(ram_restrict_group_count(&rom), 4);
    }

    #[test]
    fn ram_restrict_from_bit_m_table() {
        let rom = rom_with_bit_m(0x400, 8);
        assert!(BitTable::parse(&rom).is_ok());
        assert_eq!(ram_restrict_group_count(&rom), 8);
    }
}
