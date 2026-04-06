// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::vfio::device::MappedBar;
use crate::vfio::memory::{MemoryRegion, PraminRegion};

/// Copy `data` to a VRAM window via PRAMIN so HS-mode falcon DMA can observe it.
pub(super) fn mirror_payload_to_vram(bar0: &MappedBar, vram_addr: u32, data: &[u8]) -> bool {
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
                        return false;
                    }
                }
                off += chunk_size;
            }
            Err(_) => return false,
        }
    }
    true
}
