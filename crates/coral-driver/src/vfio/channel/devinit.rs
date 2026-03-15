// SPDX-License-Identifier: AGPL-3.0-only
//! GPU Device Initialization (devinit) — sovereign HBM2/GDDR training via VBIOS + PMU.
//!
//! After a GPU enters D3cold and returns to D0, the HBM2 memory controller loses
//! its training state. The GPU's boot ROM normally re-runs the devinit sequence
//! during power-on, but when bound to `vfio-pci` this can fail silently.
//!
//! This module replicates what nouveau's `gm200_devinit_post()` does:
//! 1. Read the VBIOS ROM from sysfs or PRAMIN
//! 2. Parse the BIT (BIOS Information Table) structure
//! 3. Extract the PMU DEVINIT firmware (type 0x04)
//! 4. Upload code+data to the PMU FALCON microcontroller via BAR0
//! 5. Execute the devinit script interpreter on the PMU
//! 6. Wait for completion — at which point HBM2 is trained and VRAM is alive
//!
//! This is the sovereign alternative to binding nouveau just for warmth.
//!
//! # Register map (PMU FALCON at BAR0 + 0x10A000)
//!
//! | Register    | Offset    | Description                      |
//! |-------------|-----------|----------------------------------|
//! | FALCON_CTRL | 0x10A100  | Start/stop PMU execution         |
//! | FALCON_PC   | 0x10A104  | Program counter (init address)   |
//! | FALCON_TRIG | 0x10A10C  | Execution trigger                |
//! | FALCON_MBOX | 0x10A040  | Mailbox / completion signal      |
//! | IMEM_PORT   | 0x10A180  | IMEM access (code upload select) |
//! | IMEM_DATA   | 0x10A184  | IMEM data write                  |
//! | IMEM_TAG    | 0x10A188  | IMEM block tag/address           |
//! | DMEM_PORT   | 0x10A1C0  | DMEM access (data upload select) |
//! | DMEM_DATA   | 0x10A1C4  | DMEM data write                  |

use crate::vfio::device::MappedBar;

// ── VBIOS init script register-write extraction ─────────────────────────

/// A register write extracted from a VBIOS init script.
#[derive(Debug, Clone)]
pub struct ScriptRegWrite {
    /// Byte offset within the VBIOS ROM where this opcode was found.
    pub rom_offset: usize,
    /// The VBIOS init opcode (0x6E, 0x7A, 0x58, 0x77).
    pub opcode: u8,
    /// BAR0 register offset to write.
    pub reg: u32,
    /// Value to write (or OR-mask for read-modify-write).
    pub value: u32,
    /// AND-mask for NV_REG (0x6E) read-modify-write; None for direct writes.
    pub mask: Option<u32>,
}

/// Scan a VBIOS ROM for register writes embedded in init scripts.
///
/// The Volta VBIOS uses PMU-format scripts, but many opcodes overlap with
/// nouveau's host-side format. This scanner finds register writes by
/// matching known opcode patterns (0x6E NV_REG, 0x7A ZM_REG, etc.)
/// without full sequential interpretation.
///
/// Returns writes sorted by ROM offset (approximate execution order).
pub fn scan_init_script_writes(rom: &[u8], start: usize, length: usize) -> Vec<ScriptRegWrite> {
    let end = (start + length).min(rom.len());
    let mut writes = Vec::new();

    let r32 = |off: usize| -> u32 {
        if off + 4 <= rom.len() {
            u32::from_le_bytes([rom[off], rom[off + 1], rom[off + 2], rom[off + 3]])
        } else {
            0
        }
    };

    // Sequential opcode-aware scan
    let mut pos = start;
    while pos < end {
        let op = rom[pos];
        match op {
            // init_zm_reg — opcode 0x7A: addr(u32) + data(u32) = 9 bytes
            0x7A => {
                if pos + 9 <= end {
                    let reg = r32(pos + 1);
                    let val = r32(pos + 5);
                    if reg < 0x0100_0000 {
                        writes.push(ScriptRegWrite {
                            rom_offset: pos, opcode: op, reg, value: val, mask: None,
                        });
                    }
                    pos += 9;
                } else { break; }
            }
            // init_nv_reg — opcode 0x6E: addr(u32) + mask(u32) + value(u32) = 13 bytes
            0x6E => {
                if pos + 13 <= end {
                    let reg = r32(pos + 1);
                    let mask = r32(pos + 5);
                    let val = r32(pos + 9);
                    if reg < 0x0100_0000 {
                        writes.push(ScriptRegWrite {
                            rom_offset: pos, opcode: op, reg, value: val, mask: Some(mask),
                        });
                    }
                    pos += 13;
                } else { break; }
            }
            // init_zm_reg_sequence — opcode 0x58: base(u32) + count(u8) then count × u32
            0x58 => {
                if pos + 6 <= end {
                    let base = r32(pos + 1);
                    let count = rom[pos + 5] as usize;
                    pos += 6;
                    for i in 0..count {
                        if pos + 4 > end { break; }
                        let val = r32(pos);
                        let reg = base + (i as u32) * 4;
                        if reg < 0x0100_0000 {
                            writes.push(ScriptRegWrite {
                                rom_offset: pos, opcode: 0x58, reg, value: val, mask: None,
                            });
                        }
                        pos += 4;
                    }
                } else { break; }
            }
            // init_zm_reg16 — opcode 0x77: addr(u32) + data(u16) = 7 bytes
            0x77 => {
                if pos + 7 <= end {
                    let reg = r32(pos + 1);
                    let val = u16::from_le_bytes([rom[pos + 5], rom[pos + 6]]) as u32;
                    if reg < 0x0100_0000 {
                        writes.push(ScriptRegWrite {
                            rom_offset: pos, opcode: op, reg, value: val, mask: None,
                        });
                    }
                    pos += 7;
                } else { break; }
            }
            // Fixed-size opcodes we can skip
            0x71 | 0x72 | 0x63 | 0x8C | 0x8D | 0x8E | 0x92 | 0xAA => { pos += 1; }
            0x75 | 0x6F | 0x6B | 0x33 => { pos += 2; }
            0x5B | 0x5C | 0x73 | 0x76 | 0x66 | 0x6D | 0x65 => { pos += 3; }
            0x56 | 0x74 | 0x69 | 0x9B => { pos += 5; }
            0x57 => { pos += 3; }
            0x5A | 0x90 => { pos += 9; }
            0x78 => { pos += 6; }
            0x79 => { pos += 7; }
            0x97 => { pos += 11; }
            0x5F => { pos += 22; }
            // Unknown opcode — advance by 1 and hope for the best
            _ => { pos += 1; }
        }
    }

    writes
}

/// Extract all register writes from the VBIOS boot script region.
///
/// Parses the BIT 'I' entry to find boot script location and size,
/// then scans for register write opcodes.
pub fn extract_boot_script_writes(rom: &[u8]) -> Result<Vec<ScriptRegWrite>, String> {
    let bit = BitTable::parse(rom)?;
    let bit_i = bit.find(b'I').ok_or("BIT 'I' not found")?;

    let i_off = bit_i.data_offset as usize;
    if i_off + 0x1c > rom.len() {
        return Err("BIT 'I' data too short".into());
    }

    let script_off = u16::from_le_bytes([rom[i_off + 0x18], rom[i_off + 0x19]]) as usize;
    let script_len = u16::from_le_bytes([rom[i_off + 0x1a], rom[i_off + 0x1b]]) as usize;

    if script_off == 0 || script_len == 0 {
        return Err("No boot scripts in BIT 'I'".into());
    }

    eprintln!(
        "  Scanning boot scripts at {script_off:#06x} ({script_len} bytes) for register writes..."
    );

    let writes = scan_init_script_writes(rom, script_off, script_len);
    eprintln!("  Found {} register writes in boot scripts", writes.len());

    Ok(writes)
}

