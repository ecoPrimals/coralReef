// SPDX-License-Identifier: AGPL-3.0-only
//! Shader Program Header (SPH) — metadata prepended to compiled shaders.
//!
//! The SPH tells the GPU about register usage, shared memory, barriers,
//! and other resources needed to launch a shader.

/// SPH builder for constructing shader headers.
#[derive(Debug, Default)]
pub struct SphBuilder {
    num_gprs: u32,
    num_barriers: u32,
    shared_mem_size: u32,
}

impl SphBuilder {
    /// Create a new SPH builder.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the number of GPRs.
    #[must_use]
    pub fn num_gprs(mut self, n: u32) -> Self {
        self.num_gprs = n;
        self
    }

    /// Set the number of barriers.
    #[must_use]
    pub fn num_barriers(mut self, n: u32) -> Self {
        self.num_barriers = n;
        self
    }

    /// Set shared memory size in bytes.
    #[must_use]
    pub fn shared_mem(mut self, bytes: u32) -> Self {
        self.shared_mem_size = bytes;
        self
    }

    /// Encode SPH to binary (stub — returns empty for now).
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let _ = (self.num_gprs, self.num_barriers, self.shared_mem_size);
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sph_builder_chain() {
        let sph = SphBuilder::new()
            .num_gprs(32)
            .num_barriers(1)
            .shared_mem(49152);
        let encoded = sph.encode();
        assert!(encoded.is_empty(), "stub returns empty for now");
    }

    #[test]
    fn test_sph_default() {
        let sph = SphBuilder::default();
        let encoded = sph.encode();
        assert!(encoded.is_empty());
    }

    #[test]
    fn test_sph_debug() {
        let sph = SphBuilder::new().num_gprs(64);
        let dbg = format!("{sph:?}");
        assert!(dbg.contains("SphBuilder"));
    }
}
