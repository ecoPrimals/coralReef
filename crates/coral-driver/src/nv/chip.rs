// SPDX-License-Identifier: AGPL-3.0-only
//! Capability-based chip abstraction.
//!
//! Replaces hardcoded match arms across `identity.rs`, `types.rs`, `pci_ids.rs`,
//! and `firmware.rs` with runtime-discovered, per-family capability objects.
//! New chip families are added by implementing [`ChipCapability`] — callers
//! never need to know the concrete type.
//!
//! # Discovery
//!
//! ```
//! use coral_driver::nv::chip::{detect_from_boot0, detect_from_sm};
//!
//! // From BOOT0 register (hardware probe):
//! let chip = detect_from_boot0(0x140_0_0_a1); // GV100 Titan V
//! assert_eq!(chip.chip_name(), "gv100");
//!
//! // From known SM version:
//! let chip = detect_from_sm(37);
//! assert_eq!(chip.family(), ChipFamily::Kepler);
//! ```

use super::identity;

/// GPU architecture family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChipFamily {
    /// Kepler (GK110, GK210) — SM 3.5 / 3.7.
    Kepler,
    /// Maxwell (GM200) — SM 5.x.
    Maxwell,
    /// Pascal (GP100, GP10x) — SM 6.x.
    Pascal,
    /// Volta (GV100) — SM 7.0.
    Volta,
    /// Turing (TU10x) — SM 7.5.
    Turing,
    /// Ampere (GA100, GA10x) — SM 8.x.
    Ampere,
    /// Ada Lovelace (AD10x) — SM 8.9.
    Ada,
    /// Hopper (GH100) — SM 9.0.
    Hopper,
    /// Blackwell (GB10x, GB20x) — SM 10.0+.
    Blackwell,
    /// Unrecognized architecture.
    Unknown,
}

impl ChipFamily {
    /// Human-readable family name for diagnostics.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Kepler => "kepler",
            Self::Maxwell => "maxwell",
            Self::Pascal => "pascal",
            Self::Volta => "volta",
            Self::Turing => "turing",
            Self::Ampere => "ampere",
            Self::Ada => "ada",
            Self::Hopper => "hopper",
            Self::Blackwell => "blackwell",
            Self::Unknown => "unknown",
        }
    }

    /// Whether this family has FLR (Function Level Reset) support.
    #[must_use]
    pub const fn has_flr(self) -> bool {
        !matches!(self, Self::Kepler | Self::Volta | Self::Unknown)
    }
}

impl std::fmt::Display for ChipFamily {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Runtime-discovered chip capabilities.
///
/// Every hardcoded chip-specific constant (register offsets, firmware paths,
/// compute classes, settle times) becomes a method on this trait. Callers
/// use `&dyn ChipCapability` or the concrete structs directly.
pub trait ChipCapability: Send + Sync + std::fmt::Debug {
    /// Architecture family (Kepler, Volta, etc.).
    fn family(&self) -> ChipFamily;
    /// Firmware directory name under `/lib/firmware/nvidia/` (e.g. `"gv100"`).
    fn chip_name(&self) -> &'static str;
    /// Specific chip variant (e.g. `"gk210"`, `"gv100"`, `"tu104"`).
    fn variant_name(&self) -> &'static str;
    /// SM architecture version (e.g. 70 for Volta, 37 for Kepler K80).
    fn sm_version(&self) -> u32;
    /// DRM/VFIO compute engine class ID.
    fn compute_class(&self) -> u32;
    /// BAR0 register offsets safe to probe for health monitoring.
    fn register_dump_offsets(&self) -> &'static [usize];
    /// Whether PCIe Function Level Reset is supported.
    fn has_flr(&self) -> bool;
    /// Whether the chip has SEC2 (Security Engine v2).
    fn has_sec2(&self) -> bool;
    /// Whether PMU firmware is available for this chip.
    fn has_pmu_firmware(&self) -> bool;
    /// Whether GSP firmware is used (Ampere+).
    fn has_gsp(&self) -> bool;
    /// Firmware file base path.
    fn firmware_base(&self) -> String {
        format!("/lib/firmware/nvidia/{}", self.chip_name())
    }
    /// ACR firmware files needed for secure boot (relative to firmware_base).
    fn acr_firmware_files(&self) -> &'static [&'static str];
    /// Recommended driver-swap settle time in seconds.
    fn settle_secs(&self, target_driver: &str) -> u64;
}