// ── Full VBIOS init script interpreter ───────────────────────────────────
//
// Unlike `scan_init_script_writes` which only extracts register writes,
// this interpreter executes the opcode stream sequentially, respecting:
// - Control flow (REPEAT/END_REPEAT, JUMP, SUB_DIRECT, DONE)
// - Conditions (CONDITION, STRAP_CONDITION, RAM_CONDITION, NOT)
// - Delays (TIME, LTIME, CONDITION_TIME)
// - Read-modify-write patterns (NV_REG, ANDN_REG, OR_REG)
//
// Reference: nouveau nvkm/subdev/bios/init.c (Ben Skeggs, Red Hat)

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
            u32::from_le_bytes([self.rom[off], self.rom[off + 1], self.rom[off + 2], self.rom[off + 3]])
        } else {
            0
        }
    }

    fn bar0_rd32(&mut self, reg: u32) -> u32 {
        let r = reg as usize;
        if r % 4 != 0 || r >= 0x0100_0000 { return 0xDEAD_DEAD; }
        let val = self.bar0.read_u32(r).unwrap_or(0xDEAD_DEAD);

        if super::registers::pri::is_pri_error(val) {
            self.stats.pri_faults += 1;
            self.pri_consecutive_faults += 1;
            let domain = super::registers::pri::domain_name(r).to_string();
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
        if !self.execute || r >= 0x0100_0000 || r % 4 != 0 { return; }

        // Backpressure check: skip writes to domains with 3+ consecutive faults
        let domain = super::registers::pri::domain_name(r).to_string();
        if let Some(&faults) = self.pri_domain_faults.get(&domain) {
            if faults >= 3 {
                self.stats.writes_skipped_pri += 1;
                return;
            }
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
        if super::registers::pri::is_pri_error(cur) {
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
            super::registers::pri::PRIV_RING_COMMAND,
            super::registers::pri::PRIV_RING_CMD_ACK,
        );

        // Clear PMC INTR PRIV_RING bit
        let pmc_intr = self.bar0.read_u32(super::registers::pri::PMC_INTR).unwrap_or(0);
        if pmc_intr & super::registers::pri::PMC_INTR_PRIV_RING_BIT != 0 {
            let _ = self.bar0.write_u32(
                super::registers::pri::PMC_INTR,
                super::registers::pri::PMC_INTR_PRIV_RING_BIT,
            );
        }

        std::thread::sleep(std::time::Duration::from_millis(10));
        self.pri_consecutive_faults = 0;

        // Re-probe: if BOOT0 reads clean, reset domain faults
        let boot0 = self.bar0.read_u32(0).unwrap_or(0xFFFF_FFFF);
        if !super::registers::pri::is_pri_error(boot0) && boot0 != 0xFFFF_FFFF {
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
        if reg == 0 { return true; }
        let actual = self.bar0_rd32(reg);
        (actual & mask) == value
    }

    /// Resolve the "init tables" base pointer from BIT 'I' offset 0x00.
    ///
    /// nouveau: `init_table(bios)` reads `BIT_I.data_offset + 0x00` as u16.
    /// This base is then used for all sub-table lookups.
    fn find_init_tables_base(&self) -> usize {
        if let Ok(bit) = BitTable::parse(self.rom) {
            if let Some(i_entry) = bit.find(b'I') {
                let i_off = i_entry.data_offset as usize;
                if i_off + 2 <= self.rom.len() {
                    return self.rd16(i_off) as usize;
                }
            }
        }
        0
    }

    /// Find the condition table offset from the init tables base.
    ///
    /// nouveau: `init_condition_table(b)` = `init_table_(b, 0x06, ...)`
    ///   which reads u16 at `init_tables_base + 0x06`.
    fn find_condition_table(&self) -> usize {
        let base = self.find_init_tables_base();
        if base == 0 || base + 8 > self.rom.len() { return 0; }
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
                    // Skip — requires init script table lookup
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
                    // Read NV_PEXTDEV_BOOT_0 (strap) and check
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
                        if self.offset + 4 > self.rom.len() { break; }
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
                    let src = self.rd32(self.offset + 1);
                    let shift = self.rd08(self.offset + 5);
                    let smask = self.rd32(self.offset + 6);
                    let _sxor = self.rd08(self.offset + 10);
                    // Simplified: copy src to dst with mask
                    let _ = (src, shift, smask);
                    self.offset += 9;
                    self.stats.ops_skipped += 1;
                }
                0x91 => {
                    // INIT_ZM_REG_GROUP: addr(u32) + count(u8) + count×data(u32)
                    let base = self.rd32(self.offset + 1);
                    let count = self.rd08(self.offset + 5) as usize;
                    self.offset += 6;
                    for i in 0..count {
                        if self.offset + 4 > self.rom.len() { break; }
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
                0x79 => {
                    // INIT_PLL: addr(u32) + freq(u32) — skip PLL programming
                    self.offset += 9;
                    self.stats.ops_skipped += 1;
                }
                0x4B => {
                    // INIT_PLL2: addr(u32) + freq(u32) — skip
                    self.offset += 9;
                    self.stats.ops_skipped += 1;
                }
                0x34 => {
                    // INIT_IO_RESTRICT_PLL: skip variable
                    let count = self.rd08(self.offset + 9) as usize;
                    self.offset += 10 + count * 4;
                    self.stats.ops_skipped += 1;
                }
                0x4A => {
                    // INIT_IO_RESTRICT_PLL2: skip variable
                    let count = self.rd08(self.offset + 9) as usize;
                    self.offset += 10 + count * 4;
                    self.stats.ops_skipped += 1;
                }
                0x59 => {
                    // INIT_PLL_INDIRECT: addr(u32) + io_port(u16) + io_flag(u8) + ...
                    self.offset += 13;
                    self.stats.ops_skipped += 1;
                }
                0x87 => {
                    // INIT_RAM_RESTRICT_PLL: variable
                    let count = ram_restrict_group_count(self.rom);
                    self.offset += 5 + count * 4;
                    self.stats.ops_skipped += 1;
                }

                // ── RAM-restrict groups ─────────────────────────────
                0x8F => {
                    // INIT_RAM_RESTRICT_ZM_REG_GROUP: addr(u32) + count(u8) + count×(N×u32)
                    let count = self.rd08(self.offset + 5) as usize;
                    let n = ram_restrict_group_count(self.rom);
                    self.offset += 6 + count * n * 4;
                    self.stats.ops_skipped += 1;
                }

                // ── I/O and GPIO opcodes (no-op for VFIO) ───────────
                0x69 => { self.offset += 5; self.stats.ops_skipped += 1; } // INIT_IO
                0x32 => {                                                   // INIT_IO_RESTRICT_PROG
                    let count = self.rd08(self.offset + 7) as usize;
                    self.offset += 8 + count * 4;
                    self.stats.ops_skipped += 1;
                }
                0x37 => { self.offset += 11; self.stats.ops_skipped += 1; } // INIT_COPY
                0x3B => { self.offset += 5; self.stats.ops_skipped += 1; }  // INIT_IO_MASK_OR
                0x3C => { self.offset += 5; self.stats.ops_skipped += 1; }  // INIT_IO_OR
                0x49 => {                                                    // INIT_IDX_ADDR_LATCHED
                    let count = self.rd08(self.offset + 7) as usize;
                    self.offset += 8 + count * 2;
                    self.stats.ops_skipped += 1;
                }
                0x4C => { self.offset += 7; self.stats.ops_skipped += 1; }  // INIT_I2C_BYTE
                0x4D => { self.offset += 6; self.stats.ops_skipped += 1; }  // INIT_ZM_I2C_BYTE
                0x4E => {                                                    // INIT_ZM_I2C
                    let count = self.rd08(self.offset + 4) as usize;
                    self.offset += 5 + count;
                    self.stats.ops_skipped += 1;
                }
                0x4F => { self.offset += 9; self.stats.ops_skipped += 1; }  // INIT_TMDS
                0x50 => {                                                    // INIT_ZM_TMDS_GROUP
                    let count = self.rd08(self.offset + 3) as usize;
                    self.offset += 4 + count * 2;
                    self.stats.ops_skipped += 1;
                }
                0x51 => { self.offset += 7; self.stats.ops_skipped += 1; }  // INIT_CR_IDX_ADR_LATCH
                0x52 => { self.offset += 4; self.stats.ops_skipped += 1; }  // INIT_CR
                0x53 => { self.offset += 3; self.stats.ops_skipped += 1; }  // INIT_ZM_CR
                0x54 => {                                                    // INIT_ZM_CR_GROUP
                    let count = self.rd08(self.offset + 1) as usize;
                    self.offset += 2 + count * 2;
                    self.stats.ops_skipped += 1;
                }
                0x5E => { self.offset += 6; self.stats.ops_skipped += 1; }  // INIT_I2C_IF
                0x62 => { self.offset += 5; self.stats.ops_skipped += 1; }  // INIT_ZM_INDEX_IO
                0x78 => { self.offset += 6; self.stats.ops_skipped += 1; }  // INIT_INDEX_IO
                0x96 => { self.offset += 11; self.stats.ops_skipped += 1; } // INIT_XLAT
                0x98 => { self.offset += 8; self.stats.ops_skipped += 1; }  // INIT_AUXCH
                0x99 => {                                                    // INIT_ZM_AUXCH
                    let count = self.rd08(self.offset + 5) as usize;
                    self.offset += 6 + count;
                    self.stats.ops_skipped += 1;
                }
                0x9A => { self.offset += 9; self.stats.ops_skipped += 1; }  // INIT_I2C_LONG_IF
                0xA9 => {                                                    // INIT_GPIO_NE
                    let count = self.rd08(self.offset + 1) as usize;
                    self.offset += 2 + count * 2;
                    self.stats.ops_skipped += 1;
                }

                // ── Hardware-specific no-ops ─────────────────────────
                0x63 => { self.offset += 1; }  // INIT_COMPUTE_MEM
                0x65 => { self.offset += 3; }  // INIT_RESET
                0x66 => { self.offset += 1; }  // INIT_CONFIGURE_MEM
                0x67 => { self.offset += 1; }  // INIT_CONFIGURE_CLK
                0x68 => { self.offset += 1; }  // INIT_CONFIGURE_PREINIT
                0x6F => { self.offset += 2; }  // INIT_MACRO
                0x8C => { self.offset += 1; }  // INIT_RESET_BEGUN
                0x8D => { self.offset += 1; }  // INIT_RESET_END
                0x8E => { self.offset += 1; }  // INIT_GPIO
                0x92 => { self.offset += 1; }  // INIT_RESERVED
                0xAA => { self.offset += 1; }  // INIT_RESERVED

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
    // Try reading from BIT 'M' table; default to 4 for Volta
    if let Ok(bit) = BitTable::parse(rom) {
        if let Some(m) = bit.find(b'M') {
            let m_off = m.data_offset as usize;
            if m_off + 3 <= rom.len() {
                let count = rom[m_off + 2] as usize;
                if count > 0 && count <= 16 { return count; }
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

    // BIT 'I' offset 0x00 = pointer to the "init tables" base.
    // The init tables base + 0x00 = pointer to the init script table.
    // Each entry in the script table is a u16 offset to an init script.
    // This double indirection matches nouveau's init_script_table().
    let init_tables_base = u16::from_le_bytes([rom[i_off], rom[i_off + 1]]) as usize;

    if init_tables_base == 0 || init_tables_base + 2 > rom.len() {
        return Err("Init tables base pointer is null or invalid".into());
    }

    let script_table_ptr = u16::from_le_bytes([
        rom[init_tables_base], rom[init_tables_base + 1],
    ]) as usize;

    if script_table_ptr == 0 || script_table_ptr >= rom.len() {
        return Err("Init script table pointer is null or invalid".into());
    }

    eprintln!(
        "  VBIOS interpreter: init_tables_base={init_tables_base:#06x}, script_table={script_table_ptr:#06x}",
    );

    let mut combined_stats = InterpreterStats::default();
    let mut script_idx = 0;

    // Iterate over each init script pointer in the table
    loop {
        let entry_off = script_table_ptr + script_idx * 2;
        if entry_off + 2 > rom.len() { break; }
        let script_off = u16::from_le_bytes([rom[entry_off], rom[entry_off + 1]]) as usize;
        if script_off == 0 { break; }
        if script_off >= rom.len() { break; }

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
                    script_idx,
                    interp.stats.pri_faults,
                    interp.stats.pri_recoveries,
                );
            }
        }
        combined_stats.ops_executed += interp.stats.ops_executed;
        combined_stats.writes_applied += interp.stats.writes_applied;
        combined_stats.writes_skipped_pri += interp.stats.writes_skipped_pri;
        combined_stats.ops_skipped += interp.stats.ops_skipped;
        combined_stats.conditions_evaluated += interp.stats.conditions_evaluated;
        combined_stats.delays_total_us += interp.stats.delays_total_us;
        combined_stats.unknown_opcodes.extend(interp.stats.unknown_opcodes.clone());
        combined_stats.pri_faults += interp.stats.pri_faults;
        combined_stats.pri_recoveries += interp.stats.pri_recoveries;
        for (domain, &count) in &interp.pri_domain_faults {
            if count >= 3 && !combined_stats.faulted_domains.contains(domain) {
                combined_stats.faulted_domains.push(domain.clone());
            }
        }

        script_idx += 1;
        if script_idx > 50 { break; }
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

// ── PCI power management (delegates to pci_discovery) ───────────────────

/// Force a PCI device from D3hot back to D0 by writing to the PM capability.
///
/// Delegates to [`crate::vfio::pci_discovery::force_pci_d0`]. Kept here
/// for backward compatibility with existing call sites in glowplug.rs
/// and vfio_compute.rs.
pub fn force_pci_d0(bdf: &str) -> Result<(), String> {
    crate::vfio::pci_discovery::force_pci_d0(bdf)
}

/// Trigger a PCI D3cold → D0 power cycle via sysfs.
///
/// Delegates to [`crate::vfio::pci_discovery::pci_power_cycle`]. Kept
/// here for backward compatibility.
pub fn pci_power_cycle_devinit(bdf: &str) -> Result<bool, String> {
    crate::vfio::pci_discovery::pci_power_cycle(bdf)
}

// ── PMU FALCON registers ────────────────────────────────────────────────

mod pmu_reg {
    pub const FALCON_CTRL: usize = 0x0010_A100;
    pub const FALCON_PC: usize = 0x0010_A104;
    pub const FALCON_TRIG: usize = 0x0010_A10C;
    pub const FALCON_MBOX0: usize = 0x0010_A040;
    pub const FALCON_MBOX1: usize = 0x0010_A044;
    pub const IMEM_PORT: usize = 0x0010_A180;
    pub const IMEM_DATA: usize = 0x0010_A184;
    pub const IMEM_TAG: usize = 0x0010_A188;
    pub const DMEM_PORT: usize = 0x0010_A1C0;
    pub const DMEM_DATA: usize = 0x0010_A1C4;

    /// GF100+ devinit status: bit 1 = devinit complete.
    /// If `(rd32(0x2240c) & 2) == 0`, devinit has NOT run.
    pub const DEVINIT_STATUS: usize = 0x0002_240C;

    /// PMU FALCON HWCFG — can read to check if FALCON is present.
    pub const FALCON_HWCFG: usize = 0x0010_A108;
    /// PMU FALCON CPUCTL — CPU control register.
    pub const FALCON_CPUCTL: usize = 0x0010_A100;

    /// PMU engine ID register.
    pub const FALCON_ID: usize = 0x0010_A12C;
}

/// Devinit status check result.
#[derive(Debug, Clone)]
pub struct DevinitStatus {
    pub needs_post: bool,
    pub devinit_reg: u32,
    pub pmu_id: u32,
    pub pmu_hwcfg: u32,
    pub pmu_ctrl: u32,
    pub pmu_mbox0: u32,
}

impl DevinitStatus {
    /// Check the GPU's devinit status and PMU FALCON health.
    pub fn probe(bar0: &MappedBar) -> Self {
        let r = |reg| bar0.read_u32(reg).unwrap_or(0xDEAD_DEAD);

        let devinit_reg = r(pmu_reg::DEVINIT_STATUS);
        let needs_post = (devinit_reg & 2) == 0;

        Self {
            needs_post,
            devinit_reg,
            pmu_id: r(pmu_reg::FALCON_ID),
            pmu_hwcfg: r(pmu_reg::FALCON_HWCFG),
            pmu_ctrl: r(pmu_reg::FALCON_CPUCTL),
            pmu_mbox0: r(pmu_reg::FALCON_MBOX0),
        }
    }

    pub fn print_summary(&self) {
        eprintln!("╠══ DEVINIT STATUS ══════════════════════════════════════════╣");
        eprintln!("║ devinit_reg[0x2240c]  = {:#010x}", self.devinit_reg);
        eprintln!("║ needs_post (bit1==0)  = {}", self.needs_post);
        eprintln!("║ PMU FALCON ID         = {:#010x}", self.pmu_id);
        eprintln!("║ PMU FALCON HWCFG      = {:#010x}", self.pmu_hwcfg);
        eprintln!("║ PMU FALCON CTRL       = {:#010x}", self.pmu_ctrl);
        eprintln!("║ PMU MBOX0             = {:#010x}", self.pmu_mbox0);
        if self.needs_post {
            eprintln!("║ *** GPU REQUIRES DEVINIT POST (HBM2 training not done) ***");
        } else {
            eprintln!("║ GPU devinit already complete — HBM2 should be trained.");
        }
    }

    /// Check if FALCON security bits indicate signed-only firmware is required.
    pub fn requires_signed_firmware(&self) -> bool {
        self.pmu_hwcfg & (1 << 8) != 0
    }

    /// Check if the PMU FALCON is halted (vs running).
    pub fn is_falcon_halted(&self) -> bool {
        self.pmu_ctrl & 0x10 != 0
    }
}

/// Comprehensive PMU FALCON diagnostic report.
#[derive(Debug, Clone)]
pub struct FalconDiagnostic {
    pub status: DevinitStatus,
    pub prom_accessible: bool,
    pub prom_signature: u32,
    pub prom_enable_reg: u32,
    pub secure_boot: bool,
    pub falcon_halted: bool,
    pub falcon_pc: u32,
    pub falcon_mbox1: u32,
    pub imem_size_kb: u32,
    pub dmem_size_kb: u32,
    pub vbios_sources: Vec<(String, bool, String)>,
}

impl FalconDiagnostic {
    /// Run comprehensive FALCON diagnostics.
    pub fn probe(bar0: &MappedBar, bdf: Option<&str>) -> Self {
        let r = |reg| bar0.read_u32(reg).unwrap_or(0xDEAD_DEAD);

        let status = DevinitStatus::probe(bar0);

        // PROM accessibility
        let prom_enable_reg = r(PROM_ENABLE_REG);
        let _ = bar0.write_u32(PROM_ENABLE_REG, prom_enable_reg & !1);
        let prom_signature = r(PROM_BASE);
        let prom_accessible = (prom_signature & 0xFFFF) == 0xAA55;
        let _ = bar0.write_u32(PROM_ENABLE_REG, prom_enable_reg);

        // FALCON hardware config
        let hwcfg = status.pmu_hwcfg;
        let secure_boot = hwcfg & (1 << 8) != 0;
        let falcon_halted = status.pmu_ctrl & 0x10 != 0;
        let falcon_pc = r(pmu_reg::FALCON_PC);
        let falcon_mbox1 = r(pmu_reg::FALCON_MBOX1);

        // IMEM/DMEM sizes from HWCFG
        let imem_size_kb = ((hwcfg >> 16) & 0x1FF) * 256 / 1024;
        let dmem_size_kb = ((hwcfg >> 0) & 0x1FF) * 256 / 1024;

        // Check available VBIOS sources
        let mut vbios_sources = Vec::new();

        // Source 1: PROM
        vbios_sources.push((
            "PROM (BAR0+0x300000)".into(),
            prom_accessible,
            if prom_accessible {
                format!("signature {prom_signature:#010x}")
            } else {
                format!("signature mismatch: {prom_signature:#010x}")
            },
        ));

        // Source 2: sysfs ROM
        if let Some(bdf) = bdf {
            let rom_path = format!("/sys/bus/pci/devices/{bdf}/rom");
            let sysfs_ok = std::fs::metadata(&rom_path).is_ok();
            vbios_sources.push((
                format!("sysfs ({rom_path})"),
                sysfs_ok,
                if sysfs_ok { "file exists".into() } else { "not available".into() },
            ));
        }

        // Source 3: pre-dumped files
        let dump_paths = [
            "/home/biomegate/Development/ecoPrimals/hotSpring/data/vbios_0000_4a_00_0.bin",
            "/home/biomegate/Development/ecoPrimals/hotSpring/data/vbios_0000_03_00_0.bin",
        ];
        for path in &dump_paths {
            let exists = std::fs::metadata(path).is_ok();
            let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
            vbios_sources.push((
                format!("file ({path})"),
                exists,
                if exists { format!("{} KB", size / 1024) } else { "not found".into() },
            ));
        }

        Self {
            status,
            prom_accessible,
            prom_signature,
            prom_enable_reg,
            secure_boot,
            falcon_halted,
            falcon_pc,
            falcon_mbox1,
            imem_size_kb,
            dmem_size_kb,
            vbios_sources,
        }
    }

    /// Print a human-readable diagnostic report.
    pub fn print_report(&self) {
        eprintln!("╠══ PMU FALCON DIAGNOSTIC ═══════════════════════════════════╣");
        self.status.print_summary();
        eprintln!("║");
        eprintln!("║ FALCON Security:");
        eprintln!("║   Secure boot required: {}", self.secure_boot);
        eprintln!("║   FALCON halted: {}", self.falcon_halted);
        eprintln!("║   FALCON PC: {:#010x}", self.falcon_pc);
        eprintln!("║   FALCON MBOX1: {:#010x}", self.falcon_mbox1);
        eprintln!("║   IMEM: {} KB, DMEM: {} KB", self.imem_size_kb, self.dmem_size_kb);
        eprintln!("║");
        eprintln!("║ PROM Access:");
        eprintln!("║   Enable reg (0x1854): {:#010x}", self.prom_enable_reg);
        eprintln!("║   PROM signature: {:#010x} ({})", self.prom_signature,
            if self.prom_accessible { "OK" } else { "FAIL" });
        eprintln!("║");
        eprintln!("║ VBIOS Sources:");
        for (name, ok, detail) in &self.vbios_sources {
            eprintln!("║   {} {} — {}", if *ok { "✓" } else { "✗" }, name, detail);
        }
        eprintln!("║");

        // Recommendation
        if self.status.needs_post {
            if self.secure_boot {
                eprintln!("║ RECOMMENDATION: PMU requires signed firmware.");
                eprintln!("║   → Use host-side VBIOS interpreter (interpret_boot_scripts)");
                eprintln!("║   → Or use differential replay from oracle card");
            } else if self.prom_accessible {
                eprintln!("║ RECOMMENDATION: FALCON upload should work.");
                eprintln!("║   → Try execute_devinit() with PROM-read VBIOS");
            } else {
                eprintln!("║ RECOMMENDATION: PROM inaccessible, FALCON unsigned.");
                if self.vbios_sources.iter().any(|(_, ok, _)| *ok) {
                    eprintln!("║   → Try execute_devinit() with file-based VBIOS");
                } else {
                    eprintln!("║   → No VBIOS source available — try oracle replay");
                }
            }
        } else {
            eprintln!("║ RECOMMENDATION: Devinit already complete, no action needed.");
        }
        eprintln!("╚═══════════════════════════════════════════════════════════╝");
    }

    /// Find the best available VBIOS ROM, trying all sources.
    pub fn best_vbios(&self, bar0: &MappedBar, bdf: Option<&str>) -> Result<Vec<u8>, String> {
        // Try PROM first
        if self.prom_accessible {
            if let Ok(rom) = read_vbios_prom(bar0) {
                return Ok(rom);
            }
        }

        // Try sysfs
        if let Some(bdf) = bdf {
            if let Ok(rom) = read_vbios_sysfs(bdf) {
                return Ok(rom);
            }
        }

        // Try pre-dumped files
        for (name, ok, _) in &self.vbios_sources {
            if !ok { continue; }
            if let Some(path) = name.strip_prefix("file (").and_then(|s| s.strip_suffix(')')) {
                if let Ok(rom) = read_vbios_file(path) {
                    return Ok(rom);
                }
            }
        }

        Err("No VBIOS source available".into())
    }
}

/// Execute devinit with enhanced diagnostics and automatic VBIOS source selection.
///
/// Unlike the basic `execute_devinit`, this function:
/// 1. Runs full FALCON diagnostics first
/// 2. Automatically selects the best VBIOS source
/// 3. Falls back to host-side interpreter if FALCON upload fails
/// 4. Provides detailed timing information during execution
pub fn execute_devinit_with_diagnostics(
    bar0: &MappedBar,
    bdf: Option<&str>,
) -> Result<bool, String> {
    let diag = FalconDiagnostic::probe(bar0, bdf);
    diag.print_report();

    if !diag.status.needs_post {
        return Ok(false);
    }

    let rom = diag.best_vbios(bar0, bdf)?;

    // If FALCON requires signed firmware, go straight to host interpreter
    if diag.secure_boot {
        eprintln!("  Secure boot detected — using host-side VBIOS interpreter");
        let stats = interpret_boot_scripts(bar0, &rom)?;
        let vram_ok = check_vram_via_pramin(bar0);
        eprintln!(
            "  Interpreter: {} writes, VRAM {}",
            stats.writes_applied,
            if vram_ok { "ALIVE" } else { "still dead" },
        );
        return Ok(vram_ok);
    }

    // Try FALCON upload first
    eprintln!("  Attempting PMU FALCON devinit...");
    match execute_devinit(bar0, &rom) {
        Ok(true) => {
            let vram_ok = check_vram_via_pramin(bar0);
            if vram_ok {
                eprintln!("  FALCON devinit succeeded + VRAM alive!");
                return Ok(true);
            }
            eprintln!("  FALCON devinit completed but VRAM still dead");
        }
        Ok(false) => {
            eprintln!("  FALCON reports devinit not needed");
            return Ok(false);
        }
        Err(e) => {
            eprintln!("  FALCON devinit failed: {e}");
        }
    }

    // Fallback: host-side interpreter
    eprintln!("  Falling back to host-side VBIOS interpreter...");
    let stats = interpret_boot_scripts(bar0, &rom)?;
    let vram_ok = check_vram_via_pramin(bar0);
    eprintln!(
        "  Interpreter fallback: {} writes, VRAM {}",
        stats.writes_applied,
        if vram_ok { "ALIVE" } else { "still dead" },
    );
    Ok(vram_ok)
}

/// Quick VRAM check via PRAMIN sentinel.
fn check_vram_via_pramin(bar0: &MappedBar) -> bool {
    use crate::vfio::memory::{MemoryRegion, PraminRegion};
    if let Ok(mut region) = PraminRegion::new(bar0, 0x0002_6000, 8) {
        region.probe_sentinel(0, 0xCAFE_DEAD).is_working()
    } else {
        false
    }
}

// ── VBIOS ROM reader ────────────────────────────────────────────────────

/// BAR0 offset of the PROM (Programmable ROM) region.
/// The GPU's internal VBIOS is stored here, separate from the PCI Expansion ROM.
const PROM_BASE: usize = 0x0030_0000;

/// PROM enable register — clear bit 0 to enable PROM access.
const PROM_ENABLE_REG: usize = 0x0000_1854;

/// Read the VBIOS from the GPU's internal PROM via BAR0.
///
/// This is the sovereign path — no sysfs, no kernel, just BAR0 MMIO.
/// The PROM contains the full NVIDIA BIOS with BIT table, PMU firmware,
/// init scripts, etc. The sysfs `rom` file only provides the PCI Expansion
/// ROM (VGA/EFI option ROM) which lacks the PMU devinit code.
///
/// Source: nouveau `nvkm/subdev/bios/shadowprom.c`
pub fn read_vbios_prom(bar0: &MappedBar) -> Result<Vec<u8>, String> {
    // Enable PROM access (clear bit 0 of 0x1854)
    let enable_reg = bar0.read_u32(PROM_ENABLE_REG).unwrap_or(0xDEAD);
    let _ = bar0.write_u32(PROM_ENABLE_REG, enable_reg & !1);

    // Probe ROM size — read first 4 bytes, check for 0x55AA signature
    let sig_lo = bar0.read_u32(PROM_BASE).unwrap_or(0);
    if (sig_lo & 0xFFFF) != 0xAA55 {
        // Restore PROM enable
        let _ = bar0.write_u32(PROM_ENABLE_REG, enable_reg);
        return Err(format!(
            "PROM signature mismatch: got {sig_lo:#010x} (expected 0x????AA55)"
        ));
    }

    // Image size is at byte 2 (in 512-byte units)
    let image_blocks = ((sig_lo >> 16) & 0xFF) as usize;
    let image_size = if image_blocks > 0 {
        image_blocks * 512
    } else {
        // Fallback: read up to 512KB
        512 * 1024
    };

    // The full NVIDIA VBIOS can be larger than the first image — PMU firmware
    // and init scripts often live in a second image. Read at least 256KB to
    // capture all data referenced by BIT table pointers.
    let max_size = 512 * 1024_usize;
    let read_size = image_size.max(256 * 1024).min(max_size);

    let mut rom = Vec::with_capacity(read_size);
    for offset in (0..read_size).step_by(4) {
        let word = bar0.read_u32(PROM_BASE + offset).unwrap_or(0xFFFF_FFFF);

        // Stop if we're past the first image and hit unprogrammed flash
        if offset > image_size && word == 0xFFFF_FFFF {
            // Read a few more words to confirm it's really the end
            let next = bar0.read_u32(PROM_BASE + offset + 4).unwrap_or(0xFFFF_FFFF);
            if next == 0xFFFF_FFFF {
                break;
            }
        }

        rom.extend_from_slice(&word.to_le_bytes());
    }

    // Restore PROM enable register
    let _ = bar0.write_u32(PROM_ENABLE_REG, enable_reg);

    if rom.len() < 512 {
        return Err(format!("PROM too small: {} bytes", rom.len()));
    }

    eprintln!(
        "  PROM: read {} bytes ({} KB) from BAR0+0x300000",
        rom.len(),
        rom.len() / 1024
    );

    Ok(rom)
}

/// Read the VBIOS ROM image via sysfs PCI ROM BAR.
///
/// Note: the sysfs `rom` file provides the PCI Expansion ROM, not the full
/// NVIDIA internal BIOS. It typically lacks the BIT table and PMU firmware.
/// Prefer `read_vbios_prom()` for the full VBIOS.
pub fn read_vbios_sysfs(bdf: &str) -> Result<Vec<u8>, String> {
    let rom_path = format!("/sys/bus/pci/devices/{bdf}/rom");

    // Enable ROM readback
    std::fs::write(&rom_path, "1").map_err(|e| format!("enable ROM: {e}"))?;

    // Read the full ROM
    let data = std::fs::read(&rom_path).map_err(|e| {
        let _ = std::fs::write(&rom_path, "0");
        format!("read ROM: {e}")
    })?;

    // Disable ROM readback
    let _ = std::fs::write(&rom_path, "0");

    validate_vbios(&data)
}

/// Read a pre-dumped VBIOS ROM from a file path.
pub fn read_vbios_file(path: &str) -> Result<Vec<u8>, String> {
    let data = std::fs::read(path).map_err(|e| format!("read {path}: {e}"))?;
    validate_vbios(&data)
}

fn validate_vbios(data: &[u8]) -> Result<Vec<u8>, String> {
    if data.len() < 512 {
        return Err(format!("ROM too small: {} bytes", data.len()));
    }

    // Verify PCI ROM signature (0x55AA at offset 0)
    if data[0] != 0x55 || data[1] != 0xAA {
        return Err(format!(
            "bad ROM signature: {:#04x} {:#04x} (expected 0x55 0xAA)",
            data[0], data[1]
        ));
    }

    Ok(data.to_vec())
}

// ── BIT table parser ────────────────────────────────────────────────────

/// A single entry from the BIOS Information Table (BIT).
#[derive(Debug, Clone)]
pub struct BitEntry {
    pub id: u8,
    pub version: u8,
    pub header_len: u16,
    pub data_offset: u32,
    pub data_size: u16,
}

/// Parsed BIT header from the VBIOS.
#[derive(Debug)]
pub struct BitTable {
    pub entries: Vec<BitEntry>,
}

impl BitTable {
    /// Scan the VBIOS for the BIT table and parse its entries.
    ///
    /// The BIT signature is `\xFF\xB8BIT` (5 bytes), found via:
    ///   `nvbios_findstr(bios->data, bios->size, "\xff\xb8""BIT", 5)`
    /// (nouveau `nvkm/subdev/bios/base.c` line 189)
    ///
    /// From `nvkm/subdev/bios/bit.c`:
    /// - `bit_offset + 9`  = entry_size
    /// - `bit_offset + 10` = entry_count
    /// - `bit_offset + 12` = first entry
    /// Each entry (6 bytes): id(1) + version(1) + data_size(u16 LE) + data_offset(u16 LE)
    pub fn parse(rom: &[u8]) -> Result<Self, String> {
        let sig: &[u8] = &[0xFF, 0xB8, b'B', b'I', b'T'];

        let bit_offset = rom
            .windows(sig.len())
            .position(|w| w == sig)
            .ok_or("BIT signature (\\xFF\\xB8BIT) not found in VBIOS")?;

        if bit_offset + 12 >= rom.len() {
            return Err("BIT header truncated".into());
        }

        let entry_size = rom[bit_offset + 9] as usize;
        let entry_count = rom[bit_offset + 10] as usize;
        let entries_start = bit_offset + 12;

        eprintln!(
            "  BIT found at 0x{bit_offset:04x}: entry_size={entry_size}, count={entry_count}"
        );

        if entry_size < 6 || entry_size > 16 || entry_count > 64 {
            return Err(format!(
                "BIT header invalid: entry_size={entry_size} count={entry_count}"
            ));
        }

        let mut entries = Vec::with_capacity(entry_count);
        for i in 0..entry_count {
            let off = entries_start + i * entry_size;
            if off + 6 > rom.len() {
                break;
            }

            let id = rom[off];
            let version = rom[off + 1];
            let data_size = u16::from_le_bytes([rom[off + 2], rom[off + 3]]);
            let data_offset = u16::from_le_bytes([rom[off + 4], rom[off + 5]]) as u32;

            if id == 0 && data_offset == 0 {
                continue;
            }

            entries.push(BitEntry {
                id,
                version,
                header_len: 0,
                data_offset,
                data_size,
            });
        }

        Ok(Self { entries })
    }

    /// Find an entry by its single-character ID (e.g., 'I' for PMU init data).
    pub fn find(&self, id: u8) -> Option<&BitEntry> {
        self.entries.iter().find(|e| e.id == id)
    }
}

// ── PMU firmware table (nvbios_pmuR) ────────────────────────────────────

/// A PMU firmware entry extracted from the VBIOS.
#[derive(Debug)]
pub struct PmuFirmware {
    pub app_type: u8,
    pub boot_addr_pmu: u32,
    pub boot_addr: u32,
    pub boot_size: u32,
    pub code_addr_pmu: u32,
    pub code_addr: u32,
    pub code_size: u32,
    pub data_addr_pmu: u32,
    pub data_addr: u32,
    pub data_size: u32,
    pub init_addr_pmu: u32,
    pub args_addr_pmu: u32,
}

/// Parse PMU firmware entries from the BIT 'p' (PMU) table.
///
/// The PMU firmware table format (from nouveau's nvbios/pmu.c):
/// Header: version(1) + header_size(1) + entry_count(1) + entry_size(1)
/// Each entry: type(1) + various offsets depending on version.
pub fn parse_pmu_table(rom: &[u8], bit: &BitTable) -> Result<Vec<PmuFirmware>, String> {
    let p_entry = bit
        .find(b'p')
        .ok_or("BIT 'p' (PMU) entry not found")?;

    let table_ptr_off = p_entry.data_offset as usize;
    if table_ptr_off + 2 > rom.len() {
        return Err("PMU table pointer out of bounds".into());
    }

    // The 'p' entry data contains a pointer to the PMU firmware table
    // at offset 0x00 within the data.
    let pmu_table_off = if p_entry.data_size >= 4 {
        u32::from_le_bytes([
            rom[table_ptr_off],
            rom[table_ptr_off + 1],
            rom.get(table_ptr_off + 2).copied().unwrap_or(0),
            rom.get(table_ptr_off + 3).copied().unwrap_or(0),
        ]) as usize
    } else {
        u16::from_le_bytes([rom[table_ptr_off], rom[table_ptr_off + 1]]) as usize
    };

    if pmu_table_off == 0 || pmu_table_off + 4 > rom.len() {
        return Err(format!("PMU table at {pmu_table_off:#x} out of bounds"));
    }

    let version = rom[pmu_table_off];
    let header_size = rom[pmu_table_off + 1] as usize;
    let entry_count = rom[pmu_table_off + 2] as usize;
    let entry_size = rom[pmu_table_off + 3] as usize;

    if version == 0 || entry_size < 10 {
        return Err(format!(
            "unexpected PMU table format: ver={version} hdr={header_size} entries={entry_count} entry_size={entry_size}"
        ));
    }

    let entries_start = pmu_table_off + header_size;
    let mut firmwares = Vec::new();

    for i in 0..entry_count {
        let off = entries_start + i * entry_size;
        if off + entry_size > rom.len() {
            break;
        }

        let app_type = rom[off];

        let read_u32_at = |base: usize| -> u32 {
            if base + 4 <= rom.len() {
                u32::from_le_bytes([rom[base], rom[base + 1], rom[base + 2], rom[base + 3]])
            } else {
                0
            }
        };
        let read_u16_at = |base: usize| -> u16 {
            if base + 2 <= rom.len() {
                u16::from_le_bytes([rom[base], rom[base + 1]])
            } else {
                0
            }
        };

        // Table entry format varies by version. Version 1 (GM200+):
        // [0] type
        // [1] desc_offset (u32) — pointer to descriptor structure
        let desc_ptr = read_u32_at(off + 4) as usize;
        if desc_ptr == 0 || desc_ptr + 40 > rom.len() {
            continue;
        }

        // PMU app descriptor (nouveau nvbios_pmuR format):
        // This varies, but a common layout is:
        //   [0x00] boot_addr_pmu (u32)
        //   [0x04] boot_addr (u32) — offset in VBIOS image
        //   [0x08] boot_size (u32)
        //   [0x0C] code_addr_pmu (u32)
        //   [0x10] code_addr (u32) — offset in VBIOS image
        //   [0x14] code_size (u32)
        //   [0x18] data_addr_pmu (u32)
        //   [0x1C] data_addr (u32) — offset in VBIOS image
        //   [0x20] data_size (u32)
        //   [0x24] init_addr_pmu (u32) — entry point
        //   [0x28] args_addr_pmu (u32) — argument pointer in DMEM

        // Actually, the simpler version from gm200.c:
        // pmu_load calls nvbios_pmuRm(bios, type, &pmu) which returns
        // the firmware sections. Let's use the descriptor pointer approach.

        // For version >= 0x10, the entry itself may contain the offsets directly:
        if entry_size >= 42 {
            firmwares.push(PmuFirmware {
                app_type,
                boot_addr_pmu: read_u32_at(off + 2),
                boot_addr: read_u32_at(off + 6),
                boot_size: read_u32_at(off + 10),
                code_addr_pmu: read_u32_at(off + 14),
                code_addr: read_u32_at(off + 18),
                code_size: read_u32_at(off + 22),
                data_addr_pmu: read_u32_at(off + 26),
                data_addr: read_u32_at(off + 30),
                data_size: read_u32_at(off + 34),
                init_addr_pmu: read_u32_at(off + 38),
                args_addr_pmu: if entry_size >= 46 {
                    read_u32_at(off + 42)
                } else {
                    0
                },
            });
        } else {
            // Smaller entry — need to follow descriptor pointer
            let d = desc_ptr;
            let _desc_ver = rom.get(d).copied().unwrap_or(0);
            let _desc_hdr = rom.get(d + 1).copied().unwrap_or(0) as usize;

            // Try the common descriptor format
            firmwares.push(PmuFirmware {
                app_type,
                boot_addr_pmu: read_u32_at(d + 4),
                boot_addr: read_u32_at(d + 8),
                boot_size: read_u16_at(d + 12) as u32,
                code_addr_pmu: read_u32_at(d + 14),
                code_addr: read_u32_at(d + 18),
                code_size: read_u16_at(d + 22) as u32,
                data_addr_pmu: read_u32_at(d + 24),
                data_addr: read_u32_at(d + 28),
                data_size: read_u16_at(d + 32) as u32,
                init_addr_pmu: read_u32_at(d + 34),
                args_addr_pmu: read_u32_at(d + 38),
            });
        }
    }

    Ok(firmwares)
}

// ── PMU FALCON execution ────────────────────────────────────────────────

/// Reset the PMU FALCON microcontroller.
pub fn pmu_falcon_reset(bar0: &MappedBar) {
    let r = |reg| bar0.read_u32(reg).unwrap_or(0xDEAD_DEAD);
    let w = |reg, val| { let _ = bar0.write_u32(reg, val); };

    // CPUCTL: bit 1 = start, bit 0 = halt
    // Write 0x02 to halt, then 0x00 to clear
    w(pmu_reg::FALCON_CTRL, 0x02);
    std::thread::sleep(std::time::Duration::from_millis(5));

    // Check if halted
    let ctrl = r(pmu_reg::FALCON_CTRL);
    eprintln!("  PMU FALCON CTRL after halt: {ctrl:#010x}");
}

/// Upload code to PMU FALCON IMEM.
///
/// Replicates `pmu_code()` from gm200.c:
/// - Write (0x01000000 | sec_flag | pmu_addr) to IMEM_PORT to select the address
/// - For each 256-byte block, write IMEM_TAG with (pmu_addr + i) >> 8
/// - Write each 32-bit word to IMEM_DATA
/// - Pad remaining bytes in the last 256-byte block with zeros
pub fn pmu_upload_code(bar0: &MappedBar, rom: &[u8], pmu_addr: u32, rom_offset: u32, size: u32, secure: bool) {
    let w = |reg, val: u32| { let _ = bar0.write_u32(reg, val); };

    let sec_flag: u32 = if secure { 0x1000_0000 } else { 0 };
    w(pmu_reg::IMEM_PORT, 0x0100_0000 | sec_flag | pmu_addr);

    let data = &rom[rom_offset as usize..(rom_offset + size) as usize];
    for (i, chunk) in data.chunks(4).enumerate() {
        let byte_offset = (i * 4) as u32;
        if byte_offset & 0xFF == 0 {
            w(pmu_reg::IMEM_TAG, (pmu_addr + byte_offset) >> 8);
        }

        let word = match chunk.len() {
            4 => u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]),
            3 => u32::from_le_bytes([chunk[0], chunk[1], chunk[2], 0]),
            2 => u32::from_le_bytes([chunk[0], chunk[1], 0, 0]),
            1 => u32::from_le_bytes([chunk[0], 0, 0, 0]),
            _ => 0,
        };
        w(pmu_reg::IMEM_DATA, word);
    }

    // Pad to 256-byte boundary
    let total_words = (size as usize + 3) / 4;
    let remainder = (total_words * 4) & 0xFF;
    if remainder != 0 {
        let padding_words = (256 - remainder) / 4;
        for _ in 0..padding_words {
            w(pmu_reg::IMEM_DATA, 0);
        }
    }
}

/// Upload data to PMU FALCON DMEM.
///
/// Replicates `pmu_data()` from gm200.c:
/// - Write (0x01000000 | pmu_addr) to DMEM_PORT
/// - Write each 32-bit word to DMEM_DATA
pub fn pmu_upload_data(bar0: &MappedBar, rom: &[u8], pmu_addr: u32, rom_offset: u32, size: u32) {
    let w = |reg, val: u32| { let _ = bar0.write_u32(reg, val); };

    w(pmu_reg::DMEM_PORT, 0x0100_0000 | pmu_addr);

    let data = &rom[rom_offset as usize..(rom_offset + size) as usize];
    for chunk in data.chunks(4) {
        let word = match chunk.len() {
            4 => u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]),
            3 => u32::from_le_bytes([chunk[0], chunk[1], chunk[2], 0]),
            2 => u32::from_le_bytes([chunk[0], chunk[1], 0, 0]),
            1 => u32::from_le_bytes([chunk[0], 0, 0, 0]),
            _ => 0,
        };
        w(pmu_reg::DMEM_DATA, word);
    }
}

/// Read a DMEM argument pointer.
///
/// Replicates `pmu_args()` from gm200.c:
/// ```c
/// nvkm_wr32(device, 0x10a1c0, argp);
/// nvkm_wr32(device, 0x10a1c0, nvkm_rd32(device, 0x10a1c4) + argi);
/// return nvkm_rd32(device, 0x10a1c4);
/// ```
pub fn pmu_read_args(bar0: &MappedBar, argp: u32, argi: u32) -> u32 {
    let r = |reg| bar0.read_u32(reg).unwrap_or(0);
    let w = |reg, val: u32| { let _ = bar0.write_u32(reg, val); };

    w(pmu_reg::DMEM_PORT, argp);
    let indirect = r(pmu_reg::DMEM_DATA);
    w(pmu_reg::DMEM_PORT, indirect + argi);
    r(pmu_reg::DMEM_DATA)
}

/// Start PMU FALCON execution at the given address.
///
/// Replicates `pmu_exec()` from gm200.c.
pub fn pmu_exec(bar0: &MappedBar, init_addr: u32) {
    let w = |reg, val: u32| { let _ = bar0.write_u32(reg, val); };
    w(pmu_reg::FALCON_PC, init_addr);
    w(pmu_reg::FALCON_TRIG, 0);
    w(pmu_reg::FALCON_CTRL, 0x02); // start
}

/// Execute the full devinit sequence via PMU FALCON.
///
/// Replicates `gm200_devinit_post()`:
/// 1. Parse BIT 'I' entry for init script pointers
/// 2. Upload DEVINIT app (type 0x04) to PMU
/// 3. Upload opcode tables + boot scripts from VBIOS
/// 4. Execute and wait for completion
///
/// Returns Ok(true) if devinit completed, Ok(false) if it wasn't needed,
/// or Err on failure.
pub fn execute_devinit(bar0: &MappedBar, rom: &[u8]) -> Result<bool, String> {
    let status = DevinitStatus::probe(bar0);
    status.print_summary();

    if !status.needs_post {
        eprintln!("║ Devinit already complete — skipping PMU upload.");
        return Ok(false);
    }

    // Parse BIT table
    let bit = BitTable::parse(rom)?;
    eprintln!("║ BIT table: {} entries", bit.entries.len());
    for entry in &bit.entries {
        eprintln!(
            "║   BIT '{}'  ver={} offset={:#06x} size={}",
            entry.id as char, entry.version, entry.data_offset, entry.data_size
        );
    }

    // Find BIT 'I' entry (PMU init data pointers)
    let bit_i = bit
        .find(b'I')
        .ok_or("BIT 'I' entry not found — cannot locate devinit scripts")?;

    if bit_i.version != 1 || bit_i.data_size < 0x1c {
        return Err(format!(
            "BIT 'I' entry: unexpected version {} or size {} (need ver=1, size>=0x1c)",
            bit_i.version, bit_i.data_size
        ));
    }

    // Parse PMU firmware table and find DEVINIT app (type 0x04)
    let pmu_fws = parse_pmu_table(rom, &bit)?;
    eprintln!("║ PMU firmware entries: {}", pmu_fws.len());
    for fw in &pmu_fws {
        eprintln!(
            "║   type={:#04x} boot={:#x}+{:#x}({}) code={:#x}+{:#x}({}) data={:#x}+{:#x}({}) init={:#x} args={:#x}",
            fw.app_type,
            fw.boot_addr_pmu, fw.boot_addr, fw.boot_size,
            fw.code_addr_pmu, fw.code_addr, fw.code_size,
            fw.data_addr_pmu, fw.data_addr, fw.data_size,
            fw.init_addr_pmu, fw.args_addr_pmu,
        );
    }

    let devinit_fw = pmu_fws
        .iter()
        .find(|fw| fw.app_type == 0x04)
        .ok_or("PMU DEVINIT firmware (type 0x04) not found in VBIOS")?;

    // Validate firmware offsets are within ROM
    let rom_len = rom.len() as u32;
    if devinit_fw.boot_addr + devinit_fw.boot_size > rom_len
        || devinit_fw.code_addr + devinit_fw.code_size > rom_len
        || devinit_fw.data_addr + devinit_fw.data_size > rom_len
    {
        return Err("DEVINIT firmware sections extend beyond ROM".into());
    }

    eprintln!("╠══ PMU FALCON DEVINIT UPLOAD ═══════════════════════════════╣");

    // Step 1: Reset the PMU FALCON
    pmu_falcon_reset(bar0);

    // Step 2: Upload boot code (non-secure) to IMEM
    eprintln!(
        "║ Uploading boot code: {} bytes to PMU IMEM {:#x}",
        devinit_fw.boot_size, devinit_fw.boot_addr_pmu
    );
    pmu_upload_code(
        bar0,
        rom,
        devinit_fw.boot_addr_pmu,
        devinit_fw.boot_addr,
        devinit_fw.boot_size,
        false,
    );

    // Step 3: Upload main code (secure) to IMEM
    eprintln!(
        "║ Uploading main code: {} bytes to PMU IMEM {:#x}",
        devinit_fw.code_size, devinit_fw.code_addr_pmu
    );
    pmu_upload_code(
        bar0,
        rom,
        devinit_fw.code_addr_pmu,
        devinit_fw.code_addr,
        devinit_fw.code_size,
        true,
    );

    // Step 4: Upload data to DMEM
    eprintln!(
        "║ Uploading data: {} bytes to PMU DMEM {:#x}",
        devinit_fw.data_size, devinit_fw.data_addr_pmu
    );
    pmu_upload_data(
        bar0,
        rom,
        devinit_fw.data_addr_pmu,
        devinit_fw.data_addr,
        devinit_fw.data_size,
    );

    // Step 5: Upload opcode tables (from BIT 'I' entry, offsets 0x14..0x18)
    let i_data_off = bit_i.data_offset as usize;
    let opcode_img = u16::from_le_bytes([
        rom.get(i_data_off + 0x14).copied().unwrap_or(0),
        rom.get(i_data_off + 0x15).copied().unwrap_or(0),
    ]) as u32;
    let opcode_len = u16::from_le_bytes([
        rom.get(i_data_off + 0x16).copied().unwrap_or(0),
        rom.get(i_data_off + 0x17).copied().unwrap_or(0),
    ]) as u32;

    if opcode_len > 0 && opcode_img + opcode_len <= rom_len {
        let pmu_opcode_addr = pmu_read_args(bar0, devinit_fw.args_addr_pmu + 0x08, 0x08);
        eprintln!(
            "║ Uploading opcode tables: {} bytes from ROM {:#x} to PMU DMEM {:#x}",
            opcode_len, opcode_img, pmu_opcode_addr
        );
        pmu_upload_data(bar0, rom, pmu_opcode_addr, opcode_img, opcode_len);
    } else {
        eprintln!("║ No opcode table found (img={opcode_img:#x} len={opcode_len})");
    }

    // Step 6: Upload boot scripts (from BIT 'I' entry, offsets 0x18..0x1c)
    let script_img = u16::from_le_bytes([
        rom.get(i_data_off + 0x18).copied().unwrap_or(0),
        rom.get(i_data_off + 0x19).copied().unwrap_or(0),
    ]) as u32;
    let script_len = u16::from_le_bytes([
        rom.get(i_data_off + 0x1a).copied().unwrap_or(0),
        rom.get(i_data_off + 0x1b).copied().unwrap_or(0),
    ]) as u32;

    if script_len > 0 && script_img + script_len <= rom_len {
        let pmu_script_addr = pmu_read_args(bar0, devinit_fw.args_addr_pmu + 0x08, 0x10);
        eprintln!(
            "║ Uploading boot scripts: {} bytes from ROM {:#x} to PMU DMEM {:#x}",
            script_len, script_img, pmu_script_addr
        );
        pmu_upload_data(bar0, rom, pmu_script_addr, script_img, script_len);
    } else {
        eprintln!("║ No boot script found (img={script_img:#x} len={script_len})");
    }

    // Step 7: Trigger DEVINIT execution
    eprintln!("╠══ PMU DEVINIT EXECUTION ═══════════════════════════════════╣");
    let w = |reg, val: u32| { let _ = bar0.write_u32(reg, val); };
    let r = |reg| bar0.read_u32(reg).unwrap_or(0xDEAD_DEAD);

    w(pmu_reg::FALCON_MBOX0, 0x0000_5000);
    pmu_exec(bar0, devinit_fw.init_addr_pmu);

    // Wait up to 2 seconds for completion (bit 13 of MBOX0)
    let start = std::time::Instant::now();
    let timeout = std::time::Duration::from_secs(2);
    let mut completed = false;

    while start.elapsed() < timeout {
        let mbox = r(pmu_reg::FALCON_MBOX0);
        if mbox & 0x2000 != 0 {
            completed = true;
            eprintln!(
                "║ DEVINIT COMPLETE! MBOX0={mbox:#010x} (elapsed: {}ms)",
                start.elapsed().as_millis()
            );
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }

    if !completed {
        let mbox = r(pmu_reg::FALCON_MBOX0);
        let ctrl = r(pmu_reg::FALCON_CTRL);
        eprintln!("║ DEVINIT TIMEOUT! MBOX0={mbox:#010x} CTRL={ctrl:#010x}");
        return Err(format!(
            "PMU DEVINIT timed out after 2s (MBOX0={mbox:#010x})"
        ));
    }

    // Step 8: Run PRE_OS app (type 0x01) — for fan control
    if let Some(preos_fw) = pmu_fws.iter().find(|fw| fw.app_type == 0x01) {
        eprintln!("║ Loading PRE_OS app (fan control)...");
        if preos_fw.boot_addr + preos_fw.boot_size <= rom_len
            && preos_fw.code_addr + preos_fw.code_size <= rom_len
            && preos_fw.data_addr + preos_fw.data_size <= rom_len
        {
            pmu_falcon_reset(bar0);
            pmu_upload_code(bar0, rom, preos_fw.boot_addr_pmu, preos_fw.boot_addr, preos_fw.boot_size, false);
            pmu_upload_code(bar0, rom, preos_fw.code_addr_pmu, preos_fw.code_addr, preos_fw.code_size, true);
            pmu_upload_data(bar0, rom, preos_fw.data_addr_pmu, preos_fw.data_addr, preos_fw.data_size);
            pmu_exec(bar0, preos_fw.init_addr_pmu);
            eprintln!("║ PRE_OS app launched on PMU.");
        }
    }

    // Verify devinit status register now shows complete
    let post_status = DevinitStatus::probe(bar0);
    if !post_status.needs_post {
        eprintln!("║ CONFIRMED: devinit status register now shows COMPLETE.");
    } else {
        eprintln!("║ WARNING: devinit status register still shows needs_post!");
    }

    Ok(true)
}
