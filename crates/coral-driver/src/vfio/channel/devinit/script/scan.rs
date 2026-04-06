// SPDX-License-Identifier: AGPL-3.0-or-later
//! VBIOS init script scanner — extracts register writes without full interpretation.
//!
//! Reference: nouveau nvkm/subdev/bios/init.c (Ben Skeggs, Red Hat)

use super::super::vbios::BitTable;

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
                            rom_offset: pos,
                            opcode: op,
                            reg,
                            value: val,
                            mask: None,
                        });
                    }
                    pos += 9;
                } else {
                    break;
                }
            }
            // init_nv_reg — opcode 0x6E: addr(u32) + mask(u32) + value(u32) = 13 bytes
            0x6E => {
                if pos + 13 <= end {
                    let reg = r32(pos + 1);
                    let mask = r32(pos + 5);
                    let val = r32(pos + 9);
                    if reg < 0x0100_0000 {
                        writes.push(ScriptRegWrite {
                            rom_offset: pos,
                            opcode: op,
                            reg,
                            value: val,
                            mask: Some(mask),
                        });
                    }
                    pos += 13;
                } else {
                    break;
                }
            }
            // init_zm_reg_sequence — opcode 0x58: base(u32) + count(u8) then count × u32
            0x58 => {
                if pos + 6 <= end {
                    let base = r32(pos + 1);
                    let count = rom[pos + 5] as usize;
                    pos += 6;
                    for i in 0..count {
                        if pos + 4 > end {
                            break;
                        }
                        let val = r32(pos);
                        let reg = base + (i as u32) * 4;
                        if reg < 0x0100_0000 {
                            writes.push(ScriptRegWrite {
                                rom_offset: pos,
                                opcode: 0x58,
                                reg,
                                value: val,
                                mask: None,
                            });
                        }
                        pos += 4;
                    }
                } else {
                    break;
                }
            }
            // init_zm_reg16 — opcode 0x77: addr(u32) + data(u16) = 7 bytes
            0x77 => {
                if pos + 7 <= end {
                    let reg = r32(pos + 1);
                    let val = u16::from_le_bytes([rom[pos + 5], rom[pos + 6]]) as u32;
                    if reg < 0x0100_0000 {
                        writes.push(ScriptRegWrite {
                            rom_offset: pos,
                            opcode: op,
                            reg,
                            value: val,
                            mask: None,
                        });
                    }
                    pos += 7;
                } else {
                    break;
                }
            }
            // Fixed-size opcodes we can skip
            0x71 | 0x72 | 0x63 | 0x8C | 0x8D | 0x8E | 0x92 | 0xAA => {
                pos += 1;
            }
            0x75 | 0x6F | 0x6B | 0x33 => {
                pos += 2;
            }
            0x5B | 0x5C | 0x73 | 0x76 | 0x66 | 0x6D | 0x65 => {
                pos += 3;
            }
            0x56 | 0x74 | 0x69 | 0x9B => {
                pos += 5;
            }
            0x57 => {
                pos += 3;
            }
            0x5A | 0x90 => {
                pos += 9;
            }
            0x78 => {
                pos += 6;
            }
            0x79 => {
                pos += 7;
            }
            0x97 => {
                pos += 11;
            }
            0x5F => {
                pos += 22;
            }
            // Unknown opcode — advance by 1 and hope for the best
            _ => {
                pos += 1;
            }
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

    tracing::debug!(
        script_off = format!("{script_off:#06x}"),
        script_len,
        "scanning boot scripts for register writes"
    );

    let writes = scan_init_script_writes(rom, script_off, script_len);
    tracing::debug!(
        count = writes.len(),
        "register writes found in boot scripts"
    );

    Ok(writes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_zm_reg_opcode() {
        let mut rom = vec![0u8; 32];
        rom[0] = 0x7A;
        rom[1..5].copy_from_slice(&0x0010_2000u32.to_le_bytes());
        rom[5..9].copy_from_slice(&0xCAFE_BEEFu32.to_le_bytes());
        let w = scan_init_script_writes(&rom, 0, rom.len());
        assert_eq!(w.len(), 1);
        assert_eq!(w[0].opcode, 0x7A);
        assert_eq!(w[0].reg, 0x0010_2000);
        assert_eq!(w[0].value, 0xCAFE_BEEF);
        assert!(w[0].mask.is_none());
    }

    #[test]
    fn scan_nv_reg_opcode_with_mask() {
        let mut rom = vec![0u8; 32];
        rom[0] = 0x6E;
        rom[1..5].copy_from_slice(&0x0000_1000u32.to_le_bytes());
        rom[5..9].copy_from_slice(&0x0000_FFFFu32.to_le_bytes());
        rom[9..13].copy_from_slice(&0x0000_00ABu32.to_le_bytes());
        let w = scan_init_script_writes(&rom, 0, rom.len());
        assert_eq!(w.len(), 1);
        assert_eq!(w[0].mask, Some(0xFFFF));
        assert_eq!(w[0].value, 0xAB);
    }

    #[test]
    fn scan_zm_reg_sequence() {
        let mut rom = vec![0u8; 32];
        rom[0] = 0x58;
        rom[1..5].copy_from_slice(&0x1000u32.to_le_bytes());
        rom[5] = 2;
        rom[6..10].copy_from_slice(&0x1111u32.to_le_bytes());
        rom[10..14].copy_from_slice(&0x2222u32.to_le_bytes());
        let w = scan_init_script_writes(&rom, 0, rom.len());
        assert_eq!(w.len(), 2);
        assert_eq!(w[0].reg, 0x1000);
        assert_eq!(w[0].value, 0x1111);
        assert_eq!(w[1].reg, 0x1004);
        assert_eq!(w[1].value, 0x2222);
    }

    #[test]
    fn scan_skips_bar0_addresses_above_limit() {
        let mut rom = vec![0u8; 16];
        rom[0] = 0x7A;
        rom[1..5].copy_from_slice(&0x0200_0000u32.to_le_bytes());
        rom[5..9].copy_from_slice(&1u32.to_le_bytes());
        let w = scan_init_script_writes(&rom, 0, rom.len());
        assert!(w.is_empty());
    }
}
