// SPDX-License-Identifier: AGPL-3.0-only
//! Shader Program Header (SPH) — metadata prepended to compiled shaders.
//!
//! The SPH tells the GPU about register usage, shared memory, barriers,
//! and other resources needed to launch a shader.

use bitview::BitMutViewable;

/// SPH size in bytes for SM70+ (18 dwords = 72 bytes).
pub const SIZE_BYTES: usize = 72;

/// SPH builder for constructing shader headers.
#[derive(Debug, Default)]
pub struct SphBuilder {
    num_gprs: u32,
    num_barriers: u32,
    shared_mem_size: u32,
}

impl SphBuilder {
    /// SPH size in bytes for SM70+ (20 dwords = 72 bytes).
    pub const SIZE_BYTES: usize = SIZE_BYTES;

    /// Create a new SPH builder.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the number of GPRs.
    #[must_use]
    pub const fn num_gprs(mut self, n: u32) -> Self {
        self.num_gprs = n;
        self
    }

    /// Set the number of barriers.
    #[must_use]
    pub const fn num_barriers(mut self, n: u32) -> Self {
        self.num_barriers = n;
        self
    }

    /// Set shared memory size in bytes.
    #[must_use]
    pub const fn shared_mem(mut self, bytes: u32) -> Self {
        self.shared_mem_size = bytes;
        self
    }

    /// Encode SPH to binary.
    ///
    /// Produces SM70+ SPH format:
    /// - Dword 0: SPH type (1=compute) and version (3)
    /// - Dword 1: Shader type flags
    /// - Dword 2: GPR count (0 encodes as 8)
    /// - Dword 4: Barrier count and shared memory (in 256-byte units)
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        const SPH_TYPE_COMPUTE: u32 = 1;
        const SPH_VERSION_SM70: u32 = 3;

        // 72 bytes = 18 u32 words
        let mut words = [0u32; 18];

        // Dword 0 (bits 0-31): SPH type [4:0], version [8:5]
        words.set_field(0..5, SPH_TYPE_COMPUTE);
        words.set_field(5..9, SPH_VERSION_SM70);

        // Dword 1 (bits 32-63): Shader type flags (0)
        // Dword 2 (bits 64-95): GPR count [7:0] — 0 means 8
        let gpr_encoded = if self.num_gprs == 8 {
            0u32
        } else {
            self.num_gprs
        };
        words.set_field(64..72, gpr_encoded);

        // Dword 4 (bits 128-159): barriers [20:16], shared_mem [31:21] (in 256-byte units)
        words.set_field(144..149, self.num_barriers);
        let shared_mem_units = self.shared_mem_size / 256;
        words.set_field(149..160, shared_mem_units);

        words.iter().flat_map(|w| w.to_le_bytes()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bitview::BitViewable;

    fn get_field_from_bytes(bytes: &[u8], bit_start: usize, bit_width: usize) -> u64 {
        let words: Vec<u32> = bytes
            .chunks_exact(4)
            .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect();
        words.get_field(bit_start..(bit_start + bit_width))
    }

    #[test]
    fn test_sph_builder_chain() {
        let sph = SphBuilder::new()
            .num_gprs(32)
            .num_barriers(1)
            .shared_mem(49152);
        let encoded = sph.encode();
        assert_eq!(encoded.len(), SphBuilder::SIZE_BYTES);

        // Dword 0: type=1 (compute), version=3
        assert_eq!(get_field_from_bytes(&encoded, 0, 5), 1);
        assert_eq!(get_field_from_bytes(&encoded, 5, 4), 3);

        // Dword 2: GPR count = 32
        assert_eq!(get_field_from_bytes(&encoded, 64, 8), 32);

        // Dword 4: barriers=1, shared_mem=49152/256=192
        assert_eq!(get_field_from_bytes(&encoded, 144, 5), 1);
        assert_eq!(get_field_from_bytes(&encoded, 149, 11), 192);
    }

    #[test]
    fn test_sph_gpr8_encodes_as_zero() {
        let sph = SphBuilder::new().num_gprs(8).num_barriers(0).shared_mem(0);
        let encoded = sph.encode();
        assert_eq!(get_field_from_bytes(&encoded, 64, 8), 0);
    }

    #[test]
    fn test_sph_default() {
        let sph = SphBuilder::default();
        let encoded = sph.encode();
        assert_eq!(encoded.len(), SphBuilder::SIZE_BYTES);
        // Default still has type=1, version=3
        assert_eq!(get_field_from_bytes(&encoded, 0, 5), 1);
        assert_eq!(get_field_from_bytes(&encoded, 5, 4), 3);
    }

    #[test]
    fn test_sph_size_bytes() {
        assert_eq!(SIZE_BYTES, 72);
        assert_eq!(SphBuilder::SIZE_BYTES, 72);
    }

    #[test]
    fn test_sph_debug() {
        let sph = SphBuilder::new().num_gprs(64);
        let dbg = format!("{sph:?}");
        assert!(dbg.contains("SphBuilder"));
    }
}
