// SPDX-License-Identifier: AGPL-3.0-or-later

/// Map SM architecture version to chip codename for firmware lookup.
///
/// Used by nouveau BAR0 init, VFIO compute, and GSP knowledge. Single source
/// of truth for sm → chip mapping (e.g. SM 70 → "gv100", SM 86 → "ga102").
#[must_use]
pub const fn chip_name(sm: u32) -> &'static str {
    match sm {
        35..=37 => "gk210",
        50..=52 => "gm200",
        60..=62 => "gp100",
        70 => "gv100",
        75 => "tu102",
        80 => "ga100",
        86..=87 => "ga102",
        89 => "ad102",
        90 => "gh100",
        100 => "gb100",
        120 => "gb202",
        _ => "gv100",
    }
}

/// Decode the NVIDIA `NV_PMC_BOOT_0` (BAR0 offset 0x0) register into an
/// SM architecture version.
///
/// BOOT0 layout (per envytools / nouveau):
///   - bits\[31:20\] = chipset ID (e.g. 0x140 = GV100, 0x172 = GA102)
///   - bits\[19:16\] = variant
///   - bits\[7:0\]   = revision/stepping
///
/// Returns `None` for unrecognized chipsets. This is the **authoritative**
/// hardware identity — callers must not assume an SM version without
/// consulting BOOT0 first, or they risk applying wrong firmware and
/// corrupting GPU state.
#[must_use]
pub const fn boot0_to_sm(boot0: u32) -> Option<u32> {
    let chipset = (boot0 >> 20) & 0xFFF;
    match chipset {
        0x0F0..=0x0F1 => Some(35),  // Kepler GK110/GK110B (K40/K80 first die)
        0x0F2..=0x0FF => Some(37),  // Kepler GK210 (K80 — exact ID TBD from real silicon)
        0x120..=0x12F => Some(50),  // Maxwell GM200
        0x130..=0x13F => Some(60),  // Pascal GP100/GP102/GP104/GP106/GP107/GP108
        0x140 => Some(70),          // Volta GV100
        0x164..=0x168 => Some(75),  // Turing TU102/TU104/TU106/TU116/TU117
        0x170 => Some(80),          // Ampere GA100
        0x172..=0x177 => Some(86),  // Ampere GA102/GA103/GA104/GA106/GA107
        0x180 => Some(90),          // Hopper GH100 (H100/H200)
        0x192..=0x197 => Some(89),  // Ada Lovelace AD102/AD103/AD104/AD106/AD107
        0x1A0 | 0x1A2 => Some(100), // Blackwell GB100/GB102 (B100/B200 datacenter)
        0x1B2..=0x1B7 => Some(120), // Blackwell GB202/GB203/GB205/GB206/GB207 (RTX 50-series)
        _ => None,
    }
}

/// Decode BOOT0 chipset ID to a specific chip variant name.
///
/// Finer-grained than [`chip_name`] — distinguishes AD102 from AD104, GB202 from GB205, etc.
/// Use this for diagnostics and per-chip identity; use [`chip_name`] for firmware directory lookup.
#[must_use]
pub const fn chipset_variant(boot0: u32) -> &'static str {
    let chipset = (boot0 >> 20) & 0xFFF;
    match chipset {
        0x0F0 => "gk110",
        0x0F1 => "gk110b",
        0x0F2..=0x0FF => "gk210",
        0x120..=0x12F => "gm200",
        0x130..=0x13F => "gp100",
        0x140 => "gv100",
        0x164 => "tu102",
        0x166 => "tu104",
        0x167 => "tu106",
        0x168 => "tu116",
        0x170 => "ga100",
        0x172 => "ga102",
        0x173 => "ga103",
        0x174 => "ga104",
        0x176 => "ga106",
        0x177 => "ga107",
        0x180 => "gh100",
        0x192 => "ad102",
        0x193 => "ad103",
        0x194 => "ad104",
        0x196 => "ad106",
        0x197 => "ad107",
        0x1A0 => "gb100",
        0x1A2 => "gb102",
        0x1B2 => "gb202",
        0x1B3 => "gb203",
        0x1B5 => "gb205",
        0x1B6 => "gb206",
        0x1B7 => "gb207",
        _ => "unknown",
    }
}

/// Map SM version to the NVIDIA compute engine class constant.
///
/// Single source of truth — used by VFIO, DRM, and test harnesses.
/// Returns the DRM/VFIO class ID for the compute engine on this GPU.
#[must_use]
pub const fn sm_to_compute_class(sm: u32) -> u32 {
    match sm {
        35..=37 => 0xA1C0, // KEPLER_COMPUTE_B (GK110/GK210)
        50..=52 => 0xB0C0, // MAXWELL_COMPUTE_B
        60..=62 => 0xC0C0, // PASCAL_COMPUTE_A
        70..=74 => 0xC3C0, // VOLTA_COMPUTE_A
        75..=79 => 0xC5C0, // TURING_COMPUTE_A
        80..=89 => 0xC6C0, // AMPERE_COMPUTE_A (also Ada)
        90..=99 => 0xC7C0, // HOPPER_COMPUTE_A
        _ => 0xC8C0,       // BLACKWELL_COMPUTE_A (SM 100+)
    }
}
