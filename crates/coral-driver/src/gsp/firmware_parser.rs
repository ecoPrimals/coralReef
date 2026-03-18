// SPDX-License-Identifier: AGPL-3.0-only
//! Parse NVIDIA GR firmware blobs shipped in `/lib/firmware/nvidia/{chip}/gr/`.
//!
//! These blobs contain the register init sequences that nouveau loads into
//! the GPU's FECS/GPCCS falcon engines. The key files are:
//!
//! - `sw_bundle_init.bin`: Register address/value pairs for engine init
//! - `sw_method_init.bin`: Method channel init (class methods)
//! - `sw_ctx.bin`: Context state template
//! - `sw_nonctx.bin`: Non-context register state
//!
//! These exist for every NVIDIA GPU from Maxwell onward, including Volta
//! (GV100). Parsing them gives us the exact register init sequence that
//! nouveau needs to execute — we just need to get the engine to a state
//! where it can accept them.

use std::path::{Path, PathBuf};

/// Firmware format variant discovered for a chip.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FirmwareFormat {
    /// Pre-Ampere: separate `sw_bundle_init.bin`, `sw_method_init.bin`, etc.
    Legacy,
    /// Ampere+: single `NET_img.bin` container with section table.
    NetImg,
}

/// Parsed GR firmware blobs for one GPU architecture.
#[derive(Debug, Clone)]
pub struct GrFirmwareBlobs {
    /// Chip codename (e.g. "gv100", "ga102").
    pub chip: String,
    /// Which firmware container format was parsed.
    pub format: FirmwareFormat,
    /// Register address/value pairs from bundle init.
    pub bundle_init: Vec<BundleEntry>,
    /// Method address/value pairs from method init.
    pub method_init: Vec<MethodEntry>,
    /// Context state template — raw content of `sw_ctx.bin`.
    ///
    /// This is the initial GR context that FECS uses when setting up a
    /// compute channel. Without this template, channels get CTXNOTVALID.
    pub ctx_data: Vec<u8>,
    /// Non-context state — raw content of `sw_nonctx.bin`.
    pub nonctx_data: Vec<u8>,
}

/// A single register write from `sw_bundle_init.bin`.
///
/// The format is packed pairs: `[addr: u32, value: u32]` repeated.
#[derive(Debug, Clone, Copy)]
pub struct BundleEntry {
    /// GPU register address (BAR0-relative).
    pub addr: u32,
    /// Value to write to the register.
    pub value: u32,
}

/// A single method call from `sw_method_init.bin`.
///
/// Format: `[addr: u32, value: u32]` — method address and data.
#[derive(Debug, Clone, Copy)]
pub struct MethodEntry {
    /// FECS method offset (GR class method address).
    pub addr: u32,
    /// Data value to write.
    pub value: u32,
}

impl GrFirmwareBlobs {
    /// Parse GR firmware blobs from the standard firmware directory.
    ///
    /// Automatically detects the format: `NET_img.bin` (Ampere+) or
    /// separate files (Maxwell through Turing).
    ///
    /// # Errors
    /// Returns error if files cannot be read.
    pub fn parse(chip: &str) -> Result<Self, std::io::Error> {
        let base = PathBuf::from(format!("/lib/firmware/nvidia/{chip}/gr"));
        Self::parse_from(&base, chip)
    }

    /// Parse from a specific directory (for testing).
    ///
    /// # Errors
    /// Returns error if files cannot be read.
    pub fn parse_from(base: &Path, chip: &str) -> Result<Self, std::io::Error> {
        let net_img = base.join("NET_img.bin");
        if net_img.exists() {
            return Self::parse_net_img(&net_img, chip);
        }
        Self::parse_legacy(base, chip)
    }

    /// Parse legacy format (separate files).
    fn parse_legacy(base: &Path, chip: &str) -> Result<Self, std::io::Error> {
        let bundle_init = parse_u32_pairs(&base.join("sw_bundle_init.bin"))?;
        let method_init = parse_u32_pairs(&base.join("sw_method_init.bin"))?;

        let ctx_data = std::fs::read(base.join("sw_ctx.bin")).unwrap_or_else(|_| Vec::new());
        let nonctx_data = std::fs::read(base.join("sw_nonctx.bin")).unwrap_or_else(|_| Vec::new());

        Ok(Self {
            chip: chip.to_string(),
            format: FirmwareFormat::Legacy,
            bundle_init: bundle_init
                .into_iter()
                .map(|(a, v)| BundleEntry { addr: a, value: v })
                .collect(),
            method_init: method_init
                .into_iter()
                .map(|(a, v)| MethodEntry { addr: a, value: v })
                .collect(),
            ctx_data,
            nonctx_data,
        })
    }

