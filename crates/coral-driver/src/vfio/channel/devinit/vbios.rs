// SPDX-License-Identifier: AGPL-3.0-only
#![expect(missing_docs, reason = "VBIOS parsing; full docs planned")]
//! VBIOS ROM reading and parsing (BIT table, PMU firmware table).

use crate::vfio::device::MappedBar;

/// BAR0 offset of the PROM (Programmable ROM) region.
/// The GPU's internal VBIOS is stored here, separate from the PCI Expansion ROM.
pub const PROM_BASE: usize = 0x0030_0000;

/// PROM enable register — clear bit 0 to enable PROM access.
pub const PROM_ENABLE_REG: usize = 0x0000_1854;

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
    ///   Each entry (6 bytes): id(1) + version(1) + data_size(u16 LE) + data_offset(u16 LE)
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

        tracing::debug!(
            bit_offset = format!("0x{bit_offset:04x}"),
            entry_size,
            entry_count,
            "BIT table located in VBIOS"
        );

        if !(6..=16).contains(&entry_size) || entry_count > 64 {
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
    let p_entry = bit.find(b'p').ok_or("BIT 'p' (PMU) entry not found")?;

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

// ── VBIOS ROM reader ────────────────────────────────────────────────────

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

    tracing::info!(
        bytes = rom.len(),
        kb = rom.len() / 1024,
        "PROM read from BAR0+0x300000"
    );

    Ok(rom)
}

/// Read the VBIOS ROM image via sysfs PCI ROM BAR.
///
/// Note: the sysfs `rom` file provides the PCI Expansion ROM, not the full
/// NVIDIA internal BIOS. It typically lacks the BIT table and PMU firmware.
/// Prefer `read_vbios_prom()` for the full VBIOS.
pub fn read_vbios_sysfs(bdf: &str) -> Result<Vec<u8>, String> {
    let rom_path = crate::linux_paths::sysfs_pci_device_file(bdf, "rom");

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

pub(crate) fn validate_vbios(data: &[u8]) -> Result<Vec<u8>, String> {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn rom_with_bit_at(bit_offset: usize) -> Vec<u8> {
        let mut rom = vec![0u8; bit_offset + 64];
        rom[bit_offset..bit_offset + 5].copy_from_slice(&[0xFF, 0xB8, b'B', b'I', b'T']);
        rom[bit_offset + 9] = 6;
        rom[bit_offset + 10] = 2;
        let e0 = bit_offset + 12;
        rom[e0] = b'I';
        rom[e0 + 1] = 1;
        rom[e0 + 2..e0 + 4].copy_from_slice(&0x0400u16.to_le_bytes());
        rom[e0 + 4..e0 + 6].copy_from_slice(&0x0800u16.to_le_bytes());
        let e1 = e0 + 6;
        rom[e1] = b'p';
        rom[e1 + 1] = 1;
        rom[e1 + 2..e1 + 4].copy_from_slice(&0u16.to_le_bytes());
        rom[e1 + 4..e1 + 6].copy_from_slice(&0u16.to_le_bytes());
        rom
    }

    #[test]
    fn validate_vbios_ok() {
        let mut rom = vec![0u8; 512];
        rom[0] = 0x55;
        rom[1] = 0xAA;
        let out = validate_vbios(&rom).expect("valid");
        assert_eq!(out.len(), 512);
    }

    #[test]
    fn validate_vbios_too_small() {
        assert!(validate_vbios(&[0u8; 4]).is_err());
    }

    #[test]
    fn validate_vbios_bad_sig() {
        let rom = vec![0u8; 512];
        assert!(validate_vbios(&rom).is_err());
    }

    #[test]
    fn bit_table_parse_finds_entries() {
        let rom = rom_with_bit_at(0x200);
        let bit = BitTable::parse(&rom).expect("bit");
        assert_eq!(bit.entries.len(), 2);
        assert!(bit.find(b'I').is_some());
        assert!(bit.find(b'p').is_some());
    }

    #[test]
    fn bit_table_parse_missing_sig() {
        let rom = vec![0u8; 4096];
        assert!(BitTable::parse(&rom).is_err());
    }
}
