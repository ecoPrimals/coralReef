// SPDX-License-Identifier: AGPL-3.0-only
//! VBIOS PROM read and BIT init script interpreter for Exp 123-K.

use coral_driver::gsp::RegisterAccess;
use coral_driver::nv::bar0::Bar0Access;

/// Read the VBIOS PROM from BAR0 using `Bar0Access`.
pub fn read_vbios_prom(bar0: &mut Bar0Access) -> Vec<u8> {
    const PROM_BASE: u32 = 0x0030_0000;

    // Enable PROM access
    let enable_reg = bar0.read_u32(0x1854).unwrap_or(0);
    let _ = bar0.write_u32(0x1854, enable_reg & !1);

    let sig = bar0.read_u32(PROM_BASE).unwrap_or(0);
    assert_eq!(sig & 0xFFFF, 0xAA55, "VBIOS PROM signature missing");

    let blocks = ((sig >> 16) & 0xFF) as usize;
    let image_size = if blocks > 0 { blocks * 512 } else { 64 * 1024 };
    let read_size = image_size.clamp(256 * 1024, 512 * 1024);

    let mut rom = Vec::with_capacity(read_size);
    for off in (0..read_size).step_by(4) {
        let word = bar0.read_u32(PROM_BASE + off as u32).unwrap_or(0xFFFF_FFFF);
        if off > image_size && word == 0xFFFF_FFFF {
            let next = bar0
                .read_u32(PROM_BASE + off as u32 + 4)
                .unwrap_or(0xFFFF_FFFF);
            if next == 0xFFFF_FFFF {
                break;
            }
        }
        rom.extend_from_slice(&word.to_le_bytes());
    }

    let _ = bar0.write_u32(0x1854, enable_reg);
    rom
}

/// Find BIT table in VBIOS ROM. Returns (init_tables_base, condition_table_offset).
pub fn find_bit_init_tables(rom: &[u8]) -> (usize, usize) {
    let sig: &[u8] = &[0xFF, 0xB8, b'B', b'I', b'T'];
    let bit_off = rom
        .windows(sig.len())
        .position(|w| w == sig)
        .expect("BIT signature not found");

    let entry_size = rom[bit_off + 9] as usize;
    let entry_count = rom[bit_off + 10] as usize;
    let entries_start = bit_off + 12;

    eprintln!("  BIT at {bit_off:#06x}: {entry_count} entries of {entry_size} bytes");

    let mut i_data_off = 0usize;
    for i in 0..entry_count {
        let e = entries_start + i * entry_size;
        if e + 6 > rom.len() {
            break;
        }
        let id = rom[e];
        let data_off = u16::from_le_bytes([rom[e + 4], rom[e + 5]]) as usize;
        let data_sz = u16::from_le_bytes([rom[e + 2], rom[e + 3]]);
        if id != 0 {
            eprintln!(
                "    '{}' (v{}) data_off={data_off:#06x} size={data_sz}",
                id as char,
                rom[e + 1]
            );
        }
        if id == b'I' {
            i_data_off = data_off;
        }
    }

    assert!(
        i_data_off > 0 && i_data_off + 2 <= rom.len(),
        "BIT 'I' not found"
    );
    // BIT 'I' data layout (u16 pointers):
    // [0] init script table, [2] macro index, [4] macro, [6] condition table, ...
    let script_list_ptr = u16::from_le_bytes([rom[i_data_off], rom[i_data_off + 1]]) as usize;
    let cond_table = if i_data_off + 8 <= rom.len() {
        u16::from_le_bytes([rom[i_data_off + 6], rom[i_data_off + 7]]) as usize
    } else {
        0
    };
    eprintln!("  script_list_ptr={script_list_ptr:#06x}  cond_table={cond_table:#06x}");
    (script_list_ptr, cond_table)
}

