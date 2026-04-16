// SPDX-License-Identifier: AGPL-3.0-or-later
//! Parse NVIDIA GR firmware blobs shipped under `CORALREEF_NVIDIA_FIRMWARE_ROOT` (default
//! `/lib/firmware/nvidia`), in `{chip}/gr/`.
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
use std::sync::OnceLock;

fn nvidia_firmware_root() -> String {
    static ROOT: OnceLock<String> = OnceLock::new();
    ROOT.get_or_init(|| {
        std::env::var("CORALREEF_NVIDIA_FIRMWARE_ROOT")
            .ok()
            .filter(|s| !s.is_empty())
            .map(|s| s.trim_end_matches('/').to_string())
            .unwrap_or_else(|| "/lib/firmware/nvidia".to_string())
    })
    .clone()
}

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
        let mut base = PathBuf::from(nvidia_firmware_root());
        base.push(chip);
        base.push("gr");
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
        let bundle_init = std::fs::read(base.join("sw_bundle_init.bin"))?;
        let method_init = std::fs::read(base.join("sw_method_init.bin"))?;
        let ctx_data = std::fs::read(base.join("sw_ctx.bin")).unwrap_or_else(|_| Vec::new());
        let nonctx_data = std::fs::read(base.join("sw_nonctx.bin")).unwrap_or_else(|_| Vec::new());
        Ok(Self::from_legacy_bytes(
            &bundle_init,
            &method_init,
            &ctx_data,
            &nonctx_data,
            chip,
        ))
    }

    /// Parse legacy GR blobs from in-memory file contents (no filesystem I/O).
    #[must_use]
    pub(crate) fn from_legacy_bytes(
        bundle_init: &[u8],
        method_init: &[u8],
        ctx_data: &[u8],
        nonctx_data: &[u8],
        chip: &str,
    ) -> Self {
        let bundle_pairs = parse_u32_pairs_from_bytes(bundle_init);
        let method_pairs = parse_u32_pairs_from_bytes(method_init);
        Self {
            chip: chip.to_string(),
            format: FirmwareFormat::Legacy,
            bundle_init: bundle_pairs
                .into_iter()
                .map(|(a, v)| BundleEntry { addr: a, value: v })
                .collect(),
            method_init: method_pairs
                .into_iter()
                .map(|(a, v)| MethodEntry { addr: a, value: v })
                .collect(),
            ctx_data: ctx_data.to_vec(),
            nonctx_data: nonctx_data.to_vec(),
        }
    }

    /// Parse Ampere+ `NET_img.bin` container format.
    ///
    /// Format: `[pad: u32, num_sections: u32]` header, then
    /// `[type: u32, size: u32, offset: u32]` per section.
    /// Sections with u32-pair data where first value is in the
    /// GPU register space (`0x0040_0000..0x0080_0000`) are register init.
    fn parse_net_img(path: &Path, chip: &str) -> Result<Self, std::io::Error> {
        let data = std::fs::read(path)?;
        Self::parse_net_img_bytes(&data, chip)
    }

    /// Parse `NET_img.bin` from raw bytes (same layout as [`Self::parse_net_img`]).
    ///
    /// # Errors
    ///
    /// Returns an error if the header is invalid or truncated.
    pub(crate) fn parse_net_img_bytes(data: &[u8], chip: &str) -> Result<Self, std::io::Error> {
        if data.len() < 8 {
            return Err(std::io::Error::other("NET_img.bin too small"));
        }

        let num_sections = read_u32_le(data, 4) as usize;
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
            let sec_type = read_u32_le(data, entry_off);
            let sec_size = read_u32_le(data, entry_off + 4) as usize;
            let sec_offset = read_u32_le(data, entry_off + 8) as usize;

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
pub(crate) fn read_u32_le(data: &[u8], off: usize) -> u32 {
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

/// Parse packed `[u32, u32]` pairs (little-endian). Trailing bytes shorter than 8 are ignored.
#[must_use]
pub(crate) fn parse_u32_pairs_from_bytes(data: &[u8]) -> Vec<(u32, u32)> {
    let mut pairs = Vec::with_capacity(data.len() / 8);

    let mut offset = 0;
    while offset + 8 <= data.len() {
        let addr = read_u32_le(data, offset);
        let value = read_u32_le(data, offset + 4);
        pairs.push((addr, value));
        offset += 8;
    }

    pairs
}

/// Parse a binary file as packed `[u32, u32]` pairs (little-endian).
#[cfg(test)]
fn parse_u32_pairs(path: &Path) -> Result<Vec<(u32, u32)>, std::io::Error> {
    let data = std::fs::read(path)?;
    Ok(parse_u32_pairs_from_bytes(&data))
}

#[cfg(test)]
#[path = "firmware_parser_tests.rs"]
mod tests;
