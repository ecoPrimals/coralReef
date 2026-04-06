// SPDX-License-Identifier: AGPL-3.0-or-later

/// Firmware component status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FwStatus {
    /// Firmware files found.
    Present,
    /// Firmware files missing.
    Missing,
}

impl FwStatus {
    /// Returns `true` if firmware files were found.
    #[must_use]
    pub const fn is_present(self) -> bool {
        matches!(self, Self::Present)
    }
}

/// Structured firmware inventory for an NVIDIA GPU.
///
/// Probes `/lib/firmware/nvidia/{chip}/` for each subsystem. Desktop Volta
/// (GV100) is missing PMU firmware, which blocks nouveau compute dispatch.
/// Ampere+ GPUs may use GSP firmware as a substitute.
#[derive(Debug, Clone)]
pub struct FirmwareInventory {
    /// Chip name used for probing (e.g. "gv100", "ga102").
    pub chip: String,
    /// Application Context Runtime — signed boot firmware.
    pub acr: FwStatus,
    /// Graphics/Compute engine firmware (FECS, GPCCS, context).
    pub gr: FwStatus,
    /// Security Engine v2 — secure boot chain.
    pub sec2: FwStatus,
    /// Video decode engine.
    pub nvdec: FwStatus,
    /// Power Management Unit — required for compute channel init.
    pub pmu: FwStatus,
    /// GPU System Processor — Ampere+ substitute for PMU.
    pub gsp: FwStatus,
}

impl FirmwareInventory {
    /// Whether nouveau can likely initialize a compute channel.
    ///
    /// Requires either PMU firmware (Volta/Turing) or GSP firmware (Ampere+).
    /// GR firmware is always required for compute.
    #[must_use]
    pub const fn compute_viable(&self) -> bool {
        self.gr.is_present() && (self.pmu.is_present() || self.gsp.is_present())
    }

    /// Human-readable summary of missing components blocking compute.
    #[must_use]
    pub fn compute_blockers(&self) -> Vec<&'static str> {
        let mut blockers = Vec::new();
        if !self.gr.is_present() {
            blockers.push("GR (graphics/compute engine)");
        }
        if !self.pmu.is_present() && !self.gsp.is_present() {
            blockers.push("PMU or GSP (compute init firmware)");
        }
        blockers
    }
}

/// Probe firmware inventory for an NVIDIA GPU chip.
///
/// Checks `/lib/firmware/nvidia/{chip}/` for each subsystem directory.
/// A subsystem is marked `Present` if at least one firmware file exists.
#[must_use]
pub fn firmware_inventory(chip: &str) -> FirmwareInventory {
    let base = format!("/lib/firmware/nvidia/{chip}");
    let probe = |subdir: &str, files: &[&str]| -> FwStatus {
        if files
            .iter()
            .any(|f| std::path::Path::new(&format!("{base}/{subdir}/{f}")).exists())
        {
            FwStatus::Present
        } else {
            FwStatus::Missing
        }
    };

    FirmwareInventory {
        chip: chip.to_owned(),
        acr: probe("acr", &["bl.bin", "ucode_unload.bin"]),
        gr: probe(
            "gr",
            &["fecs_bl.bin", "fecs_inst.bin", "gpccs_bl.bin", "sw_ctx.bin"],
        ),
        sec2: probe("sec2", &["desc.bin", "image.bin", "sig.bin"]),
        nvdec: probe("nvdec", &["scrubber.bin"]),
        pmu: probe("pmu", &["bl.bin", "inst.bin", "data.bin", "sig.bin"]),
        gsp: probe(
            "gsp",
            &[
                "booter_load-535.113.01.bin",
                "bootloader-535.113.01.bin",
                "gsp-535.113.01.bin",
            ],
        ),
    }
}

/// Check for NVIDIA firmware files required by nouveau for compute on Volta+.
///
/// Returns a list of (path, exists) for the firmware files that nouveau
/// typically needs. For structured results, use [`firmware_inventory`] instead.
#[must_use]
pub fn check_nouveau_firmware(chip: &str) -> Vec<(String, bool)> {
    let base = format!("/lib/firmware/nvidia/{chip}");
    let firmware_files = [
        "acr/bl.bin",
        "acr/ucode_unload.bin",
        "gr/fecs_bl.bin",
        "gr/fecs_inst.bin",
        "gr/fecs_data.bin",
        "gr/gpccs_bl.bin",
        "gr/gpccs_inst.bin",
        "gr/gpccs_data.bin",
        "gr/sw_ctx.bin",
        "gr/sw_nonctx.bin",
        "gr/sw_bundle_init.bin",
        "gr/sw_method_init.bin",
        "nvdec/scrubber.bin",
        "sec2/desc.bin",
        "sec2/image.bin",
        "sec2/sig.bin",
    ];

    firmware_files
        .iter()
        .map(|f| {
            let path = format!("{base}/{f}");
            let exists = std::path::Path::new(&path).exists();
            (path, exists)
        })
        .collect()
}
