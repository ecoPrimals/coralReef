// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::vfio::channel::registers::falcon;
use crate::vfio::device::MappedBar;

use super::DEAD_READ;
use super::types::FalconCapabilities;

/// Safe PIO interface for a specific falcon, backed by discovered capabilities.
///
/// All control words are constructed from validated bit layouts. Provides
/// upload, readback, and verification methods that cannot use the wrong format.
pub struct FalconPio<'a> {
    bar0: &'a MappedBar,
    caps: &'a FalconCapabilities,
}

impl<'a> FalconPio<'a> {
    /// Create a new PIO interface for a probed falcon.
    #[must_use]
    pub const fn new(bar0: &'a MappedBar, caps: &'a FalconCapabilities) -> Self {
        Self { bar0, caps }
    }

    /// Upload data to IMEM at `addr`, with optional secure page marking.
    pub fn upload_imem(&self, addr: u32, data: &[u8], secure: bool) {
        let ctrl = if secure {
            self.caps.imem_write_secure_ctrl(addr)
        } else {
            self.caps.imem_write_ctrl(addr)
        };
        let _ = self
            .bar0
            .write_u32(self.caps.base + falcon::IMEMC, ctrl.raw());

        for (i, chunk) in data.chunks(4).enumerate() {
            let byte_offset = (i * 4) as u32;
            if byte_offset & 0xFF == 0 {
                let _ = self
                    .bar0
                    .write_u32(self.caps.base + falcon::IMEMT, (addr + byte_offset) >> 8);
            }
            let _ = self
                .bar0
                .write_u32(self.caps.base + falcon::IMEMD, le_word(chunk));
        }

        // Pad to 256-byte boundary
        let total_bytes = (data.len().div_ceil(4)) * 4;
        let remainder = total_bytes & 0xFF;
        if remainder != 0 {
            let padding_words = (256 - remainder) / 4;
            for _ in 0..padding_words {
                let _ = self.bar0.write_u32(self.caps.base + falcon::IMEMD, 0);
            }
        }
    }

    /// Upload data to DMEM at `addr`.
    pub fn upload_dmem(&self, addr: u32, data: &[u8]) {
        let ctrl = self.caps.dmem_write_ctrl(addr);
        let _ = self
            .bar0
            .write_u32(self.caps.base + falcon::DMEMC, ctrl.raw());

        for chunk in data.chunks(4) {
            let _ = self
                .bar0
                .write_u32(self.caps.base + falcon::DMEMD, le_word(chunk));
        }
    }

    /// Read `count` 32-bit words from IMEM starting at `addr`.
    pub fn read_imem(&self, addr: u32, count: usize) -> Vec<u32> {
        let ctrl = self.caps.imem_read_ctrl(addr);
        let _ = self
            .bar0
            .write_u32(self.caps.base + falcon::IMEMC, ctrl.raw());

        (0..count)
            .map(|_| {
                self.bar0
                    .read_u32(self.caps.base + falcon::IMEMD)
                    .unwrap_or(DEAD_READ)
            })
            .collect()
    }

    /// Read `count` 32-bit words from DMEM starting at `addr`.
    pub fn read_dmem(&self, addr: u32, count: usize) -> Vec<u32> {
        let ctrl = self.caps.dmem_read_ctrl(addr);
        let _ = self
            .bar0
            .write_u32(self.caps.base + falcon::DMEMC, ctrl.raw());

        (0..count)
            .map(|_| {
                self.bar0
                    .read_u32(self.caps.base + falcon::DMEMD)
                    .unwrap_or(DEAD_READ)
            })
            .collect()
    }

    /// Upload IMEM data and verify by readback. Returns number of mismatched words.
    pub fn upload_imem_verified(&self, addr: u32, data: &[u8], secure: bool) -> usize {
        self.upload_imem(addr, data, secure);

        if secure {
            // Secure pages return sentinel on readback — skip verification
            return 0;
        }

        let word_count = data.len().div_ceil(4);
        let readback = self.read_imem(addr, word_count);

        let mut mismatches = 0;
        for (i, chunk) in data.chunks(4).enumerate() {
            let expected = le_word(chunk);
            if i < readback.len() && readback[i] != expected {
                if mismatches < 4 {
                    tracing::warn!(
                        falcon = %self.caps.name,
                        offset = i * 4 + addr as usize,
                        expected = format!("{expected:#010x}"),
                        got = format!("{:#010x}", readback[i]),
                        "IMEM verify mismatch"
                    );
                }
                mismatches += 1;
            }
        }
        mismatches
    }

    /// Upload DMEM data and verify by readback. Returns number of mismatched words.
    pub fn upload_dmem_verified(&self, addr: u32, data: &[u8]) -> usize {
        self.upload_dmem(addr, data);

        let word_count = data.len().div_ceil(4);
        let readback = self.read_dmem(addr, word_count);

        let mut mismatches = 0;
        for (i, chunk) in data.chunks(4).enumerate() {
            let expected = le_word(chunk);
            if i < readback.len() && readback[i] != expected {
                if mismatches < 4 {
                    tracing::warn!(
                        falcon = %self.caps.name,
                        offset = i * 4 + addr as usize,
                        expected = format!("{expected:#010x}"),
                        got = format!("{:#010x}", readback[i]),
                        "DMEM verify mismatch"
                    );
                }
                mismatches += 1;
            }
        }
        mismatches
    }
}

fn le_word(chunk: &[u8]) -> u32 {
    match chunk.len() {
        4 => u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]),
        3 => u32::from_le_bytes([chunk[0], chunk[1], chunk[2], 0]),
        2 => u32::from_le_bytes([chunk[0], chunk[1], 0, 0]),
        1 => u32::from_le_bytes([chunk[0], 0, 0, 0]),
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::le_word;

    #[test]
    fn le_word_full() {
        assert_eq!(le_word(&[0xd0, 0x00, 0x14, 0x00]), 0x001400d0);
    }

    #[test]
    fn le_word_partial() {
        assert_eq!(le_word(&[0x01, 0x02, 0x03]), 0x00030201);
        assert_eq!(le_word(&[0xFF, 0x00]), 0x000000FF);
        assert_eq!(le_word(&[0x42]), 0x00000042);
        assert_eq!(le_word(&[]), 0);
    }
}