// ---------------------------------------------------------------------------
// Kepler (GK110 / GK210 — K40, K80)
// ---------------------------------------------------------------------------

/// Kepler chip capability (SM 3.5 / 3.7).
#[derive(Debug)]
pub struct KeplerChip {
    sm: u32,
    variant: &'static str,
}

impl KeplerChip {
    /// SM 37 = GK210 (K80), SM 35 = GK110 (K40).
    #[must_use]
    pub const fn new(sm: u32, variant: &'static str) -> Self {
        Self { sm, variant }
    }
}

/// Conservative Kepler register offsets — only engines known to exist.
/// Omits Volta-specific domains (FBHUB, SEC2, GPCCS, PCLOCK) and uses
/// Kepler-era PMU/GR/FECS base addresses.
pub static KEPLER_REGISTER_DUMP_OFFSETS: &[usize] = &[
    // PMC
    0x00_0000, 0x00_0004, 0x00_0200, 0x00_0204,
    // PBUS
    0x00_1C00, 0x00_1C04,
    // PFIFO
    0x00_2004, 0x00_2100, 0x00_2140, 0x00_2200, 0x00_2254, 0x00_2504, 0x00_2508,
    // PBDMA0 (Kepler has fewer PBDMA channels)
    0x04_0040, 0x04_0044, 0x04_0048, 0x04_004C, 0x04_0060,
    // PFB
    0x10_0000, 0x10_0200, 0x10_0204, 0x10_0800, 0x10_0804,
    // BAR1 / BAR2 PRAMIN
    0x10_1000, 0x10_1004, 0x10_1008,
    // PMU Falcon (Kepler addresses)
    0x10_A000, 0x10_A040, 0x10_A044, 0x10_A04C, 0x10_A100, 0x10_A104,
    // GR
    0x40_0100, 0x40_0108, 0x40_0110,
    // FECS Falcon
    0x40_9028, 0x40_9030, 0x40_9034, 0x40_9038, 0x40_9040, 0x40_9044,
    0x40_9080, 0x40_9084, 0x40_9100, 0x40_9104, 0x40_9108,
    // THERM
    0x02_0400, 0x02_0460,
    // NV_PRAMIN window
    0x70_0000, 0x70_0004,
    // PROM
    0x30_0000, 0x30_0004,
];

impl ChipCapability for KeplerChip {
    fn family(&self) -> ChipFamily { ChipFamily::Kepler }
    fn chip_name(&self) -> &'static str { "gk210" }
    fn variant_name(&self) -> &'static str { self.variant }
    fn sm_version(&self) -> u32 { self.sm }
    fn compute_class(&self) -> u32 { 0xA1C0 } // KEPLER_COMPUTE_B
    fn register_dump_offsets(&self) -> &'static [usize] { KEPLER_REGISTER_DUMP_OFFSETS }
    fn has_flr(&self) -> bool { false }
    fn has_sec2(&self) -> bool { false }
    fn has_pmu_firmware(&self) -> bool { true }
    fn has_gsp(&self) -> bool { false }
    fn acr_firmware_files(&self) -> &'static [&'static str] { &[] }
    fn settle_secs(&self, target_driver: &str) -> u64 {
        if target_driver == "nouveau" { 20 } else { 5 }
    }
}

// ---------------------------------------------------------------------------
// Volta (GV100 — Titan V, V100)
// ---------------------------------------------------------------------------

/// Volta chip capability (SM 7.0).
#[derive(Debug)]
pub struct VoltaChip {
    variant: &'static str,
}

impl VoltaChip {
    #[must_use]
    pub const fn new(variant: &'static str) -> Self {
        Self { variant }
    }
}

