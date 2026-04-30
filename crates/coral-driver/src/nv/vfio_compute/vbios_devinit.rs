// SPDX-License-Identifier: AGPL-3.0-or-later
//! VBIOS DEVINIT script interpreter — reads and executes Kepler GPU initialization scripts.

use crate::vfio::device::MappedBar;

/// Thin wrapper to adapt `&MappedBar` (which uses `&self` for writes)
/// to the `RegisterAccess` trait (which requires `&mut self` for writes).
#[expect(
    dead_code,
    reason = "kept for VBIOS devinit and future unguarded paths"
)]
struct MappedBarRegAccess<'a>(&'a MappedBar);

impl crate::gsp::RegisterAccess for MappedBarRegAccess<'_> {
    fn read_u32(&self, offset: u32) -> Result<u32, crate::gsp::ApplyError> {
        self.0
            .read_u32(offset as usize)
            .map_err(|e| crate::gsp::ApplyError::MmioFailed {
                offset,
                detail: e.to_string(),
            })
    }

    fn write_u32(&mut self, offset: u32, value: u32) -> Result<(), crate::gsp::ApplyError> {
        self.0
            .write_u32(offset as usize, value)
            .map_err(|e| crate::gsp::ApplyError::MmioFailed {
                offset,
                detail: e.to_string(),
            })
    }
}

/// `RegisterAccess` adapter routing through `GuardedBar` — writes go through
/// the blocklist/canary checks, reads through the link-alive check.
#[expect(
    dead_code,
    reason = "WIP: guarded VBIOS opcode paths share kepler_fecs_boot adapter"
)]
struct GuardedBarRegAccess<'a>(&'a super::hardware_guard::GuardedBar<'a>);

impl crate::gsp::RegisterAccess for GuardedBarRegAccess<'_> {
    fn read_u32(&self, offset: u32) -> Result<u32, crate::gsp::ApplyError> {
        self.0
            .read_u32(offset)
            .map_err(|refusal| crate::gsp::ApplyError::MmioFailed {
                offset,
                detail: refusal.to_string(),
            })
    }

    fn write_u32(&mut self, offset: u32, value: u32) -> Result<(), crate::gsp::ApplyError> {
        self.0
            .write_u32(offset, value)
            .map_err(|refusal| crate::gsp::ApplyError::MmioFailed {
                offset,
                detail: refusal.to_string(),
            })
    }
}

/// Read and execute VBIOS DEVINIT scripts to enable Kepler clock domains.
pub(super) fn kepler_vbios_devinit(bar0: &MappedBar) {
    const PROM_BASE: usize = 0x0030_0000;
    const PROM_ENABLE: usize = 0x1854;

    let enable_reg = bar0.read_u32(PROM_ENABLE).unwrap_or(0);
    let _ = bar0.write_u32(PROM_ENABLE, enable_reg & !1);

    let sig = bar0.read_u32(PROM_BASE).unwrap_or(0);
    if sig & 0xFFFF != 0xAA55 {
        tracing::warn!(
            sig = format_args!("{sig:#010x}"),
            "VBIOS PROM signature missing — skipping DEVINIT"
        );
        return;
    }

    let blocks = ((sig >> 16) & 0xFF) as usize;
    let image_size = if blocks > 0 { blocks * 512 } else { 64 * 1024 };
    let read_size = image_size.clamp(256 * 1024, 512 * 1024);

    let mut rom = Vec::with_capacity(read_size);
    for off in (0..read_size).step_by(4) {
        let word = bar0.read_u32(PROM_BASE + off).unwrap_or(0xFFFF_FFFF);
        rom.extend_from_slice(&word.to_le_bytes());
        if off > image_size && word == 0xFFFF_FFFF {
            let next = bar0.read_u32(PROM_BASE + off + 4).unwrap_or(0xFFFF_FFFF);
            if next == 0xFFFF_FFFF {
                break;
            }
        }
    }

    let _ = bar0.write_u32(PROM_ENABLE, enable_reg);
    tracing::info!(rom_size = rom.len(), "VBIOS ROM read from PROM");

    let (scripts, ops, writes) = interpret_devinit_scripts(bar0, &rom);
    tracing::info!(scripts, ops, writes, "VBIOS DEVINIT complete");

    std::thread::sleep(std::time::Duration::from_millis(100));
}

