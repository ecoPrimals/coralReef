// SPDX-License-Identifier: AGPL-3.0-or-later
//! Abstractions for loading NVIDIA GR firmware without tying call sites to the filesystem.
//!
//! Production code uses [`FilesystemFirmwareSource`], which reads from
//! `CORALREEF_NVIDIA_FIRMWARE_PATH` / `CORALREEF_NVIDIA_FIRMWARE_ROOT` (see `nvidia_firmware_base`).
//! Tests can inject [`MockFirmwareSource`] to exercise parsing and knowledge-base learning without
//! touching the real firmware tree.

use std::path::{Path, PathBuf};

use super::firmware_parser::GrFirmwareBlobs;

/// Root directory for NVIDIA firmware blobs (`{chip}/gr/`).
///
/// Resolution order: `CORALREEF_NVIDIA_FIRMWARE_PATH`, then
/// `CORALREEF_NVIDIA_FIRMWARE_ROOT`, then `/lib/firmware/nvidia` (aligned with
/// [`GrFirmwareBlobs::parse`](super::firmware_parser::GrFirmwareBlobs::parse) defaults).
#[must_use]
pub fn nvidia_firmware_base() -> PathBuf {
    if let Ok(p) = std::env::var("CORALREEF_NVIDIA_FIRMWARE_PATH") {
        let t = p.trim();
        if !t.is_empty() {
            return PathBuf::from(t.trim_end_matches('/'));
        }
    }
    if let Ok(p) = std::env::var("CORALREEF_NVIDIA_FIRMWARE_ROOT") {
        let t = p.trim();
        if !t.is_empty() {
            return PathBuf::from(t.trim_end_matches('/'));
        }
    }
    PathBuf::from("/lib/firmware/nvidia")
}

/// Discover NVIDIA chip codenames under a firmware root (directories with a `gr/` child).
///
/// Only includes names starting with `g`, `t`, or `a` (chip codenames such as `gv100`, `tu102`),
/// matching the historical discovery filter.
pub(crate) fn discover_nvidia_chips_from(base: &Path) -> Result<Vec<String>, std::io::Error> {
    let entries = std::fs::read_dir(base)?;
    let mut chips = Vec::new();
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('g') || name.starts_with('t') || name.starts_with('a') {
            let gr_dir = entry.path().join("gr");
            if gr_dir.is_dir() {
                chips.push(name);
            }
        }
    }
    chips.sort();
    Ok(chips)
}

/// Abstraction for accessing NVIDIA GPU firmware files.
pub trait NvidiaFirmwareSource {
    /// Load GR firmware blobs for a given chip (e.g. "tu102", "ga102").
    fn load_gr_firmware(&self, chip: &str) -> Result<GrFirmwareBlobs, std::io::Error>;

    /// List chip names that have GR firmware available.
    fn list_chips(&self) -> Result<Vec<String>, std::io::Error>;
}

/// Production firmware source reading from `/lib/firmware/nvidia` (or env override).
pub struct FilesystemFirmwareSource {
    base_path: PathBuf,
}

impl FilesystemFirmwareSource {
    /// Firmware tree from `nvidia_firmware_base()`.
    #[must_use]
    pub fn new() -> Self {
        Self {
            base_path: nvidia_firmware_base(),
        }
    }

    /// Firmware tree rooted at `path` (expected layout: `{path}/{chip}/gr/...`).
    #[must_use]
    pub fn with_path(path: PathBuf) -> Self {
        Self { base_path: path }
    }

    /// Directory containing per-chip firmware packages.
    #[must_use]
    pub fn base_path(&self) -> &Path {
        &self.base_path
    }
}

impl Default for FilesystemFirmwareSource {
    fn default() -> Self {
        Self::new()
    }
}

impl NvidiaFirmwareSource for FilesystemFirmwareSource {
    fn load_gr_firmware(&self, chip: &str) -> Result<GrFirmwareBlobs, std::io::Error> {
        let mut gr = self.base_path.clone();
        gr.push(chip);
        gr.push("gr");
        GrFirmwareBlobs::parse_from(&gr, chip)
    }

    fn list_chips(&self) -> Result<Vec<String>, std::io::Error> {
        discover_nvidia_chips_from(&self.base_path)
    }
}

#[cfg(test)]
use std::collections::HashMap;

/// Test double: in-memory chip list and GR blobs (no filesystem).
#[cfg(test)]
pub(crate) struct MockFirmwareSource {
    listed: Vec<String>,
    blobs: HashMap<String, GrFirmwareBlobs>,
}