/// Full GV100 register dump offsets — the canonical set.
pub static VOLTA_REGISTER_DUMP_OFFSETS: &[usize] = &[
    // PMC
    0x00_0000, 0x00_0004, 0x00_0200, 0x00_0204,
    // PBUS
    0x00_1C00, 0x00_1C04,
    // PFIFO
    0x00_2004, 0x00_2100, 0x00_2140, 0x00_2200, 0x00_2254, 0x00_2270, 0x00_2274, 0x00_2280,
    0x00_2284, 0x00_228C, 0x00_2390, 0x00_2394, 0x00_2398, 0x00_239C, 0x00_2504, 0x00_2508,
    0x00_252C, 0x00_2630, 0x00_2634, 0x00_2638, 0x00_2640, 0x00_2A00, 0x00_2A04,
    // PBDMA idle + PBDMA0
    0x00_3080, 0x00_3084, 0x00_3088, 0x00_308C, 0x04_0040, 0x04_0044, 0x04_0048, 0x04_004C,
    0x04_0054, 0x04_0060, 0x04_0068, 0x04_0080, 0x04_0084, 0x04_00A4, 0x04_0100, 0x04_0104,
    0x04_0108, 0x04_010C, 0x04_0110, 0x04_0114, 0x04_0118,
    // PFB / FBHUB
    0x10_0000, 0x10_0200, 0x10_0204, 0x10_0C80, 0x10_0C84, 0x10_0800, 0x10_0804, 0x10_0808,
    0x10_080C, 0x10_0810,
    // BAR1 / BAR2 PRAMIN
    0x10_1000, 0x10_1004, 0x10_1008, 0x10_1714,
    // PMU Falcon
    0x10_A000, 0x10_A040, 0x10_A044, 0x10_A04C, 0x10_A100, 0x10_A104, 0x10_A108, 0x10_A110,
    0x10_A114, 0x10_A118,
    // PCLOCK
    0x13_7000, 0x13_7050, 0x13_7100,
    // GR
    0x40_0100, 0x40_0108, 0x40_0110,
    // FECS Falcon
    0x40_9028, 0x40_9030, 0x40_9034, 0x40_9038, 0x40_9040, 0x40_9044, 0x40_904C, 0x40_9080,
    0x40_9084, 0x40_9100, 0x40_9104, 0x40_9108, 0x40_9110, 0x40_9210, 0x40_9380,
    // GPCCS Falcon
    0x41_A028, 0x41_A030, 0x41_A034, 0x41_A038, 0x41_A040, 0x41_A044, 0x41_A04C, 0x41_A080,
    0x41_A084, 0x41_A100, 0x41_A108,
    // MMU Fault buffer
    0x10_0E24, 0x10_0E28, 0x10_0E2C, 0x10_0E30,
    // LTC (L2 cache)
    0x17_E200, 0x17_E204, 0x17_E210,
    // FBPA0
    0x9A_0000, 0x9A_0004, 0x9A_0200,
    // THERM
    0x02_0400, 0x02_0460,
    // NV_PRAMIN window
    0x70_0000, 0x70_0004,
    // PROM
    0x30_0000, 0x30_0004,
];

static VOLTA_ACR_FILES: &[&str] = &[
    "acr/bl.bin",
    "acr/ucode_load.bin",
    "sec2/desc.bin",
    "sec2/image.bin",
    "sec2/sig.bin",
    "gr/fecs_bl.bin",
    "gr/fecs_inst.bin",
    "gr/fecs_data.bin",
    "gr/fecs_sig.bin",
    "gr/gpccs_bl.bin",
    "gr/gpccs_inst.bin",
    "gr/gpccs_data.bin",
    "gr/gpccs_sig.bin",
];

impl ChipCapability for VoltaChip {
    fn family(&self) -> ChipFamily { ChipFamily::Volta }
    fn chip_name(&self) -> &'static str { "gv100" }
    fn variant_name(&self) -> &'static str { self.variant }
    fn sm_version(&self) -> u32 { 70 }
    fn compute_class(&self) -> u32 { 0xC3C0 } // VOLTA_COMPUTE_A
    fn register_dump_offsets(&self) -> &'static [usize] { VOLTA_REGISTER_DUMP_OFFSETS }
    fn has_flr(&self) -> bool { false }
    fn has_sec2(&self) -> bool { true }
    fn has_pmu_firmware(&self) -> bool { false }
    fn has_gsp(&self) -> bool { false }
    fn acr_firmware_files(&self) -> &'static [&'static str] { VOLTA_ACR_FILES }
    fn settle_secs(&self, target_driver: &str) -> u64 {
        if target_driver == "nouveau" { 10 } else { 5 }
    }
}

// ---------------------------------------------------------------------------
// Turing (TU10x — RTX 2000 series)
// ---------------------------------------------------------------------------

/// Turing chip capability (SM 7.5).
#[derive(Debug)]
pub struct TuringChip {
    variant: &'static str,
}

impl TuringChip {
    /// Create a Turing capability with the given variant name.
    #[must_use]
    pub const fn new(variant: &'static str) -> Self {
        Self { variant }
    }
}