    /// Parse Ampere+ `NET_img.bin` container format.
    ///
    /// Format: `[pad: u32, num_sections: u32]` header, then
    /// `[type: u32, size: u32, offset: u32]` per section.
    /// Sections with u32-pair data where first value is in the
    /// GPU register space (`0x0040_0000..0x0080_0000`) are register init.
    fn parse_net_img(path: &Path, chip: &str) -> Result<Self, std::io::Error> {
        let data = std::fs::read(path)?;
        if data.len() < 8 {
            return Err(std::io::Error::other("NET_img.bin too small"));
        }

        let num_sections = read_u32_le(&data, 4) as usize;
        let header_size = 8 + num_sections * 12;
        if data.len() < header_size {
            return Err(std::io::Error::other("NET_img.bin header truncated"));
        }

        let mut bundle_entries = Vec::new();
        let mut method_entries = Vec::new();
        let mut ctx_data = Vec::new();
        let mut nonctx_data = Vec::new();

        for i in 0..num_sections {
            let entry_off = 8 + i * 12;
            let sec_type = read_u32_le(&data, entry_off);
            let sec_size = read_u32_le(&data, entry_off + 4) as usize;
            let sec_offset = read_u32_le(&data, entry_off + 8) as usize;

            if sec_size < 8 || sec_offset + sec_size > data.len() {
                continue;
            }

            let chunk = &data[sec_offset..sec_offset + sec_size];

            match sec_type {
                // Register init data: main bundle and per-GPC/TPC sections
                0x05 | 0x30 | 0x31 | 0x32 | 0x33 | 0x24 | 0x25 | 0x26 | 0x27 | 0x28 | 0x29
                | 0x0b | 0x23 | 0x1f | 0x2b | 0x2c | 0x0d | 0x0e | 0x14 => {
                    parse_register_pairs(chunk, &mut bundle_entries);
                }
                // Method init (type 0x07)
                0x07 => {
                    for pair in chunk.chunks_exact(8) {
                        let addr = u32::from_le_bytes([pair[0], pair[1], pair[2], pair[3]]);
                        let value = u32::from_le_bytes([pair[4], pair[5], pair[6], pair[7]]);
                        method_entries.push(MethodEntry { addr, value });
                    }
                }
                // Context sections — store raw content for FECS init
                0x01 => ctx_data = chunk.to_vec(),
                0x03 => nonctx_data = chunk.to_vec(),
                _ => {}
            }
        }

        Ok(Self {
            chip: chip.to_string(),
            format: FirmwareFormat::NetImg,
            bundle_init: bundle_entries,
            method_init: method_entries,
            ctx_data,
            nonctx_data,
        })
    }

    /// Number of bundle init register writes.
    #[must_use]
    pub const fn bundle_count(&self) -> usize {
        self.bundle_init.len()
    }

    /// Number of method init calls.
    #[must_use]
    pub const fn method_count(&self) -> usize {
        self.method_init.len()
    }

    /// Find all bundle entries targeting a specific register address.
    #[must_use]
    pub fn bundle_writes_to(&self, addr: u32) -> Vec<&BundleEntry> {
        self.bundle_init.iter().filter(|e| e.addr == addr).collect()
    }

    /// Context state template size in bytes.
    #[must_use]
    pub const fn ctx_size(&self) -> usize {
        self.ctx_data.len()
    }

    /// Non-context state size in bytes.
    #[must_use]
    pub const fn nonctx_size(&self) -> usize {
        self.nonctx_data.len()
    }

    /// Whether a context template is available for FECS init.
    #[must_use]
    pub const fn has_ctx_template(&self) -> bool {
        !self.ctx_data.is_empty()
    }

