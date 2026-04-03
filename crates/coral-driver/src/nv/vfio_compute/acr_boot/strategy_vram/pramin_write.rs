// SPDX-License-Identifier: AGPL-3.0-only

//! PRAMIN chunked writes into VRAM (shared by legacy, native, and dual-phase paths).

use crate::vfio::device::MappedBar;
use crate::vfio::memory::{MemoryRegion, PraminRegion};

/// Write a byte slice to VRAM via PRAMIN in 48 KiB chunks.
pub(crate) fn write_to_vram(
    bar0: &MappedBar,
    vram_addr: u32,
    data: &[u8],
    notes: &mut Vec<String>,
) -> bool {
    let mut off = 0usize;
    while off < data.len() {
        let chunk_vram = vram_addr + off as u32;
        let chunk_size = (data.len() - off).min(0xC000);
        match PraminRegion::new(bar0, chunk_vram, chunk_size) {
            Ok(mut region) => {
                for wo in (0..chunk_size).step_by(4) {
                    let src = off + wo;
                    if src >= data.len() {
                        break;
                    }
                    let end = (src + 4).min(data.len());
                    let mut bytes = [0u8; 4];
                    bytes[..end - src].copy_from_slice(&data[src..end]);
                    if region.write_u32(wo, u32::from_le_bytes(bytes)).is_err() {
                        notes.push(format!("VRAM write failed at {chunk_vram:#x}+{wo:#x}"));
                        return false;
                    }
                }
                off += chunk_size;
            }
            Err(e) => {
                notes.push(format!("PRAMIN at VRAM@{chunk_vram:#x}: {e}"));
                return false;
            }
        }
    }
    true
}