impl ChipCapability for TuringChip {
    fn family(&self) -> ChipFamily { ChipFamily::Turing }
    fn chip_name(&self) -> &'static str { "tu102" }
    fn variant_name(&self) -> &'static str { self.variant }
    fn sm_version(&self) -> u32 { 75 }
    fn compute_class(&self) -> u32 { 0xC5C0 } // TURING_COMPUTE_A
    fn register_dump_offsets(&self) -> &'static [usize] { VOLTA_REGISTER_DUMP_OFFSETS }
    fn has_flr(&self) -> bool { true }
    fn has_sec2(&self) -> bool { true }
    fn has_pmu_firmware(&self) -> bool { false }
    fn has_gsp(&self) -> bool { false }
    fn acr_firmware_files(&self) -> &'static [&'static str] { VOLTA_ACR_FILES }
    fn settle_secs(&self, target_driver: &str) -> u64 {
        if target_driver == "nouveau" { 10 } else { 5 }
    }
}

// ---------------------------------------------------------------------------
// Ampere (GA100, GA10x — RTX 3000 / A100)
// ---------------------------------------------------------------------------

/// Ampere chip capability (SM 8.x).
#[derive(Debug)]
pub struct AmpereChip {
    sm: u32,
    variant: &'static str,
}

impl AmpereChip {
    /// Create an Ampere capability with the given SM and variant name.
    #[must_use]
    pub const fn new(sm: u32, variant: &'static str) -> Self {
        Self { sm, variant }
    }
}

impl ChipCapability for AmpereChip {
    fn family(&self) -> ChipFamily { ChipFamily::Ampere }
    fn chip_name(&self) -> &'static str {
        if self.sm == 80 { "ga100" } else { "ga102" }
    }
    fn variant_name(&self) -> &'static str { self.variant }
    fn sm_version(&self) -> u32 { self.sm }
    fn compute_class(&self) -> u32 { 0xC6C0 } // AMPERE_COMPUTE_A
    fn register_dump_offsets(&self) -> &'static [usize] { VOLTA_REGISTER_DUMP_OFFSETS }
    fn has_flr(&self) -> bool { true }
    fn has_sec2(&self) -> bool { true }
    fn has_pmu_firmware(&self) -> bool { false }
    fn has_gsp(&self) -> bool { true }
    fn acr_firmware_files(&self) -> &'static [&'static str] { VOLTA_ACR_FILES }
    fn settle_secs(&self, target_driver: &str) -> u64 {
        if target_driver == "nouveau" { 10 } else { 8 }
    }
}

// ---------------------------------------------------------------------------
// Generic / Unknown fallback
// ---------------------------------------------------------------------------

/// Fallback for unrecognized chips — uses conservative Volta-era defaults.
#[derive(Debug)]
pub struct GenericChip {
    sm: u32,
    variant: &'static str,
}

impl GenericChip {
    /// Create a generic fallback capability with the given SM and variant name.
    #[must_use]
    pub const fn new(sm: u32, variant: &'static str) -> Self {
        Self { sm, variant }
    }
}

impl ChipCapability for GenericChip {
    fn family(&self) -> ChipFamily { ChipFamily::Unknown }
    fn chip_name(&self) -> &'static str { identity::chip_name(self.sm) }
    fn variant_name(&self) -> &'static str { self.variant }
    fn sm_version(&self) -> u32 { self.sm }
    fn compute_class(&self) -> u32 { identity::sm_to_compute_class(self.sm) }
    fn register_dump_offsets(&self) -> &'static [usize] { VOLTA_REGISTER_DUMP_OFFSETS }
    fn has_flr(&self) -> bool { true }
    fn has_sec2(&self) -> bool { true }
    fn has_pmu_firmware(&self) -> bool { false }
    fn has_gsp(&self) -> bool { self.sm >= 80 }
    fn acr_firmware_files(&self) -> &'static [&'static str] { VOLTA_ACR_FILES }
    fn settle_secs(&self, target_driver: &str) -> u64 {
        if target_driver == "nouveau" { 10 } else { 5 }
    }
}

// ---------------------------------------------------------------------------
// Detection / Factory
// ---------------------------------------------------------------------------

/// Detect chip capability from a BOOT0 register value.
///
/// Reads the chipset ID from BOOT0, maps it to an SM version, then
/// builds the appropriate capability struct. Returns a `GenericChip`
/// for unrecognized chipsets.
#[must_use]
pub fn detect_from_boot0(boot0: u32) -> Box<dyn ChipCapability> {
    let variant = identity::chipset_variant(boot0);
    match identity::boot0_to_sm(boot0) {
        Some(sm) => detect_from_sm_and_variant(sm, variant),
        None => Box::new(GenericChip::new(0, variant)),
    }
}

