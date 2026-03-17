// SPDX-License-Identifier: AGPL-3.0-only
#![allow(missing_docs)]
//! VBIOS init script host-side interpreter — executes opcode stream via BAR0.
//!
//! Reference: nouveau nvkm/subdev/bios/init.c (Ben Skeggs, Red Hat)

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

    fn bar0_rd32(&mut self, reg: u32) -> u32 {
        let r = reg as usize;
        if !r.is_multiple_of(4) || r >= 0x0100_0000 {
            return 0xDEAD_DEAD;
        }
        let val = self.bar0.read_u32(r).unwrap_or(0xDEAD_DEAD);

        if crate::vfio::channel::registers::pri::is_pri_error(val) {
            self.stats.pri_faults += 1;
            self.pri_consecutive_faults += 1;
            let domain = crate::vfio::channel::registers::pri::domain_name(r).to_string();
            *self.pri_domain_faults.entry(domain).or_insert(0) += 1;

            if self.pri_consecutive_faults >= self.pri_fault_threshold {
                self.attempt_pri_recovery();
            }
        } else {
            self.pri_consecutive_faults = 0;
        }

        val
    }

    fn bar0_wr32(&mut self, reg: u32, val: u32) {
        let r = reg as usize;
        if !self.execute || r >= 0x0100_0000 || !r.is_multiple_of(4) {
            return;
        }

        // Backpressure check: skip writes to domains with 3+ consecutive faults
        let domain = crate::vfio::channel::registers::pri::domain_name(r).to_string();
        if let Some(&faults) = self.pri_domain_faults.get(&domain)
            && faults >= 3
        {
            self.stats.writes_skipped_pri += 1;
            return;
        }

        // Backpressure check: if bus is heavily faulted, pause before writing
        if self.pri_consecutive_faults >= self.pri_fault_threshold * 2 {
            self.stats.writes_skipped_pri += 1;
            return;
        }

        let _ = self.bar0.write_u32(r, val);
        self.stats.writes_applied += 1;
    }

    fn bar0_mask(&mut self, reg: u32, mask: u32, val: u32) -> u32 {
        let cur = self.bar0_rd32(reg);
        if crate::vfio::channel::registers::pri::is_pri_error(cur) {
            return cur;
        }
        self.bar0_wr32(reg, (cur & mask) | val);
        cur
    }

    /// Attempt to clear PRI bus faults and resume operations.
    fn attempt_pri_recovery(&mut self) {
        self.stats.pri_recoveries += 1;

        // Ack PRIV_RING faults
        let _ = self.bar0.write_u32(
            crate::vfio::channel::registers::pri::PRIV_RING_COMMAND,
            crate::vfio::channel::registers::pri::PRIV_RING_CMD_ACK,
        );

        // Clear PMC INTR PRIV_RING bit
        let pmc_intr = self
            .bar0
            .read_u32(crate::vfio::channel::registers::pri::PMC_INTR)
            .unwrap_or(0);
        if pmc_intr & crate::vfio::channel::registers::pri::PMC_INTR_PRIV_RING_BIT != 0 {
            let _ = self.bar0.write_u32(
                crate::vfio::channel::registers::pri::PMC_INTR,
                crate::vfio::channel::registers::pri::PMC_INTR_PRIV_RING_BIT,
            );
        }

        std::thread::sleep(std::time::Duration::from_millis(10));
        self.pri_consecutive_faults = 0;

        // Re-probe: if BOOT0 reads clean, reset domain faults
        let boot0 = self.bar0.read_u32(0).unwrap_or(0xFFFF_FFFF);
        if !crate::vfio::channel::registers::pri::is_pri_error(boot0) && boot0 != 0xFFFF_FFFF {
            self.pri_domain_faults.clear();
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

    fn run(&mut self) -> Result<(), String> {
        let cond_table = self.find_condition_table();
        self.nested += 1;
        let max_ops = 50_000;

        while self.offset != 0 && self.stats.ops_executed < max_ops {
            let op = self.rd08(self.offset);
            self.stats.ops_executed += 1;

            match op {
                // ── Termination ─────────────────────────────────────
                0x71 => {
                    // INIT_DONE — end of script
                    self.offset = 0;
                }

                // ── Control flow ────────────────────────────────────
                0x72 => {
                    // INIT_RESUME — re-enable execution
                    self.execute = true;
                    self.offset += 1;
                }
                0x38 => {
                    // INIT_NOT — invert execution flag
                    self.execute = !self.execute;
                    self.offset += 1;
                }
                0x33 => {
                    // INIT_REPEAT: count(u8) — repeat next block count times
                    self.repeat_count = self.rd08(self.offset + 1);
                    self.repeat_offset = self.offset + 2;
                    self.offset += 2;
                }
                0x36 => {
                    // INIT_END_REPEAT
                    if self.repeat_count > 1 {
                        self.repeat_count -= 1;
                        self.offset = self.repeat_offset;
                    } else {
                        self.repeat_count = 0;
                        self.offset += 1;
                    }
                }
                0x5C => {
                    // INIT_JUMP: offset(u16) — jump to offset in ROM
                    let target = self.rd16(self.offset + 1) as usize;
                    if target == 0 || target >= self.rom.len() {
                        self.offset = 0;
                    } else {
                        self.offset = target;
                    }
                }
                0x5B => {
                    // INIT_SUB_DIRECT: addr(u16) — call sub-script
                    let sub_addr = self.rd16(self.offset + 1) as usize;
                    self.offset += 3;
                    if sub_addr != 0 && sub_addr < self.rom.len() {
                        let saved = self.offset;
                        self.offset = sub_addr;
                        self.run()?;
                        self.offset = saved;
                    }
                }
                0x6B => {
                    // INIT_SUB: index(u8) — call indexed sub-script
                    self.offset += 2;
                    self.stats.ops_skipped += 1;
                }

                // ── Conditions ──────────────────────────────────────
                0x75 => {
                    // INIT_CONDITION: cond(u8)
                    let cond = self.rd08(self.offset + 1);
                    self.stats.conditions_evaluated += 1;
                    if cond_table != 0 && !self.condition_met(cond_table, cond) {
                        self.execute = false;
                    }
                    self.offset += 2;
                }
                0x73 => {
                    // INIT_STRAP_CONDITION: cond(u8), value(u8)
                    self.stats.conditions_evaluated += 1;
                    self.offset += 3;
                }
                0x6D => {
                    // INIT_RAM_CONDITION: mask(u8), value(u8)
                    let mask = self.rd08(self.offset + 1);
                    let value = self.rd08(self.offset + 2);
                    self.stats.conditions_evaluated += 1;
                    let strap = self.bar0_rd32(0x101000);
                    if (strap as u8 & mask) != value {
                        self.execute = false;
                    }
                    self.offset += 3;
                }
                0x56 => {
                    // INIT_CONDITION_TIME: cond(u8), retries(u8), delay(u16)
                    let cond = self.rd08(self.offset + 1);
                    let retries = self.rd08(self.offset + 2) as u32;
                    let delay = self.rd16(self.offset + 3) as u64;
                    self.stats.conditions_evaluated += 1;
                    if cond_table != 0 {
                        let mut met = false;
                        for _ in 0..retries.max(1) {
                            if self.condition_met(cond_table, cond) {
                                met = true;
                                break;
                            }
                            std::thread::sleep(std::time::Duration::from_micros(delay));
                            self.stats.delays_total_us += delay;
                        }
                        if !met {
                            self.execute = false;
                        }
                    }
                    self.offset += 5;
                }
                0x76 => {
                    // INIT_IO_CONDITION: cond(u8)
                    self.stats.conditions_evaluated += 1;
                    self.offset += 2;
                }
                0x39 => {
                    // INIT_IO_FLAG_CONDITION: cond(u8)
                    self.stats.conditions_evaluated += 1;
                    self.offset += 2;
                }
                0x3A => {
                    // INIT_GENERIC_CONDITION: cond(u8), len(u8), then data
                    let size = self.rd08(self.offset + 2) as usize;
                    self.stats.conditions_evaluated += 1;
                    self.offset += 3 + size;
                }

                // ── Delays ──────────────────────────────────────────
                0x74 => {
                    // INIT_TIME: usec(u16)
                    let usec = self.rd16(self.offset + 1) as u64;
                    if self.execute && usec > 0 {
                        if usec <= 20_000 {
                            std::thread::sleep(std::time::Duration::from_micros(usec));
                        } else {
                            std::thread::sleep(std::time::Duration::from_millis(usec / 1000));
                        }
                        self.stats.delays_total_us += usec;
                    }
                    self.offset += 3;
                }
                0x57 => {
                    // INIT_LTIME: usec(u16) — same as TIME
                    let usec = self.rd16(self.offset + 1) as u64;
                    if self.execute && usec > 0 {
                        std::thread::sleep(std::time::Duration::from_micros(usec));
                        self.stats.delays_total_us += usec;
                    }
                    self.offset += 3;
                }

                // ── Register writes ─────────────────────────────────
                0x7A => {
                    // INIT_ZM_REG: addr(u32) + data(u32)
                    let reg = self.rd32(self.offset + 1);
                    let val = self.rd32(self.offset + 5);
                    if reg < 0x0100_0000 {
                        self.bar0_wr32(reg, val);
                    }
                    self.offset += 9;
                }
                0x6E => {
                    // INIT_NV_REG: addr(u32) + mask(u32) + value(u32) — read-modify-write
                    let reg = self.rd32(self.offset + 1);
                    let mask = self.rd32(self.offset + 5);
                    let val = self.rd32(self.offset + 9);
                    if reg < 0x0100_0000 {
                        self.bar0_mask(reg, mask, val);
                    }
                    self.offset += 13;
                }
                0x58 => {
                    // INIT_ZM_REG_SEQUENCE: base(u32) + count(u8) + count×data(u32)
                    let base = self.rd32(self.offset + 1);
                    let count = self.rd08(self.offset + 5) as usize;
                    self.offset += 6;
                    for i in 0..count {
                        if self.offset + 4 > self.rom.len() {
                            break;
                        }
                        let val = self.rd32(self.offset);
                        let reg = base + (i as u32) * 4;
                        if reg < 0x0100_0000 {
                            self.bar0_wr32(reg, val);
                        }
                        self.offset += 4;
                    }
                }
                0x77 => {
                    // INIT_ZM_REG16: addr(u32) + data(u16) — write low 16 bits
                    let reg = self.rd32(self.offset + 1);
                    let val = self.rd16(self.offset + 5) as u32;
                    if reg < 0x0100_0000 {
                        self.bar0_wr32(reg, val);
                    }
                    self.offset += 7;
                }
                0x47 => {
                    // INIT_ANDN_REG: addr(u32) + mask(u32) — clear bits
                    let reg = self.rd32(self.offset + 1);
                    let mask = self.rd32(self.offset + 5);
                    if reg < 0x0100_0000 {
                        self.bar0_mask(reg, !mask, 0);
                    }
                    self.offset += 9;
                }
                0x48 => {
                    // INIT_OR_REG: addr(u32) + value(u32) — set bits
                    let reg = self.rd32(self.offset + 1);
                    let val = self.rd32(self.offset + 5);
                    if reg < 0x0100_0000 {
                        self.bar0_mask(reg, 0xFFFF_FFFF, val);
                    }
                    self.offset += 9;
                }
                0x90 => {
                    // INIT_COPY_ZM_REG: src_reg(u32) + dst_reg(u32)
                    self.offset += 9;
                    self.stats.ops_skipped += 1;
                }
                0x91 => {
                    // INIT_ZM_REG_GROUP: addr(u32) + count(u8) + count×data(u32)
                    let base = self.rd32(self.offset + 1);
                    let count = self.rd08(self.offset + 5) as usize;
                    self.offset += 6;
                    for i in 0..count {
                        if self.offset + 4 > self.rom.len() {
                            break;
                        }
                        let val = self.rd32(self.offset);
                        let reg = base + (i as u32) * 4;
                        if reg < 0x0100_0000 {
                            self.bar0_wr32(reg, val);
                        }
                        self.offset += 4;
                    }
                }
                0x97 => {
                    // INIT_ZM_MASK_ADD: addr(u32) + mask(u32) + add(u8)
                    let reg = self.rd32(self.offset + 1);
                    let mask = self.rd32(self.offset + 5);
                    let add = self.rd08(self.offset + 9) as u32;
                    if reg < 0x0100_0000 {
                        let cur = self.bar0_rd32(reg);
                        self.bar0_wr32(reg, (cur & mask) + add);
                    }
                    self.offset += 11;
                }
                0x5A => {
                    // INIT_ZM_REG_INDIRECT: addr(u32) + src(u32)
                    let reg = self.rd32(self.offset + 1);
                    let src = self.rd32(self.offset + 5);
                    if reg < 0x0100_0000 && src < 0x0100_0000 {
                        let val = self.bar0_rd32(src);
                        self.bar0_wr32(reg, val);
                    }
                    self.offset += 9;
                }
                0x5F => {
                    // INIT_COPY_NV_REG: 22 bytes total
                    self.offset += 22;
                    self.stats.ops_skipped += 1;
                }

                // ── PLL programming ─────────────────────────────────
                0x79 | 0x4B => {
                    self.offset += 9;
                    self.stats.ops_skipped += 1;
                }
                0x34 => {
                    let count = self.rd08(self.offset + 9) as usize;
                    self.offset += 10 + count * 4;
                    self.stats.ops_skipped += 1;
                }
                0x4A => {
                    let count = self.rd08(self.offset + 9) as usize;
                    self.offset += 10 + count * 4;
                    self.stats.ops_skipped += 1;
                }
                0x59 => {
                    self.offset += 13;
                    self.stats.ops_skipped += 1;
                }
                0x87 => {
                    let count = ram_restrict_group_count(self.rom);
                    self.offset += 5 + count * 4;
                    self.stats.ops_skipped += 1;
                }

                // ── RAM-restrict groups ─────────────────────────────
                0x8F => {
                    let count = self.rd08(self.offset + 5) as usize;
                    let n = ram_restrict_group_count(self.rom);
                    self.offset += 6 + count * n * 4;
                    self.stats.ops_skipped += 1;
                }

                // ── I/O and GPIO opcodes (no-op for VFIO) ───────────
                0x69 => {
                    self.offset += 5;
                    self.stats.ops_skipped += 1;
                }
                0x32 => {
                    let count = self.rd08(self.offset + 7) as usize;
                    self.offset += 8 + count * 4;
                    self.stats.ops_skipped += 1;
                }
                0x37 => {
                    self.offset += 11;
                    self.stats.ops_skipped += 1;
                }
                0x3B | 0x3C => {
                    self.offset += 5;
                    self.stats.ops_skipped += 1;
                }
                0x49 => {
                    let count = self.rd08(self.offset + 7) as usize;
                    self.offset += 8 + count * 2;
                    self.stats.ops_skipped += 1;
                }
                0x4C => {
                    self.offset += 7;
                    self.stats.ops_skipped += 1;
                }
                0x4D => {
                    self.offset += 6;
                    self.stats.ops_skipped += 1;
                }
                0x4E => {
                    let count = self.rd08(self.offset + 4) as usize;
                    self.offset += 5 + count;
                    self.stats.ops_skipped += 1;
                }
                0x4F => {
                    self.offset += 9;
                    self.stats.ops_skipped += 1;
                }
                0x50 => {
                    let count = self.rd08(self.offset + 3) as usize;
                    self.offset += 4 + count * 2;
                    self.stats.ops_skipped += 1;
                }
                0x51 => {
                    self.offset += 7;
                    self.stats.ops_skipped += 1;
                }
                0x52 => {
                    self.offset += 4;
                    self.stats.ops_skipped += 1;
                }
                0x53 => {
                    self.offset += 3;
                    self.stats.ops_skipped += 1;
                }
                0x54 => {
                    let count = self.rd08(self.offset + 1) as usize;
                    self.offset += 2 + count * 2;
                    self.stats.ops_skipped += 1;
                }
                0x5E => {
                    self.offset += 6;
                    self.stats.ops_skipped += 1;
                }
                0x62 => {
                    self.offset += 5;
                    self.stats.ops_skipped += 1;
                }
                0x78 => {
                    self.offset += 6;
                    self.stats.ops_skipped += 1;
                }
                0x96 => {
                    self.offset += 11;
                    self.stats.ops_skipped += 1;
                }
                0x98 => {
                    self.offset += 8;
                    self.stats.ops_skipped += 1;
                }
                0x99 => {
                    let count = self.rd08(self.offset + 5) as usize;
                    self.offset += 6 + count;
                    self.stats.ops_skipped += 1;
                }
                0x9A => {
                    self.offset += 9;
                    self.stats.ops_skipped += 1;
                }
                0xA9 => {
                    let count = self.rd08(self.offset + 1) as usize;
                    self.offset += 2 + count * 2;
                    self.stats.ops_skipped += 1;
                }

                // ── Hardware-specific no-ops ─────────────────────────
                0x63 => self.offset += 1,
                0x65 => self.offset += 3,
                0x66..=0x68 => self.offset += 1,
                0x6F => self.offset += 2,
                0x8C | 0x8D | 0x8E | 0x92 | 0xAA => self.offset += 1,

                // ── Unknown opcodes ─────────────────────────────────
                _ => {
                    self.stats.unknown_opcodes.push((self.offset, op));
                    self.stats.ops_skipped += 1;
                    self.offset += 1;
                    if self.stats.unknown_opcodes.len() > 100 {
                        return Err(format!(
                            "Too many unknown opcodes (>100), last at {:#x}: {op:#04x}",
                            self.offset - 1,
                        ));
                    }
                }
            }
        }

        self.nested -= 1;
        Ok(())
    }
}

/// Number of RAM-restrict groups from VBIOS strap info.
fn ram_restrict_group_count(rom: &[u8]) -> usize {
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
pub fn interpret_boot_scripts(bar0: &MappedBar, rom: &[u8]) -> Result<InterpreterStats, String> {
    let bit = BitTable::parse(rom)?;
    let bit_i = bit.find(b'I').ok_or("BIT 'I' not found")?;

    let i_off = bit_i.data_offset as usize;
    if i_off + 2 > rom.len() {
        return Err("BIT 'I' data too short".into());
    }

    let init_tables_base = u16::from_le_bytes([rom[i_off], rom[i_off + 1]]) as usize;

    if init_tables_base == 0 || init_tables_base + 2 > rom.len() {
        return Err("Init tables base pointer is null or invalid".into());
    }

    let script_table_ptr =
        u16::from_le_bytes([rom[init_tables_base], rom[init_tables_base + 1]]) as usize;

    if script_table_ptr == 0 || script_table_ptr >= rom.len() {
        return Err("Init script table pointer is null or invalid".into());
    }

    eprintln!(
        "  VBIOS interpreter: init_tables_base={init_tables_base:#06x}, script_table={script_table_ptr:#06x}",
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

        eprintln!(
            "  VBIOS interpreter: running init script {} at {script_off:#06x}",
            script_idx,
        );

        let mut interp = VbiosInterpreter::new(bar0, rom, script_off);
        match interp.run() {
            Ok(()) => {
                eprintln!(
                    "    script {}: {} ops, {} writes ({} PRI-skipped), {} unknown, {} PRI faults ({} recoveries)",
                    script_idx,
                    interp.stats.ops_executed,
                    interp.stats.writes_applied,
                    interp.stats.writes_skipped_pri,
                    interp.stats.unknown_opcodes.len(),
                    interp.stats.pri_faults,
                    interp.stats.pri_recoveries,
                );
            }
            Err(e) => {
                eprintln!(
                    "    script {}: error: {e} ({} PRI faults, {} recoveries)",
                    script_idx, interp.stats.pri_faults, interp.stats.pri_recoveries,
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

    eprintln!(
        "  VBIOS interpreter total: {} scripts, {} ops, {} writes ({} PRI-skipped), {:.1}ms delays, {} unknown",
        script_idx,
        combined_stats.ops_executed,
        combined_stats.writes_applied,
        combined_stats.writes_skipped_pri,
        combined_stats.delays_total_us as f64 / 1000.0,
        combined_stats.unknown_opcodes.len(),
    );

    if combined_stats.pri_faults > 0 {
        eprintln!(
            "  PRI backpressure: {} faults, {} recoveries, {} faulted domains: {:?}",
            combined_stats.pri_faults,
            combined_stats.pri_recoveries,
            combined_stats.faulted_domains.len(),
            combined_stats.faulted_domains,
        );
    }

    if !combined_stats.unknown_opcodes.is_empty() {
        let first_few: Vec<_> = combined_stats.unknown_opcodes.iter().take(10).collect();
        eprintln!("  Unknown opcodes: {:?}", first_few);
    }

    Ok(combined_stats)
}
