// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::error::DevinitError;

use super::{VbiosInterpreter, ram_restrict_group_count};

/// Execute a single VBIOS init opcode.
pub(super) fn dispatch_opcode(
    vm: &mut VbiosInterpreter<'_>,
    op: u8,
    cond_table: usize,
) -> Result<(), DevinitError> {
    match op {
        // ── Termination ─────────────────────────────────────
        0x71 => {
            // INIT_DONE — end of script
            vm.offset = 0;
        }

        // ── Control flow ────────────────────────────────────
        0x72 => {
            // INIT_RESUME — re-enable execution
            vm.execute = true;
            vm.offset += 1;
        }
        0x38 => {
            // INIT_NOT — invert execution flag
            vm.execute = !vm.execute;
            vm.offset += 1;
        }
        0x33 => {
            // INIT_REPEAT: count(u8) — repeat next block count times
            vm.repeat_count = vm.rd08(vm.offset + 1);
            vm.repeat_offset = vm.offset + 2;
            vm.offset += 2;
        }
        0x36 => {
            // INIT_END_REPEAT
            if vm.repeat_count > 1 {
                vm.repeat_count -= 1;
                vm.offset = vm.repeat_offset;
            } else {
                vm.repeat_count = 0;
                vm.offset += 1;
            }
        }
        0x5C => {
            // INIT_JUMP: offset(u16) — jump to offset in ROM
            let target = vm.rd16(vm.offset + 1) as usize;
            if target == 0 || target >= vm.rom.len() {
                vm.offset = 0;
            } else {
                vm.offset = target;
            }
        }
        0x5B => {
            // INIT_SUB_DIRECT: addr(u16) — call sub-script
            let sub_addr = vm.rd16(vm.offset + 1) as usize;
            vm.offset += 3;
            if sub_addr != 0 && sub_addr < vm.rom.len() {
                let saved = vm.offset;
                vm.offset = sub_addr;
                vm.run()?;
                vm.offset = saved;
            }
        }
        0x6B => {
            // INIT_SUB: index(u8) — call indexed sub-script
            vm.offset += 2;
            vm.stats.ops_skipped += 1;
        }

        // ── Conditions ──────────────────────────────────────
        0x75 => {
            // INIT_CONDITION: cond(u8)
            let cond = vm.rd08(vm.offset + 1);
            vm.stats.conditions_evaluated += 1;
            if cond_table != 0 && !vm.condition_met(cond_table, cond) {
                vm.execute = false;
            }
            vm.offset += 2;
        }
        0x73 => {
            // INIT_STRAP_CONDITION: cond(u8), value(u8)
            vm.stats.conditions_evaluated += 1;
            vm.offset += 3;
        }
        0x6D => {
            // INIT_RAM_CONDITION: mask(u8), value(u8)
            let mask = vm.rd08(vm.offset + 1);
            let value = vm.rd08(vm.offset + 2);
            vm.stats.conditions_evaluated += 1;
            let strap = vm.bar0_rd32(0x101000);
            if (strap as u8 & mask) != value {
                vm.execute = false;
            }
            vm.offset += 3;
        }
        0x56 => {
            // INIT_CONDITION_TIME: cond(u8), retries(u8), delay(u16)
            let cond = vm.rd08(vm.offset + 1);
            let retries = vm.rd08(vm.offset + 2) as u32;
            let delay = vm.rd16(vm.offset + 3) as u64;
            vm.stats.conditions_evaluated += 1;
            if cond_table != 0 {
                let mut met = false;
                for _ in 0..retries.max(1) {
                    if vm.condition_met(cond_table, cond) {
                        met = true;
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_micros(delay));
                    vm.stats.delays_total_us += delay;
                }
                if !met {
                    vm.execute = false;
                }
            }
            vm.offset += 5;
        }
        0x76 => {
            // INIT_IO_CONDITION: cond(u8)
            vm.stats.conditions_evaluated += 1;
            vm.offset += 2;
        }
        0x39 => {
            // INIT_IO_FLAG_CONDITION: cond(u8)
            vm.stats.conditions_evaluated += 1;
            vm.offset += 2;
        }
        0x3A => {
            // INIT_GENERIC_CONDITION: cond(u8), len(u8), then data
            let size = vm.rd08(vm.offset + 2) as usize;
            vm.stats.conditions_evaluated += 1;
            vm.offset += 3 + size;
        }

        // ── Delays ──────────────────────────────────────────
        0x74 => {
            // INIT_TIME: usec(u16)
            let usec = vm.rd16(vm.offset + 1) as u64;
            if vm.execute && usec > 0 {
                if usec <= 20_000 {
                    std::thread::sleep(std::time::Duration::from_micros(usec));
                } else {
                    std::thread::sleep(std::time::Duration::from_millis(usec / 1000));
                }
                vm.stats.delays_total_us += usec;
            }
            vm.offset += 3;
        }
        0x57 => {
            // INIT_LTIME: usec(u16) — same as TIME
            let usec = vm.rd16(vm.offset + 1) as u64;
            if vm.execute && usec > 0 {
                std::thread::sleep(std::time::Duration::from_micros(usec));
                vm.stats.delays_total_us += usec;
            }
            vm.offset += 3;
        }

        // ── Register writes ─────────────────────────────────
        0x7A => {
            // INIT_ZM_REG: addr(u32) + data(u32)
            let reg = vm.rd32(vm.offset + 1);
            let val = vm.rd32(vm.offset + 5);
            if reg < 0x0100_0000 {
                vm.bar0_wr32(reg, val);
            }
            vm.offset += 9;
        }
        0x6E => {
            // INIT_NV_REG: addr(u32) + mask(u32) + value(u32) — read-modify-write
            let reg = vm.rd32(vm.offset + 1);
            let mask = vm.rd32(vm.offset + 5);
            let val = vm.rd32(vm.offset + 9);
            if reg < 0x0100_0000 {
                vm.bar0_mask(reg, mask, val);
            }
            vm.offset += 13;
        }
        0x58 => {
            // INIT_ZM_REG_SEQUENCE: base(u32) + count(u8) + count×data(u32)
            let base = vm.rd32(vm.offset + 1);
            let count = vm.rd08(vm.offset + 5) as usize;
            vm.offset += 6;
            for i in 0..count {
                if vm.offset + 4 > vm.rom.len() {
                    break;
                }
                let val = vm.rd32(vm.offset);
                let reg = base + (i as u32) * 4;
                if reg < 0x0100_0000 {
                    vm.bar0_wr32(reg, val);
                }
                vm.offset += 4;
            }
        }
        0x77 => {
            // INIT_ZM_REG16: addr(u32) + data(u16) — write low 16 bits
            let reg = vm.rd32(vm.offset + 1);
            let val = vm.rd16(vm.offset + 5) as u32;
            if reg < 0x0100_0000 {
                vm.bar0_wr32(reg, val);
            }
            vm.offset += 7;
        }
        0x47 => {
            // INIT_ANDN_REG: addr(u32) + mask(u32) — clear bits
            let reg = vm.rd32(vm.offset + 1);
            let mask = vm.rd32(vm.offset + 5);
            if reg < 0x0100_0000 {
                vm.bar0_mask(reg, !mask, 0);
            }
            vm.offset += 9;
        }
        0x48 => {
            // INIT_OR_REG: addr(u32) + value(u32) — set bits
            let reg = vm.rd32(vm.offset + 1);
            let val = vm.rd32(vm.offset + 5);
            if reg < 0x0100_0000 {
                vm.bar0_mask(reg, 0xFFFF_FFFF, val);
            }
            vm.offset += 9;
        }
        0x90 => {
            // INIT_COPY_ZM_REG: src_reg(u32) + dst_reg(u32)
            vm.offset += 9;
            vm.stats.ops_skipped += 1;
        }
        0x91 => {
            // INIT_ZM_REG_GROUP: addr(u32) + count(u8) + count×data(u32)
            let base = vm.rd32(vm.offset + 1);
            let count = vm.rd08(vm.offset + 5) as usize;
            vm.offset += 6;
            for i in 0..count {
                if vm.offset + 4 > vm.rom.len() {
                    break;
                }
                let val = vm.rd32(vm.offset);
                let reg = base + (i as u32) * 4;
                if reg < 0x0100_0000 {
                    vm.bar0_wr32(reg, val);
                }
                vm.offset += 4;
            }
        }
        0x97 => {
            // INIT_ZM_MASK_ADD: addr(u32) + mask(u32) + add(u8)
            let reg = vm.rd32(vm.offset + 1);
            let mask = vm.rd32(vm.offset + 5);
            let add = vm.rd08(vm.offset + 9) as u32;
            if reg < 0x0100_0000 {
                let cur = vm.bar0_rd32(reg);
                vm.bar0_wr32(reg, (cur & mask) + add);
            }
            vm.offset += 11;
        }
        0x5A => {
            // INIT_ZM_REG_INDIRECT: addr(u32) + src(u32)
            let reg = vm.rd32(vm.offset + 1);
            let src = vm.rd32(vm.offset + 5);
            if reg < 0x0100_0000 && src < 0x0100_0000 {
                let val = vm.bar0_rd32(src);
                vm.bar0_wr32(reg, val);
            }
            vm.offset += 9;
        }
        0x5F => {
            // INIT_COPY_NV_REG: 22 bytes total
            vm.offset += 22;
            vm.stats.ops_skipped += 1;
        }

        // ── PLL programming ─────────────────────────────────
        0x79 | 0x4B => {
            vm.offset += 9;
            vm.stats.ops_skipped += 1;
        }
        0x34 => {
            let count = vm.rd08(vm.offset + 9) as usize;
            vm.offset += 10 + count * 4;
            vm.stats.ops_skipped += 1;
        }
        0x4A => {
            let count = vm.rd08(vm.offset + 9) as usize;
            vm.offset += 10 + count * 4;
            vm.stats.ops_skipped += 1;
        }
        0x59 => {
            vm.offset += 13;
            vm.stats.ops_skipped += 1;
        }
        0x87 => {
            let count = ram_restrict_group_count(vm.rom);
            vm.offset += 5 + count * 4;
            vm.stats.ops_skipped += 1;
        }

        // ── RAM-restrict groups ─────────────────────────────
        0x8F => {
            let count = vm.rd08(vm.offset + 5) as usize;
            let n = ram_restrict_group_count(vm.rom);
            vm.offset += 6 + count * n * 4;
            vm.stats.ops_skipped += 1;
        }

        // ── I/O and GPIO opcodes (no-op for VFIO) ───────────
        0x69 => {
            vm.offset += 5;
            vm.stats.ops_skipped += 1;
        }
        0x32 => {
            let count = vm.rd08(vm.offset + 7) as usize;
            vm.offset += 8 + count * 4;
            vm.stats.ops_skipped += 1;
        }
        0x37 => {
            vm.offset += 11;
            vm.stats.ops_skipped += 1;
        }
        0x3B | 0x3C => {
            vm.offset += 5;
            vm.stats.ops_skipped += 1;
        }
        0x49 => {
            let count = vm.rd08(vm.offset + 7) as usize;
            vm.offset += 8 + count * 2;
            vm.stats.ops_skipped += 1;
        }
        0x4C => {
            vm.offset += 7;
            vm.stats.ops_skipped += 1;
        }
        0x4D => {
            vm.offset += 6;
            vm.stats.ops_skipped += 1;
        }
        0x4E => {
            let count = vm.rd08(vm.offset + 4) as usize;
            vm.offset += 5 + count;
            vm.stats.ops_skipped += 1;
        }
        0x4F => {
            vm.offset += 9;
            vm.stats.ops_skipped += 1;
        }
        0x50 => {
            let count = vm.rd08(vm.offset + 3) as usize;
            vm.offset += 4 + count * 2;
            vm.stats.ops_skipped += 1;
        }
        0x51 => {
            vm.offset += 7;
            vm.stats.ops_skipped += 1;
        }
        0x52 => {
            vm.offset += 4;
            vm.stats.ops_skipped += 1;
        }
        0x53 => {
            vm.offset += 3;
            vm.stats.ops_skipped += 1;
        }
        0x54 => {
            let count = vm.rd08(vm.offset + 1) as usize;
            vm.offset += 2 + count * 2;
            vm.stats.ops_skipped += 1;
        }
        0x5E => {
            vm.offset += 6;
            vm.stats.ops_skipped += 1;
        }
        0x62 => {
            vm.offset += 5;
            vm.stats.ops_skipped += 1;
        }
        0x78 => {
            vm.offset += 6;
            vm.stats.ops_skipped += 1;
        }
        0x96 => {
            vm.offset += 11;
            vm.stats.ops_skipped += 1;
        }
        0x98 => {
            vm.offset += 8;
            vm.stats.ops_skipped += 1;
        }
        0x99 => {
            let count = vm.rd08(vm.offset + 5) as usize;
            vm.offset += 6 + count;
            vm.stats.ops_skipped += 1;
        }
        0x9A => {
            vm.offset += 9;
            vm.stats.ops_skipped += 1;
        }
        0xA9 => {
            let count = vm.rd08(vm.offset + 1) as usize;
            vm.offset += 2 + count * 2;
            vm.stats.ops_skipped += 1;
        }

        // ── Hardware-specific no-ops ─────────────────────────
        0x63 => vm.offset += 1,
        0x65 => vm.offset += 3,
        0x66..=0x68 => vm.offset += 1,
        0x6F => vm.offset += 2,
        0x8C | 0x8D | 0x8E | 0x92 | 0xAA => vm.offset += 1,

        // ── Unknown opcodes ─────────────────────────────────
        _ => {
            vm.stats.unknown_opcodes.push((vm.offset, op));
            vm.stats.ops_skipped += 1;
            vm.offset += 1;
            if vm.stats.unknown_opcodes.len() > 100 {
                return Err(DevinitError::InterpreterTooManyUnknownOpcodes {
                    last_offset: vm.offset - 1,
                    last_opcode: op,
                });
            }
        }
    }
    Ok(())
}