/// Detect chip capability from a known SM version.
///
/// Uses the default chip name from `identity::chip_name`.
#[must_use]
pub fn detect_from_sm(sm: u32) -> Box<dyn ChipCapability> {
    detect_from_sm_and_variant(sm, identity::chip_name(sm))
}

fn detect_from_sm_and_variant(sm: u32, variant: &'static str) -> Box<dyn ChipCapability> {
    match sm {
        35..=37 => Box::new(KeplerChip::new(sm, variant)),
        50..=52 => Box::new(GenericChip::new(sm, variant)), // Maxwell — future impl
        60..=62 => Box::new(GenericChip::new(sm, variant)), // Pascal — future impl
        70 => Box::new(VoltaChip::new(variant)),
        75 => Box::new(TuringChip::new(variant)),
        80..=89 => Box::new(AmpereChip::new(sm, variant)),
        _ => Box::new(GenericChip::new(sm, variant)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_gv100_from_boot0() {
        let boot0: u32 = 0x140_0_00_a1; // GV100 chipset 0x140
        let chip = detect_from_boot0(boot0);
        assert_eq!(chip.family(), ChipFamily::Volta);
        assert_eq!(chip.chip_name(), "gv100");
        assert_eq!(chip.sm_version(), 70);
        assert!(!chip.has_flr());
        assert!(chip.has_sec2());
        assert!(!chip.has_pmu_firmware());
        assert!(!chip.acr_firmware_files().is_empty());
    }

    #[test]
    fn detect_gk210_from_boot0() {
        let boot0: u32 = 0x0F2_0_00_a1; // GK210 chipset 0x0F2
        let chip = detect_from_boot0(boot0);
        assert_eq!(chip.family(), ChipFamily::Kepler);
        assert_eq!(chip.chip_name(), "gk210");
        assert_eq!(chip.sm_version(), 37);
        assert!(!chip.has_flr());
        assert!(!chip.has_sec2());
        assert!(chip.has_pmu_firmware());
        assert!(chip.acr_firmware_files().is_empty());
    }

    #[test]
    fn detect_from_sm_volta() {
        let chip = detect_from_sm(70);
        assert_eq!(chip.family(), ChipFamily::Volta);
        assert_eq!(chip.compute_class(), 0xC3C0);
    }

    #[test]
    fn detect_from_sm_kepler() {
        let chip = detect_from_sm(37);
        assert_eq!(chip.family(), ChipFamily::Kepler);
        assert_eq!(chip.compute_class(), 0xA1C0);
    }

    #[test]
    fn detect_from_sm_turing() {
        let chip = detect_from_sm(75);
        assert_eq!(chip.family(), ChipFamily::Turing);
        assert!(chip.has_flr());
    }

    #[test]
    fn detect_from_sm_ampere() {
        let chip = detect_from_sm(86);
        assert_eq!(chip.family(), ChipFamily::Ampere);
        assert!(chip.has_gsp());
        assert!(chip.has_flr());
    }

    #[test]
    fn unknown_boot0_gives_generic() {
        let chip = detect_from_boot0(0xBAD0_0000);
        assert_eq!(chip.family(), ChipFamily::Unknown);
    }

    #[test]
    fn kepler_register_offsets_are_subset_of_volta() {
        let kepler = KEPLER_REGISTER_DUMP_OFFSETS;
        let volta = VOLTA_REGISTER_DUMP_OFFSETS;
        for &off in kepler {
            assert!(
                volta.contains(&off),
                "Kepler offset {off:#08x} not in Volta set",
            );
        }
    }

    #[test]
    fn chip_family_display() {
        assert_eq!(ChipFamily::Volta.to_string(), "volta");
        assert_eq!(ChipFamily::Kepler.as_str(), "kepler");
    }

    #[test]
    fn settle_secs_kepler_nouveau_is_longest() {
        let chip = detect_from_sm(37);
        assert_eq!(chip.settle_secs("nouveau"), 20);
        assert_eq!(chip.settle_secs("vfio-pci"), 5);
    }

    #[test]
    fn volta_firmware_base() {
        let chip = detect_from_sm(70);
        assert_eq!(chip.firmware_base(), "/lib/firmware/nvidia/gv100");
    }
}