#[cfg(test)]
impl MockFirmwareSource {
    /// Empty source: no chips, no blobs.
    #[must_use]
    pub(crate) fn new_empty() -> Self {
        Self {
            listed: Vec::new(),
            blobs: HashMap::new(),
        }
    }

    /// Chips returned by [`NvidiaFirmwareSource::list_chips`] and their parsed blobs.
    #[must_use]
    pub(crate) fn with_blobs(pairs: Vec<(String, GrFirmwareBlobs)>) -> Self {
        let listed: Vec<String> = pairs.iter().map(|(s, _)| s.clone()).collect();
        let blobs: HashMap<_, _> = pairs.into_iter().collect();
        Self { listed, blobs }
    }

    /// Same as [`Self::with_blobs`], but `listed` may name chips that have no blob (load fails).
    #[must_use]
    pub(crate) fn with_listed_and_blobs(
        listed: Vec<String>,
        blobs: HashMap<String, GrFirmwareBlobs>,
    ) -> Self {
        Self { listed, blobs }
    }
}

#[cfg(test)]
impl NvidiaFirmwareSource for MockFirmwareSource {
    fn load_gr_firmware(&self, chip: &str) -> Result<GrFirmwareBlobs, std::io::Error> {
        self.blobs.get(chip).cloned().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("mock firmware missing for chip {chip}"),
            )
        })
    }

    fn list_chips(&self) -> Result<Vec<String>, std::io::Error> {
        Ok(self.listed.clone())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::super::firmware_parser::FirmwareFormat;
    use super::super::knowledge::{GpuKnowledge, GpuVendor};
    use super::*;

    fn bundle_bytes(addr: u32, value: u32) -> Vec<u8> {
        let mut v = Vec::with_capacity(8);
        v.extend_from_slice(&addr.to_le_bytes());
        v.extend_from_slice(&value.to_le_bytes());
        v
    }

    #[test]
    fn learn_nvidia_firmware_mock_three_chips() {
        let tu102 =
            GrFirmwareBlobs::from_legacy_bytes(&bundle_bytes(0x1_0000, 1), &[], &[], &[], "tu102");
        let ga102 = GrFirmwareBlobs::from_legacy_bytes(
            &bundle_bytes(0x0040_0000, 2),
            &[],
            &[],
            &[],
            "ga102",
        );
        let gv100 =
            GrFirmwareBlobs::from_legacy_bytes(&bundle_bytes(0x2_0000, 3), &[], &[], &[], "gv100");
        let source = MockFirmwareSource::with_blobs(vec![
            ("tu102".to_string(), tu102),
            ("ga102".to_string(), ga102),
            ("gv100".to_string(), gv100),
        ]);
        let mut kb = GpuKnowledge::new();
        kb.learn_nvidia_firmware_with_source(&source);

        assert_eq!(kb.known_chips().len(), 3);
        assert!(kb.get("tu102").is_some());
        assert!(kb.get("ga102").is_some());
        assert!(kb.get("gv100").is_some());

        let tu = kb.get("tu102").expect("tu102");
        assert_eq!(tu.format, Some(FirmwareFormat::Legacy));
        assert_eq!(tu.vendor, GpuVendor::Nvidia);
        assert!(tu.gr_blobs.is_some());
        assert!(tu.gr_init.is_some());
    }

    #[test]
    fn learn_nvidia_firmware_mock_empty_source() {
        let source = MockFirmwareSource::new_empty();
        let mut kb = GpuKnowledge::new();
        kb.learn_nvidia_firmware_with_source(&source);
        assert!(kb.known_chips().is_empty());
        let summary = kb.summary();
        assert_eq!(summary.architectures_known, 0);
    }

    #[test]
    fn learn_nvidia_firmware_skips_chip_when_load_fails() {
        let ga102 =
            GrFirmwareBlobs::from_legacy_bytes(&bundle_bytes(0x100, 1), &[], &[], &[], "ga102");
        let source = MockFirmwareSource::with_listed_and_blobs(
            vec!["bad_chip".to_string(), "ga102".to_string()],
            HashMap::from([("ga102".to_string(), ga102)]),
        );
        let mut kb = GpuKnowledge::new();
        kb.learn_nvidia_firmware_with_source(&source);

        assert_eq!(kb.known_chips().len(), 1);
        assert!(kb.get("bad_chip").is_none());
        assert!(kb.get("ga102").is_some());
    }
}