/// Minimal VBIOS init script interpreter using `Bar0Access`.
pub fn interpret_vbios_scripts(bar0: &mut Bar0Access, rom: &[u8]) -> (usize, usize, usize) {
    let (init_tables_base, cond_table) = find_bit_init_tables(rom);
    // init_tables_base is a pointer to a list of u16 script pointers (direct from BIT 'I' offset 0)
    let script_table = init_tables_base;

    let rd08 = |rom: &[u8], off: usize| -> u8 { rom.get(off).copied().unwrap_or(0) };
    let rd16 = |rom: &[u8], off: usize| -> u16 {
        if off + 2 <= rom.len() {
            u16::from_le_bytes([rom[off], rom[off + 1]])
        } else {
            0
        }
    };
    let rd32 = |rom: &[u8], off: usize| -> u32 {
        if off + 4 <= rom.len() {
            u32::from_le_bytes([rom[off], rom[off + 1], rom[off + 2], rom[off + 3]])
        } else {
            0
        }
    };

    let mut total_ops = 0usize;
    let mut total_writes = 0usize;
    let mut total_scripts = 0usize;

    let mut script_idx = 0;
    loop {
        let entry_off = script_table + script_idx * 2;
        if entry_off + 2 > rom.len() {
            break;
        }
        let script_off = rd16(rom, entry_off) as usize;
        if script_off == 0 || script_off >= rom.len() {
            break;
        }

        eprintln!("    Script {script_idx} at {script_off:#06x}");
        let mut off = script_off;
        let mut execute = true;
        let mut ops = 0usize;
        let mut writes = 0usize;
        let max_ops = 50_000;

        while off != 0 && ops < max_ops {
            let op = rd08(rom, off);
            ops += 1;

            match op {
                0x71 => {
                    off = 0;
                } // DONE
                0x72 => {
                    execute = true;
                    off += 1;
                } // RESUME
                0x38 => {
                    execute = !execute;
                    off += 1;
                } // NOT
                0x7A => {
                    // ZM_REG: reg(u32) + val(u32)
                    let reg = rd32(rom, off + 1);
                    let val = rd32(rom, off + 5);
                    if execute && reg < 0x0100_0000 {
                        let _ = bar0.write_u32(reg, val);
                        writes += 1;
                    }
                    off += 9;
                }
                0x6E => {
                    // NV_REG: reg(u32) + mask(u32) + val(u32)
                    let reg = rd32(rom, off + 1);
                    let mask = rd32(rom, off + 5);
                    let val = rd32(rom, off + 9);
                    if execute && reg < 0x0100_0000 {
                        let cur = bar0.read_u32(reg).unwrap_or(0);
                        let _ = bar0.write_u32(reg, (cur & mask) | val);
                        writes += 1;
                    }
                    off += 13;
                }
                0x58 | 0x91 => {
                    // ZM_REG_SEQUENCE / ZM_REG_GROUP
                    let base = rd32(rom, off + 1);
                    let count = rd08(rom, off + 5) as usize;
                    off += 6;
                    for i in 0..count {
                        if off + 4 > rom.len() {
                            break;
                        }
                        let val = rd32(rom, off);
                        let reg = base + (i as u32) * 4;
                        if execute && reg < 0x0100_0000 {
                            let _ = bar0.write_u32(reg, val);
                            writes += 1;
                        }
                        off += 4;
                    }
                }
                0x77 => {
                    // ZM_REG16
                    let reg = rd32(rom, off + 1);
                    let val = rd16(rom, off + 5) as u32;
                    if execute && reg < 0x0100_0000 {
                        let _ = bar0.write_u32(reg, val);
                        writes += 1;
                    }
                    off += 7;
                }
                0x47 => {
                    // ANDN_REG
                    let reg = rd32(rom, off + 1);
                    let mask = rd32(rom, off + 5);
                    if execute && reg < 0x0100_0000 {
                        let cur = bar0.read_u32(reg).unwrap_or(0);
                        let _ = bar0.write_u32(reg, cur & !mask);
                        writes += 1;
                    }
                    off += 9;
                }
                0x48 => {
                    // OR_REG
                    let reg = rd32(rom, off + 1);
                    let val = rd32(rom, off + 5);
                    if execute && reg < 0x0100_0000 {
                        let cur = bar0.read_u32(reg).unwrap_or(0);
                        let _ = bar0.write_u32(reg, cur | val);
                        writes += 1;
                    }
                    off += 9;
                }
                0x74 | 0x57 => {
                    // TIME / LTIME
                    let usec = rd16(rom, off + 1) as u64;
                    if execute && usec > 0 {
                        std::thread::sleep(std::time::Duration::from_micros(usec.min(100_000)));
                    }
                    off += 3;
                }
                0x75 => {
                    // CONDITION
                    let cond = rd08(rom, off + 1);
                    if cond_table != 0 {
                        let e = cond_table + (cond as usize) * 12;
                        if e + 12 <= rom.len() {
                            let reg = rd32(rom, e);
                            let mask = rd32(rom, e + 4);
                            let val = rd32(rom, e + 8);
                            if reg != 0 {
                                let actual = bar0.read_u32(reg).unwrap_or(0);
                                if (actual & mask) != val {
                                    execute = false;
                                }
                            }
                        }
                    }
                    off += 2;
                }
                0x56 => {
                    // CONDITION_TIME
                    let cond = rd08(rom, off + 1);
                    let retries = rd08(rom, off + 2).max(1);
                    let delay = rd16(rom, off + 3) as u64;
                    if cond_table != 0 {
                        let e = cond_table + (cond as usize) * 12;
                        let mut met = false;
                        for _ in 0..retries {
                            if e + 12 <= rom.len() {
                                let reg = rd32(rom, e);
                                let mask = rd32(rom, e + 4);
                                let val = rd32(rom, e + 8);
                                let actual = bar0.read_u32(reg).unwrap_or(0);
                                if (actual & mask) == val {
                                    met = true;
                                    break;
                                }
                            }
                            std::thread::sleep(std::time::Duration::from_micros(delay));
                        }
                        if !met {
                            execute = false;
                        }
                    }
                    off += 5;
                }
                0x73 => {
                    off += 3;
                } // STRAP_CONDITION (skip)
                0x6D => {
                    // RAM_CONDITION
                    let mask = rd08(rom, off + 1);
                    let val = rd08(rom, off + 2);
                    let strap = bar0.read_u32(0x101000).unwrap_or(0) as u8;
                    if (strap & mask) != val {
                        execute = false;
                    }
                    off += 3;
                }
                0x33 => {
                    off += 2;
                } // REPEAT
                0x36 => {
                    off += 1;
                } // END_REPEAT
                0x5C => {
                    // JUMP
                    let target = rd16(rom, off + 1) as usize;
                    off = if target > 0 && target < rom.len() {
                        target
                    } else {
                        0
                    };
                }
                0x5B => {
                    off += 3;
                } // SUB_DIRECT (skip for simplicity)
                0x6B => {
                    off += 2;
                } // SUB (skip)
                0x76 | 0x39 => {
                    off += 2;
                } // IO_CONDITION / IO_FLAG_CONDITION
                0x3A => {
                    let sz = rd08(rom, off + 2) as usize;
                    off += 3 + sz;
                }
                // PLL opcodes (skip)
                0x79 | 0x4B => {
                    off += 9;
                }
                0x34 | 0x4A => {
                    let c = rd08(rom, off + 9) as usize;
                    off += 10 + c * 4;
                }
                0x59 => {
                    off += 13;
                }
                // I/O and GPIO (skip)
                0x69 => {
                    off += 5;
                }
                0x32 => {
                    let c = rd08(rom, off + 7) as usize;
                    off += 8 + c * 4;
                }
                0x37 => {
                    off += 11;
                }
                0x3B | 0x3C => {
                    off += 5;
                }
                0x49 => {
                    let c = rd08(rom, off + 7) as usize;
                    off += 8 + c * 2;
                }
                0x4C => {
                    off += 7;
                }
                0x4D => {
                    off += 6;
                }
                0x4E => {
                    let c = rd08(rom, off + 4) as usize;
                    off += 5 + c;
                }
                0x4F => {
                    off += 9;
                }
                0x50 => {
                    let c = rd08(rom, off + 3) as usize;
                    off += 4 + c * 2;
                }
                0x51 => {
                    off += 7;
                }
                0x52 => {
                    off += 4;
                }
                0x53 => {
                    off += 3;
                }
                0x54 => {
                    let c = rd08(rom, off + 1) as usize;
                    off += 2 + c * 2;
                }
                0x5A => {
                    off += 9;
                } // ZM_REG_INDIRECT
                0x5E => {
                    off += 6;
                }
                0x5F => {
                    off += 22;
                }
                0x62 => {
                    off += 5;
                }
                0x78 => {
                    off += 6;
                }
                0x87 => {
                    off += 5 + 4 * 4;
                } // simplified RAM_RESTRICT
                0x8F => {
                    let c = rd08(rom, off + 5) as usize;
                    off += 6 + c * 4 * 4;
                }
                0x90 => {
                    off += 9;
                }
                0x96 => {
                    off += 11;
                }
                0x97 => {
                    // ZM_MASK_ADD
                    let reg = rd32(rom, off + 1);
                    let mask = rd32(rom, off + 5);
                    let add = rd08(rom, off + 9) as u32;
                    if execute && reg < 0x0100_0000 {
                        let cur = bar0.read_u32(reg).unwrap_or(0);
                        let _ = bar0.write_u32(reg, (cur & mask) + add);
                        writes += 1;
                    }
                    off += 11;
                }
                0x98 => {
                    off += 8;
                }
                0x99 => {
                    let c = rd08(rom, off + 5) as usize;
                    off += 6 + c;
                }
                0x9A => {
                    off += 9;
                }
                0xA9 => {
                    let c = rd08(rom, off + 1) as usize;
                    off += 2 + c * 2;
                }
                // No-ops
                0x63 | 0x66..=0x68 | 0x8C..=0x8E | 0x92 | 0xAA => {
                    off += 1;
                }
                0x65 => {
                    off += 3;
                }
                0x6F => {
                    off += 2;
                }
                _ => {
                    eprintln!("    Unknown opcode {op:#04x} at {off:#06x}, stopping script");
                    off = 0;
                }
            }
        }
        eprintln!("    Script {script_idx}: {ops} ops, {writes} writes");
        total_ops += ops;
        total_writes += writes;
        total_scripts += 1;
        script_idx += 1;
        if script_idx > 50 {
            break;
        }
    }
    (total_scripts, total_ops, total_writes)
}
