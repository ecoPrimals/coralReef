// SPDX-License-Identifier: AGPL-3.0-only

//! Falcon IMEM/DMEM programmed-I/O upload helpers (Nouveau-style chunking).

use crate::vfio::channel::registers::falcon;
use crate::vfio::device::MappedBar;

/// Upload code to falcon IMEM matching Nouveau's per-256B-chunk protocol.
///
/// Nouveau re-initializes IMEMC for each 256-byte page. This is critical —
/// auto-increment may not cross page boundaries on all falcon versions.
/// Upload code to falcon IMEM with optional SECURE flag.
///
/// When `secure` is true, bit 28 is set in IMEMC, marking the IMEM region
/// as accessible only in HS mode. Nouveau sets secure=true for the HS code
/// section of ACR firmware (`gm200_flcn_pio_imem_wr_init`).
pub fn falcon_imem_upload_secure(
    bar0: &MappedBar,
    base: usize,
    imem_addr: u32,
    data: &[u8],
    start_tag: u32,
    secure: bool,
) {
    let w = |off: usize, val: u32| {
        let _ = bar0.write_u32(base + off, val);
    };
    let sec_bit = if secure { 1u32 << 28 } else { 0 };

    for (chunk_idx, chunk) in data.chunks(256).enumerate() {
        let chunk_addr = imem_addr + (chunk_idx as u32) * 256;
        let chunk_tag = start_tag + chunk_idx as u32;

        w(falcon::IMEMC, sec_bit | (1u32 << 24) | chunk_addr);

        // Set tag for this page (Nouveau: gm200_flcn_pio_imem_wr)
        w(falcon::IMEMT, chunk_tag);

        // Write data words
        for word_chunk in chunk.chunks(4) {
            let word = match word_chunk.len() {
                4 => {
                    u32::from_le_bytes([word_chunk[0], word_chunk[1], word_chunk[2], word_chunk[3]])
                }
                3 => u32::from_le_bytes([word_chunk[0], word_chunk[1], word_chunk[2], 0]),
                2 => u32::from_le_bytes([word_chunk[0], word_chunk[1], 0, 0]),
                1 => u32::from_le_bytes([word_chunk[0], 0, 0, 0]),
                _ => 0,
            };
            w(falcon::IMEMD, word);
        }

        // Pad remainder of 256-byte page with zeroes
        let written = (chunk.len().div_ceil(4)) * 4;
        let remainder = written & 0xFF;
        if remainder != 0 {
            let pad_words = (256 - remainder) / 4;
            for _ in 0..pad_words {
                w(falcon::IMEMD, 0);
            }
        }
    }
}

/// Upload code to falcon IMEM (non-secure). Convenience wrapper.
pub fn falcon_imem_upload_nouveau(
    bar0: &MappedBar,
    base: usize,
    imem_addr: u32,
    data: &[u8],
    start_tag: u32,
) {
    falcon_imem_upload_secure(bar0, base, imem_addr, data, start_tag, false);
}

/// Read SEC2 DMEM contents via PIO.
pub(crate) fn sec2_dmem_read(bar0: &MappedBar, offset: u32, len: usize) -> Vec<u32> {
    let base = falcon::SEC2_BASE;
    let w = |off: usize, val: u32| {
        let _ = bar0.write_u32(base + off, val);
    };
    let r = |off: usize| bar0.read_u32(base + off).unwrap_or(0xDEAD_DEAD);

    // DMEMC: BIT(25) = read mode with auto-increment
    w(falcon::DMEMC, (1u32 << 25) | offset);

    let words = len.div_ceil(4);
    let mut result = Vec::with_capacity(words);
    for _ in 0..words {
        result.push(r(falcon::DMEMD));
    }
    result
}

/// Upload data to a Falcon engine's DMEM via PIO (programmed I/O).
pub fn falcon_dmem_upload(bar0: &MappedBar, base: usize, dmem_addr: u32, data: &[u8]) {
    let w = |off: usize, val: u32| {
        let _ = bar0.write_u32(base + off, val);
    };

    // DMEMC: BIT(24) = write mode with auto-increment
    w(falcon::DMEMC, (1u32 << 24) | dmem_addr);

    for chunk in data.chunks(4) {
        let word = match chunk.len() {
            4 => u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]),
            3 => u32::from_le_bytes([chunk[0], chunk[1], chunk[2], 0]),
            2 => u32::from_le_bytes([chunk[0], chunk[1], 0, 0]),
            1 => u32::from_le_bytes([chunk[0], 0, 0, 0]),
            _ => 0,
        };
        w(falcon::DMEMD, word);
    }
}
