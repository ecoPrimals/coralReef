// SPDX-License-Identifier: AGPL-3.0-or-later

use std::collections::BTreeSet;

use super::super::firmware_parser::{FirmwareFormat, GrFirmwareBlobs};
use super::super::gr_init::GrInitSequence;

/// Address space used by firmware init data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum AddressSpace {
    /// FECS method offsets (`0x0000_0000`–`0x0001_FFFF`).
    /// Used by legacy `sw_bundle_init.bin` (Maxwell through Turing).
    /// Data is submitted through FECS falcon channel methods.
    MethodOffset,
    /// Absolute BAR0 MMIO register offsets (`0x0040_0000`–`0x007F_FFFF`).
    /// Used by `NET_img.bin` (Ampere+).
    /// Data can be written directly to BAR0.
    Bar0Mmio,
    /// Unknown/empty.
    Unknown,
}

/// Per-architecture knowledge collected by the sovereign GSP.
#[derive(Debug, Clone)]
pub struct ArchKnowledge {
    /// Chip codename (e.g. "gv100", "ga102").
    pub chip: String,
    /// SM architecture version (e.g. 70 = Volta, 86 = Ampere).
    pub sm: Option<u32>,
    /// Vendor: nvidia, amd.
    pub vendor: GpuVendor,
    /// Has native firmware (GSP or PMU).
    pub has_firmware: bool,
    /// Firmware blob format (Legacy or `NetImg`).
    pub format: Option<FirmwareFormat>,
    /// Address space of the init data.
    pub address_space: AddressSpace,
    /// Parsed GR firmware blobs (if available).
    pub gr_blobs: Option<GrFirmwareBlobs>,
    /// Computed GR init sequence.
    pub gr_init: Option<GrInitSequence>,
    /// Number of unique registers in init sequence.
    pub register_count: usize,
}

/// GPU vendor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuVendor {
    /// NVIDIA (nouveau or proprietary).
    Nvidia,
    /// AMD (amdgpu).
    Amd,
    /// Other/unknown.
    Other,
}

/// Summary statistics for the knowledge base.
#[derive(Debug, Clone, serde::Serialize)]
pub struct KnowledgeSummary {
    pub architectures_known: usize,
    pub with_native_firmware: usize,
    pub needs_sovereign_gsp: usize,
    pub total_unique_registers: usize,
}

/// Register transfer map between two architectures.
#[derive(Debug, Clone, serde::Serialize)]
pub struct RegisterTransferMap {
    /// Chip that teaches (has firmware).
    pub teacher: String,
    /// Chip that learns (needs sovereign GSP).
    pub target: String,
    /// Registers present in both architectures.
    pub common_registers: BTreeSet<u32>,
    /// Registers only in the teacher (new in that generation).
    pub teacher_only_registers: BTreeSet<u32>,
    /// Registers only in the target (must come from target's own firmware).
    pub target_only_registers: BTreeSet<u32>,
}

impl RegisterTransferMap {
    /// Percentage of target registers covered by the teacher.
    #[must_use]
    pub fn coverage_pct(&self) -> f64 {
        let total = self.common_registers.len() + self.target_only_registers.len();
        if total == 0 {
            return 0.0;
        }
        #[expect(
            clippy::cast_precision_loss,
            reason = "coverage percentage; usize→f64 loss acceptable"
        )]
        {
            self.common_registers.len() as f64 / total as f64 * 100.0
        }
    }
}

/// Per-generation statistics for register evolution.
#[derive(Debug, Clone, serde::Serialize)]
pub struct GenerationStats {
    /// SM version (e.g. 52, 60, 70, 75, 86).
    pub sm: u32,
    /// Number of chip variants in this generation.
    pub chip_count: usize,
    /// Representative chip (first alphabetically).
    pub representative: String,
    /// Number of unique registers in init sequence.
    pub unique_registers: usize,
    /// Whether this generation has firmware.
    pub has_firmware: bool,
    /// Register overlap with the previous generation.
    pub overlap_with_previous: Option<usize>,
}