    /// Unique register addresses touched by the bundle init.
    #[must_use]
    pub fn unique_bundle_addrs(&self) -> Vec<u32> {
        let mut addrs: Vec<u32> = self.bundle_init.iter().map(|e| e.addr).collect();
        addrs.sort_unstable();
        addrs.dedup();
        addrs
    }
}

/// Read a little-endian u32 at byte offset.
fn read_u32_le(data: &[u8], off: usize) -> u32 {
    u32::from_le_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]])
}

/// Parse register address/value pairs from a raw chunk.
///
/// Only includes pairs where the address looks like a GPU register
/// (`0x0040_0000..0x0080_0000` range, covering GPC/TPC/SM/PPC space).
fn parse_register_pairs(chunk: &[u8], out: &mut Vec<BundleEntry>) {
    for pair in chunk.chunks_exact(8) {
        let addr = u32::from_le_bytes([pair[0], pair[1], pair[2], pair[3]]);
        let value = u32::from_le_bytes([pair[4], pair[5], pair[6], pair[7]]);
        if (0x0040_0000..0x0080_0000).contains(&addr) {
            out.push(BundleEntry { addr, value });
        }
    }
}

/// Parse a binary file as packed `[u32, u32]` pairs (little-endian).
fn parse_u32_pairs(path: &Path) -> Result<Vec<(u32, u32)>, std::io::Error> {
    let data = std::fs::read(path)?;
    let mut pairs = Vec::with_capacity(data.len() / 8);

    let mut offset = 0;
    while offset + 8 <= data.len() {
        let addr = read_u32_le(&data, offset);
        let value = read_u32_le(&data, offset + 4);
        pairs.push((addr, value));
        offset += 8;
    }

    Ok(pairs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_u32_pairs_basic() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.bin");
        // Two pairs: (0x100, 0x42) and (0x200, 0xFF)
        let mut data = Vec::new();
        data.extend_from_slice(&0x100u32.to_le_bytes());
        data.extend_from_slice(&0x42u32.to_le_bytes());
        data.extend_from_slice(&0x200u32.to_le_bytes());
        data.extend_from_slice(&0xFFu32.to_le_bytes());
        std::fs::write(&path, &data).unwrap();

        let pairs = parse_u32_pairs(&path).unwrap();
        assert_eq!(pairs.len(), 2);
        assert_eq!(pairs[0], (0x100, 0x42));
        assert_eq!(pairs[1], (0x200, 0xFF));
    }

    #[test]
    fn legacy_parse_retains_ctx_data() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        std::fs::write(base.join("sw_bundle_init.bin"), []).unwrap();
        std::fs::write(base.join("sw_method_init.bin"), []).unwrap();
        let ctx_content = vec![0xAA_u8; 256];
        std::fs::write(base.join("sw_ctx.bin"), &ctx_content).unwrap();
        let nonctx_content = vec![0xBB_u8; 128];
        std::fs::write(base.join("sw_nonctx.bin"), &nonctx_content).unwrap();

        let blobs = GrFirmwareBlobs::parse_from(base, "test").unwrap();
        assert_eq!(blobs.ctx_data.len(), 256);
        assert_eq!(blobs.ctx_data, ctx_content);
        assert_eq!(blobs.nonctx_data.len(), 128);
        assert_eq!(blobs.nonctx_data, nonctx_content);
        assert!(blobs.has_ctx_template());
        assert_eq!(blobs.ctx_size(), 256);
        assert_eq!(blobs.nonctx_size(), 128);
    }

    #[test]
    fn missing_ctx_produces_empty() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        std::fs::write(base.join("sw_bundle_init.bin"), []).unwrap();
        std::fs::write(base.join("sw_method_init.bin"), []).unwrap();

        let blobs = GrFirmwareBlobs::parse_from(base, "test").unwrap();
        assert!(blobs.ctx_data.is_empty());
        assert!(!blobs.has_ctx_template());
    }

    #[test]
    fn parse_real_gv100_firmware() {
        match GrFirmwareBlobs::parse("gv100") {
            Ok(blobs) => {
                assert_eq!(blobs.chip, "gv100");
                assert_eq!(blobs.format, FirmwareFormat::Legacy);
                assert!(blobs.bundle_count() > 0);
                assert!(blobs.method_count() > 0);
                eprintln!(
                    "GV100 (legacy): {} bundle writes, {} method inits, ctx={}B, nonctx={}B",
                    blobs.bundle_count(),
                    blobs.method_count(),
                    blobs.ctx_data.len(),
                    blobs.nonctx_data.len()
                );
                let addrs = blobs.unique_bundle_addrs();
                eprintln!("  {} unique registers", addrs.len());
            }
            Err(e) => {
                eprintln!("GV100 firmware not present (expected in CI): {e}");
            }
        }
    }

    #[test]
    fn parse_real_ga102_net_img() {
        match GrFirmwareBlobs::parse("ga102") {
            Ok(blobs) => {
                assert_eq!(blobs.chip, "ga102");
                assert_eq!(blobs.format, FirmwareFormat::NetImg);
                assert!(
                    blobs.bundle_count() > 0,
                    "GA102 NET_img should have register init data"
                );
                let addrs = blobs.unique_bundle_addrs();
                eprintln!(
                    "GA102 (NET_img): {} bundle writes ({} unique regs), {} method inits, ctx={}B, nonctx={}B",
                    blobs.bundle_count(),
                    addrs.len(),
                    blobs.method_count(),
                    blobs.ctx_data.len(),
                    blobs.nonctx_data.len()
                );
            }
            Err(e) => {
                eprintln!("GA102 firmware not present: {e}");
            }
        }
    }

    #[test]
    fn parse_net_img_synthetic() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("NET_img.bin");

        // Build synthetic NET_img: header [pad, num_sections=2], then 2 sections
        // Section 0: type 0x05 (register init), size 8, offset 32
        // Section 1: type 0x07 (method init), size 8, offset 40
        let mut data = Vec::new();
        data.extend_from_slice(&0u32.to_le_bytes()); // pad
        data.extend_from_slice(&2u32.to_le_bytes()); // num_sections

        // Section 0: type 0x05, size 8, offset 32
        data.extend_from_slice(&0x05u32.to_le_bytes());
        data.extend_from_slice(&8u32.to_le_bytes());
        data.extend_from_slice(&32u32.to_le_bytes());

        // Section 1: type 0x07, size 8, offset 40
        data.extend_from_slice(&0x07u32.to_le_bytes());
        data.extend_from_slice(&8u32.to_le_bytes());
        data.extend_from_slice(&40u32.to_le_bytes());

        // Pad to offset 32, then section 0 data: one register pair in GPU range
        let reg_addr = 0x0040_1000u32; // in 0x0040_0000..0x0080_0000
        let reg_val = 0xDEAD_BEEFu32;
        data.resize(32, 0);
        data.extend_from_slice(&reg_addr.to_le_bytes());
        data.extend_from_slice(&reg_val.to_le_bytes());

        // Section 1 data: method pair at offset 40
        let method_addr = 0x0100u32;
        let method_val = 0x42u32;
        data.extend_from_slice(&method_addr.to_le_bytes());
        data.extend_from_slice(&method_val.to_le_bytes());

        std::fs::write(&path, &data).unwrap();

        let blobs = GrFirmwareBlobs::parse_from(dir.path(), "ga102").unwrap();
        assert_eq!(blobs.chip, "ga102");
        assert_eq!(blobs.format, FirmwareFormat::NetImg);
        assert_eq!(blobs.bundle_init.len(), 1);
        assert_eq!(blobs.bundle_init[0].addr, reg_addr);
        assert_eq!(blobs.bundle_init[0].value, reg_val);
        assert_eq!(blobs.method_init.len(), 1);
        assert_eq!(blobs.method_init[0].addr, method_addr);
        assert_eq!(blobs.method_init[0].value, method_val);
    }

    #[test]
    fn parse_net_img_too_small() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("NET_img.bin");
        std::fs::write(&path, &[0u8; 4]).unwrap();
        let result = GrFirmwareBlobs::parse_from(dir.path(), "ga102");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("too small"));
    }

    #[test]
    fn parse_net_img_ctx_and_nonctx_sections() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("NET_img.bin");

        let mut data = Vec::new();
        data.extend_from_slice(&0u32.to_le_bytes());
        data.extend_from_slice(&0x04u32.to_le_bytes()); // 4 sections

        // Section 0: type 0x01 (ctx), size 64, offset 56
        for v in &[0x01u32, 64u32, 56u32] {
            data.extend_from_slice(&v.to_le_bytes());
        }
        // Section 1: type 0x03 (nonctx), size 32, offset 120
        for v in &[0x03u32, 32u32, 120u32] {
            data.extend_from_slice(&v.to_le_bytes());
        }
        // Section 2: type 0x05 (register), size 8, offset 152
        for v in &[0x05u32, 8u32, 152u32] {
            data.extend_from_slice(&v.to_le_bytes());
        }
        // Section 3: type 0x07 (method), size 0, offset 0
        for v in &[0x07u32, 0u32, 0u32] {
            data.extend_from_slice(&v.to_le_bytes());
        }

        data.resize(56, 0);
        data.extend_from_slice(&[0xAAu8; 64]); // ctx data
        data.extend_from_slice(&[0xBBu8; 32]); // nonctx data
        data.extend_from_slice(&0x0040_2000u32.to_le_bytes());
        data.extend_from_slice(&0x1234u32.to_le_bytes());

        std::fs::write(&path, &data).unwrap();

        let blobs = GrFirmwareBlobs::parse_from(dir.path(), "ga102").unwrap();
        assert_eq!(blobs.ctx_data.len(), 64);
        assert_eq!(blobs.ctx_data[0], 0xAA);
        assert_eq!(blobs.nonctx_data.len(), 32);
        assert_eq!(blobs.nonctx_data[0], 0xBB);
        assert!(blobs.has_ctx_template());
    }

    #[test]
    fn parse_u32_pairs_odd_length_ignores_trailing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.bin");
        // 8 bytes (one pair) + 4 bytes trailing
        let mut data = Vec::new();
        data.extend_from_slice(&0x100u32.to_le_bytes());
        data.extend_from_slice(&0x42u32.to_le_bytes());
        data.extend_from_slice(&0xFFu32.to_le_bytes());
        std::fs::write(&path, &data).unwrap();

        let pairs = parse_u32_pairs(&path).unwrap();
        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0], (0x100, 0x42));
    }

    #[test]
    fn bundle_writes_to_and_unique_addrs() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        let mut bundle_data = Vec::new();
        for &(addr, val) in &[(0x0040_1000u32, 1u32), (0x0040_2000u32, 2u32), (0x0040_1000u32, 3u32)] {
            bundle_data.extend_from_slice(&addr.to_le_bytes());
            bundle_data.extend_from_slice(&val.to_le_bytes());
        }
        std::fs::write(base.join("sw_bundle_init.bin"), &bundle_data).unwrap();
        std::fs::write(base.join("sw_method_init.bin"), []).unwrap();

        let blobs = GrFirmwareBlobs::parse_from(base, "test").unwrap();
        let to_1000 = blobs.bundle_writes_to(0x0040_1000);
        assert_eq!(to_1000.len(), 2);
        assert_eq!(to_1000[0].value, 1);
        assert_eq!(to_1000[1].value, 3);

        let addrs = blobs.unique_bundle_addrs();
        assert_eq!(addrs.len(), 2);
        assert_eq!(addrs[0], 0x0040_1000);
        assert_eq!(addrs[1], 0x0040_2000);
    }

    #[test]
    fn parse_all_available_firmware() {
        let base = std::path::Path::new("/lib/firmware/nvidia");
        let Ok(entries) = std::fs::read_dir(base) else {
            eprintln!("No NVIDIA firmware directory");
            return;
        };
        let mut chips: Vec<String> = entries
            .flatten()
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                if e.path().join("gr").is_dir() {
                    Some(name)
                } else {
                    None
                }
            })
            .collect();
        chips.sort();

        for chip in &chips {
            match GrFirmwareBlobs::parse(chip) {
                Ok(blobs) => {
                    eprintln!(
                        "{chip:8} ({:?}): {:5} bundle, {:5} method, {:4} unique regs",
                        blobs.format,
                        blobs.bundle_count(),
                        blobs.method_count(),
                        blobs.unique_bundle_addrs().len()
                    );
                }
                Err(e) => eprintln!("{chip:8}: parse error: {e}"),
            }
        }
    }
}
