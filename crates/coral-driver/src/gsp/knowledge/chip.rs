// SPDX-License-Identifier: AGPL-3.0-or-later

use super::super::firmware_parser::GrFirmwareBlobs;
use super::super::firmware_source::nvidia_firmware_base;
use super::types::AddressSpace;

/// Detect address space from parsed firmware blobs.
pub(super) fn detect_address_space(blobs: &GrFirmwareBlobs) -> AddressSpace {
    let Some(first) = blobs.bundle_init.first() else {
        return AddressSpace::Unknown;
    };
    if first.addr >= 0x0040_0000 {
        AddressSpace::Bar0Mmio
    } else {
        AddressSpace::MethodOffset
    }
}

/// Check if a chip has GSP or PMU firmware.
pub(super) fn has_gsp_or_pmu(chip: &str) -> bool {
    let base = nvidia_firmware_base().join(chip);
    base.join("gsp").is_dir() || base.join("pmu").is_dir()
}

/// Map chip codename to SM version.
pub(super) fn sm_for_chip(chip: &str) -> Option<u32> {
    match chip {
        "gk20a" => Some(32),
        "gm200" | "gm204" | "gm206" | "gm20b" => Some(52),
        "gp100" | "gp102" | "gp104" | "gp106" | "gp107" | "gp108" | "gp10b" => Some(60),
        "gv100" => Some(70),
        "tu102" | "tu104" | "tu106" | "tu10x" | "tu116" | "tu117" => Some(75),
        "ga100" => Some(80),
        "ga102" | "ga103" | "ga104" | "ga106" | "ga107" => Some(86),
        "ad102" | "ad103" | "ad104" | "ad106" | "ad107" => Some(89),
        _ => None,
    }
}
