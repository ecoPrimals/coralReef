// SPDX-License-Identifier: AGPL-3.0-or-later

//! SEC2 EMEM programmed-I/O read/write.

use crate::vfio::channel::registers::falcon;
use crate::vfio::device::MappedBar;

/// Write data to SEC2 EMEM via PIO (always writable, even in HS lockdown).
///
/// nouveau `gp102_flcn_pio_emem_wr_init`: BIT(24) only for write mode.
/// Auto-increment is implicit in the EMEM port hardware.
pub fn sec2_emem_write(bar0: &MappedBar, offset: u32, data: &[u8]) {
    let base = falcon::SEC2_BASE;
    let w = |off: usize, val: u32| {
        let _ = bar0.write_u32(base + off, val);
    };

    // BIT(24) = write mode (nouveau: gp102_flcn_pio_emem_wr_init)
    w(falcon::EMEMC0, (1 << 24) | offset);

    for chunk in data.chunks(4) {
        let word = match chunk.len() {
            4 => u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]),
            3 => u32::from_le_bytes([chunk[0], chunk[1], chunk[2], 0]),
            2 => u32::from_le_bytes([chunk[0], chunk[1], 0, 0]),
            1 => u32::from_le_bytes([chunk[0], 0, 0, 0]),
            _ => 0,
        };
        w(falcon::EMEMD0, word);
    }
}

/// Read back data from SEC2 EMEM via PIO.
///
/// nouveau `gp102_flcn_pio_emem_rd_init`: BIT(25) only for read mode.
pub fn sec2_emem_read(bar0: &MappedBar, offset: u32, len: usize) -> Vec<u32> {
    let base = falcon::SEC2_BASE;
    let w = |off: usize, val: u32| {
        let _ = bar0.write_u32(base + off, val);
    };
    let r = |off: usize| bar0.read_u32(base + off).unwrap_or(0xDEAD_DEAD);

    // BIT(25) = read mode (nouveau: gp102_flcn_pio_emem_rd_init)
    w(falcon::EMEMC0, (1 << 25) | offset);

    let word_count = len.div_ceil(4);
    (0..word_count).map(|_| r(falcon::EMEMD0)).collect()
}

/// Verify EMEM write by reading back and comparing.
pub fn sec2_emem_verify(bar0: &MappedBar, offset: u32, data: &[u8]) -> bool {
    let readback = sec2_emem_read(bar0, offset, data.len());
    for (i, chunk) in data.chunks(4).enumerate() {
        let expected = match chunk.len() {
            4 => u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]),
            3 => u32::from_le_bytes([chunk[0], chunk[1], chunk[2], 0]),
            2 => u32::from_le_bytes([chunk[0], chunk[1], 0, 0]),
            1 => u32::from_le_bytes([chunk[0], 0, 0, 0]),
            _ => 0,
        };
        if i >= readback.len() || readback[i] != expected {
            tracing::error!(
                offset,
                word = i,
                expected = format!("{expected:#010x}"),
                got = format!("{:#010x}", readback.get(i).copied().unwrap_or(0xDEAD)),
                "EMEM verify mismatch"
            );
            return false;
        }
    }
    true
}