fn interpret_devinit_scripts(bar0: &MappedBar, rom: &[u8]) -> (usize, usize, usize) {
    let rd08 = |off: usize| -> u8 { rom.get(off).copied().unwrap_or(0) };
    let rd16 = |off: usize| -> u16 {
        if off + 2 <= rom.len() {
            u16::from_le_bytes([rom[off], rom[off + 1]])
        } else {
            0
        }
    };
    let rd32 = |off: usize| -> u32 {
        if off + 4 <= rom.len() {
            u32::from_le_bytes([rom[off], rom[off + 1], rom[off + 2], rom[off + 3]])
        } else {
            0
        }
    };

    let sig: &[u8] = &[0xFF, 0xB8, b'B', b'I', b'T'];
    let Some(bit_off) = rom.windows(sig.len()).position(|w| w == sig) else {
        tracing::warn!("BIT signature not found in VBIOS ROM");
        return (0, 0, 0);
    };

    let entry_size = rom[bit_off + 9] as usize;
    let entry_count = rom[bit_off + 10] as usize;
    let entries_start = bit_off + 12;

    if entry_size < 6 {
        tracing::warn!(entry_size, "BIT entry size too small");
        return (0, 0, 0);
    }

    let mut i_data_off = 0usize;
    for i in 0..entry_count {
        let e = entries_start + i * entry_size;
        if e + 6 > rom.len() {
            break;
        }
        if rom[e] == b'I' {
            i_data_off = rd16(e + 4) as usize;
        }
    }

    if i_data_off == 0 || i_data_off + 2 > rom.len() {
        tracing::warn!("BIT 'I' entry not found");
        return (0, 0, 0);
    }

    let script_table = rd16(i_data_off) as usize;
    let cond_table = if i_data_off + 8 <= rom.len() {
        rd16(i_data_off + 6) as usize
    } else {
        0
    };

    if script_table == 0 || script_table >= rom.len() {
        tracing::warn!("DEVINIT script table pointer invalid");
        return (0, 0, 0);
    }

    let mut total_scripts = 0usize;
    let mut total_ops = 0usize;
    let mut total_writes = 0usize;
    let mut script_idx = 0;

    loop {
        let entry_off = script_table + script_idx * 2;
        if entry_off + 2 > rom.len() {
            break;
        }
        let script_off = rd16(entry_off) as usize;
        if script_off == 0 || script_off >= rom.len() {
            break;
        }

        let (ops, writes) =
            execute_devinit_script(bar0, rom, script_off, cond_table, &rd08, &rd16, &rd32);
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

/// Read RAM restrict group count from BIT 'M' entry byte at m_off+2.
/// Falls back to 4 (the most common value for GDDR5 Kepler).
/// Program a PLL to the requested frequency (in kHz).
///
/// Kepler PLLs have the layout:
///   reg+0x00: PLL_CTRL (enable, bypass, etc.)
///   reg+0x04: PLL_COEF (M, N, P dividers)
///   reg+0x14: PLL_STAT (lock status)
///
/// Reference oscillator on K80 is 27000 kHz.
/// Freq = ref_khz * N / M / P
fn devinit_pll_set(bar0: &MappedBar, reg: u32, freq_khz: u32) {
    const REF_KHZ: u32 = 27000;

    if freq_khz == 0 {
        return;
    }

    let bar_read = |r: u32| -> u32 { bar0.read_u32(r as usize).unwrap_or(0) };
    let bar_write = |r: u32, v: u32| {
        let _ = bar0.write_u32(r as usize, v);
    };

    // Compute M/N/P: target = ref * N / M / 2^P
    // For simplicity: M=1, P=0, N = freq/ref (clamped)
    // Better: M=1, find N and P such that VCO is in range [1200MHz, 2500MHz]
    let mut best_n = 1u32;
    let mut best_m = 1u32;
    let mut best_p = 0u32;
    let mut best_err = u32::MAX;

    for p in 0..7u32 {
        let target_vco = freq_khz << p;
        if !(1_200_000..=2_700_000).contains(&target_vco) {
            continue;
        }
        for m in 1..=13u32 {
            let n = (target_vco * m + REF_KHZ / 2) / REF_KHZ;
            if !(1..=255).contains(&n) {
                continue;
            }
            let actual = (REF_KHZ * n / m) >> p;
            let err = actual.abs_diff(freq_khz);
            if err < best_err {
                best_err = err;
                best_n = n;
                best_m = m;
                best_p = p;
            }
        }
    }

    // GK110 PLL_COEF format: [7:0]=M, [15:8]=N, [20:16]=P
    let coef = best_m | (best_n << 8) | (best_p << 16);
    let actual_khz = (REF_KHZ * best_n / best_m) >> best_p;

    tracing::info!(
        reg = format_args!("{reg:#010x}"),
        target_khz = freq_khz,
        actual_khz,
        m = best_m,
        n = best_n,
        p = best_p,
        coef = format_args!("{coef:#010x}"),
        "PLL programming"
    );

    // PLL enable sequence (nouveau gf100_pll_set):
    // 1. Disable PLL (clear enable bit)
    bar_write(reg, bar_read(reg) & !0x1);
    // 2. Write coefficients
    bar_write(reg + 4, coef);
    // 3. Enable PLL (set enable bit)
    bar_write(reg, bar_read(reg) | 0x1);
    std::thread::sleep(std::time::Duration::from_millis(10));

    // Check if PLL locked (bit 31 of reg+0x14)
    let stat = bar_read(reg + 0x14);
    let locked = stat & 0x8000_0000 != 0;
    tracing::info!(
        reg = format_args!("{reg:#010x}"),
        stat = format_args!("{stat:#010x}"),
        locked,
        "PLL lock status"
    );
}

fn ram_restrict_group_count(rom: &[u8]) -> usize {
    let sig: &[u8] = &[0xFF, 0xB8, b'B', b'I', b'T'];
    let Some(bit_off) = rom.windows(sig.len()).position(|w| w == sig) else {
        return 4;
    };
    if bit_off + 12 >= rom.len() {
        return 4;
    }
    let entry_size = rom[bit_off + 9] as usize;
    let entry_count = rom[bit_off + 10] as usize;
    if entry_size < 6 {
        return 4;
    }
    let entries_start = bit_off + 12;
    for i in 0..entry_count {
        let e = entries_start + i * entry_size;
        if e + 6 > rom.len() {
            break;
        }
        if rom[e] == b'M' {
            let m_data_off = u16::from_le_bytes([rom[e + 4], rom[e + 5]]) as usize;
            if m_data_off + 3 <= rom.len() {
                let count = rom[m_data_off + 2] as usize;
                if count > 0 && count <= 16 {
                    return count;
                }
            }
        }
    }
    4
}

/// Read the RAM restrict strap from NV_PEXTDEV_BOOT0 (0x101000).
/// On cold VFIO K80s, PEXTDEV may be PRI-faulted; return 0 (default strap).
fn ram_restrict_strap(bar_read: &dyn Fn(u32) -> u32) -> usize {
    let pextdev = bar_read(0x0010_1000);
    if pextdev == 0xFFFF_FFFF || pextdev == 0xBADF_1200 {
        0
    } else {
        ((pextdev >> 2) & 0xF) as usize
    }
}

fn execute_devinit_script(
    bar0: &MappedBar,
    rom: &[u8],
    start: usize,
    cond_table: usize,
    rd08: &dyn Fn(usize) -> u8,
    rd16: &dyn Fn(usize) -> u16,
    rd32: &dyn Fn(usize) -> u32,
) -> (usize, usize) {
    let bar_read = |reg: u32| -> u32 { bar0.read_u32(reg as usize).unwrap_or(0) };
    let bar_write = |reg: u32, val: u32| {
        tracing::trace!(
            reg = format_args!("{reg:#010x}"),
            val = format_args!("{val:#010x}"),
            "DEVINIT write"
        );
        let _ = bar0.write_u32(reg as usize, val);
    };

    let mut off = start;
    let mut execute = true;
    let mut ops = 0usize;
    let mut writes = 0usize;
    let max_ops = 50_000;

    while off != 0 && off < rom.len() && ops < max_ops {
        let op = rd08(off);
        ops += 1;

        match op {
            0x71 => {
                off = 0;
            }
            0x72 => {
                execute = true;
                off += 1;
            }
            0x38 => {
                execute = !execute;
                off += 1;
            }
            0x7A => {
                let reg = rd32(off + 1);
                let val = rd32(off + 5);
                if execute && reg < 0x0100_0000 {
                    bar_write(reg, val);
                    writes += 1;
                }
                off += 9;
            }
            0x6E => {
                let reg = rd32(off + 1);
                let mask = rd32(off + 5);
                let val = rd32(off + 9);
                if execute && reg < 0x0100_0000 {
                    let cur = bar_read(reg);
                    bar_write(reg, (cur & mask) | val);
                    writes += 1;
                }
                off += 13;
            }
            0x58 | 0x91 => {
                let base = rd32(off + 1);
                let count = rd08(off + 5) as usize;
                off += 6;
                for i in 0..count {
                    if off + 4 > rom.len() {
                        break;
                    }
                    let val = rd32(off);
                    let reg = base + (i as u32) * 4;
                    if execute && reg < 0x0100_0000 {
                        bar_write(reg, val);
                        writes += 1;
                    }
                    off += 4;
                }
            }
            0x77 => {
                let reg = rd32(off + 1);
                let val = rd16(off + 5) as u32;
                if execute && reg < 0x0100_0000 {
                    bar_write(reg, val);
                    writes += 1;
                }
                off += 7;
            }
            0x47 => {
                let reg = rd32(off + 1);
                let mask = rd32(off + 5);
                if execute && reg < 0x0100_0000 {
                    let cur = bar_read(reg);
                    bar_write(reg, cur & !mask);
                    writes += 1;
                }
                off += 9;
            }
            0x48 => {
                let reg = rd32(off + 1);
                let val = rd32(off + 5);
                if execute && reg < 0x0100_0000 {
                    let cur = bar_read(reg);
                    bar_write(reg, cur | val);
                    writes += 1;
                }
                off += 9;
            }
            0x74 | 0x57 => {
                let usec = rd16(off + 1) as u64;
                if execute && usec > 0 {
                    std::thread::sleep(std::time::Duration::from_micros(usec.min(100_000)));
                }
                off += 3;
            }
            0x75 => {
                let cond = rd08(off + 1);
                if cond_table != 0 {
                    let e = cond_table + (cond as usize) * 12;
                    if e + 12 <= rom.len() {
                        let reg = rd32(e);
                        let mask = rd32(e + 4);
                        let val = rd32(e + 8);
                        if reg != 0 && (bar_read(reg) & mask) != val {
                            execute = false;
                        }
                    }
                }
                off += 2;
            }
            0x56 => {
                let cond = rd08(off + 1);
                let retries = rd08(off + 2).max(1);
                let delay = rd16(off + 3) as u64;
                if cond_table != 0 {
                    let e = cond_table + (cond as usize) * 12;
                    let mut met = false;
                    for _ in 0..retries {
                        if e + 12 <= rom.len() {
                            let reg = rd32(e);
                            let mask = rd32(e + 4);
                            let val = rd32(e + 8);
                            if (bar_read(reg) & mask) == val {
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
            0x6D => {
                let mask = rd08(off + 1);
                let val = rd08(off + 2);
                if (bar_read(0x101000) as u8 & mask) != val {
                    execute = false;
                }
                off += 3;
            }
            0x97 => {
                let reg = rd32(off + 1);
                let mask = rd32(off + 5);
                let add = rd08(off + 9) as u32;
                if execute && reg < 0x0100_0000 {
                    bar_write(reg, (bar_read(reg) & mask) + add);
                    writes += 1;
                }
                off += 11;
            }
            0x5C => {
                let target = rd16(off + 1) as usize;
                off = if target > 0 && target < rom.len() {
                    target
                } else {
                    0
                };
            }
            0x73 => {
                off += 3;
            }
            0x33 => {
                off += 2;
            }
            0x36 => {
                off += 1;
            }
            0x5B => {
                off += 3;
            }
            0x6B => {
                off += 2;
            }
            0x76 | 0x39 => {
                off += 2;
            }
            0x3A => {
                let sz = rd08(off + 2) as usize;
                off += 3 + sz;
            }

            // ── PLL opcodes (nouveau init.c) ─────────────────
            // 0x79 INIT_PLL: reg(u32) + freq_10khz(u16) = 7 bytes
            0x79 => {
                let reg = rd32(off + 1);
                let freq_10k = rd16(off + 5) as u32;
                let freq_khz = freq_10k * 10;
                tracing::info!(
                    reg = format_args!("{reg:#010x}"),
                    freq_khz,
                    "DEVINIT INIT_PLL (0x79)"
                );
                if execute && reg < 0x0100_0000 {
                    devinit_pll_set(bar0, reg, freq_khz);
                    writes += 1;
                }
                off += 7;
            }
            // 0x4B INIT_PLL2: reg(u32) + freq_khz(u32) = 9 bytes
            0x4B => {
                let reg = rd32(off + 1);
                let freq = rd32(off + 5);
                tracing::info!(
                    reg = format_args!("{reg:#010x}"),
                    freq_khz = freq,
                    "DEVINIT INIT_PLL2 (0x4B)"
                );
                if execute && reg < 0x0100_0000 {
                    devinit_pll_set(bar0, reg, freq);
                    writes += 1;
                }
                off += 9;
            }

            // 0x49 INDEX_ADDRESS_LATCHED: creg(u32) + dreg(u32) +
            //   mask(u32) + data(u32) + count(u8) = 18 byte header,
            //   then count * 2 bytes of (iaddr, idata) pairs.
            // Semantics: for each pair, write idata to dreg, then
            //   write (read(creg) & mask) | data | iaddr to creg.
            0x49 => {
                let creg = rd32(off + 1);
                let dreg = rd32(off + 5);
                let mask = rd32(off + 9);
                let data = rd32(off + 13);
                let count = rd08(off + 17) as usize;
                off += 18;
                for _ in 0..count {
                    if off + 2 > rom.len() {
                        break;
                    }
                    let iaddr = rd08(off) as u32;
                    let idata = rd08(off + 1) as u32;
                    off += 2;
                    if execute && creg < 0x0100_0000 && dreg < 0x0100_0000 {
                        bar_write(dreg, idata);
                        let cur = bar_read(creg);
                        bar_write(creg, (cur & mask) | data | iaddr);
                        writes += 2;
                    }
                }
            }

            // 0x87 RAM_RESTRICT_PLL: type(u8) = 2 byte header,
            //   then group_count * 4 bytes of freq values.
            //   Calls init_prog_pll for the strap-selected frequency.
            0x87 => {
                let pll_type = rd08(off + 1);
                let grp_count = ram_restrict_group_count(rom);
                tracing::debug!(
                    pll_type,
                    grp_count,
                    "DEVINIT RAM_RESTRICT_PLL (0x87) — skipped (handled by clock recipe)"
                );
                off += 2 + grp_count * 4;
            }

            // 0x8F RAM_RESTRICT_ZM_REG_GROUP: addr(u32) + incr(u8) +
            //   num(u8) = 7 byte header, then num * group_count * 4 bytes.
            //   Writes register values selected by RAM strap.
            0x8F => {
                let addr = rd32(off + 1);
                let incr = rd08(off + 5) as u32;
                let num = rd08(off + 6) as usize;
                let grp_count = ram_restrict_group_count(rom);
                let strap = ram_restrict_strap(&bar_read);
                off += 7;
                for i in 0..num {
                    let target_reg = addr + (i as u32) * incr;
                    for j in 0..grp_count {
                        if off + 4 > rom.len() {
                            break;
                        }
                        let val = rd32(off);
                        off += 4;
                        if j == strap && execute && target_reg < 0x0100_0000 {
                            bar_write(target_reg, val);
                            writes += 1;
                        }
                    }
                }
            }

            // 0x90 COPY_ZM_REG: sreg(u32) + dreg(u32) = 9 bytes.
            0x90 => {
                let sreg = rd32(off + 1);
                let dreg = rd32(off + 5);
                if execute && sreg < 0x0100_0000 && dreg < 0x0100_0000 {
                    let val = bar_read(sreg);
                    bar_write(dreg, val);
                    writes += 1;
                }
                off += 9;
            }

            // 0x45 IO_RESTRICT_PLL: port(u16)+idx(u8)+mask(u8)+shift(u8)+
            //   flag(u8)+count(u8)+reg(u32) = 12 byte header, then count*2
            0x45 => {
                let count = rd08(off + 7) as usize;
                let reg = rd32(off + 8);
                tracing::debug!(
                    reg = format_args!("{reg:#010x}"),
                    count,
                    "DEVINIT IO_RESTRICT_PLL (0x45) — skipped (handled by clock recipe)"
                );
                off += 12 + count * 2;
            }

            0x34 | 0x4A => {
                let c = rd08(off + 9) as usize;
                off += 10 + c * 4;
            }
            0x59 => {
                off += 13;
            }
            0x69 => {
                off += 5;
            }
            0x32 => {
                let c = rd08(off + 7) as usize;
                off += 8 + c * 4;
            }
            0x37 => {
                off += 11;
            }
            0x3B | 0x3C => {
                off += 5;
            }
            0x4C => {
                off += 7;
            }
            0x4D => {
                off += 6;
            }
            0x4E => {
                let c = rd08(off + 4) as usize;
                off += 5 + c;
            }
            0x4F => {
                off += 9;
            }
            0x50 => {
                let c = rd08(off + 3) as usize;
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
                let c = rd08(off + 1) as usize;
                off += 2 + c * 2;
            }
            0x5A => {
                off += 9;
            }
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
            0x96 => {
                off += 11;
            }
            0x98 => {
                off += 8;
            }
            0x99 => {
                let c = rd08(off + 5) as usize;
                off += 6 + c;
            }
            0x9A => {
                off += 9;
            }
            0xA9 => {
                let c = rd08(off + 1) as usize;
                off += 2 + c * 2;
            }
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
                tracing::debug!(
                    opcode = format_args!("{op:#04x}"),
                    off,
                    "unknown DEVINIT opcode — stopping script"
                );
                off = 0;
            }
        }
    }

    (ops, writes)
}
