// SPDX-License-Identifier: AGPL-3.0-only
//! Sovereign GPU boot — architecture-agnostic entry point.
//!
//! Composes [`DomainMap`] + [`BootSequence`] + [`RecipeStep`] + [`ReplayHooks`]
//! into a single function that can cold-boot any supported GPU architecture.
//! The K80 Kepler and Titan V Volta are the first two configurations; AMD RDNA
//! and Intel Xe are stubs awaiting domain table captures.

use std::path::Path;

use crate::DriverError;
use crate::vfio::device::MappedBar;

use super::boot_follower::{
    BootPhase, BootSequence, DomainMap, DomainRange, KeplerBootSequence, KeplerDomainMap,
    RecipeStep, VoltaBootSequence, VoltaDomainMap, classify_from_table,
};
use super::firmware_probe::{self, FirmwareSnapshot};
use super::k80_cold_boot;
use super::replay::{
    self, KeplerPllHooks, NoHooks, ReplayHooks, ReplayResult, VoltaReplayHooks,
};

// ── GPU era classification ───────────────────────────────────────────────

/// GPU architecture era, determined from PMC_BOOT_0 chip identification.
///
/// This is the driver-internal equivalent of `GpuTarget` from `coral-reef`,
/// detected from live hardware rather than compile-time configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuEra {
    /// Kepler (GK1xx, chip 0x0E0-0x0FF) — K80, K40, K20.
    NvidiaKepler,
    /// Volta (GV1xx, chip 0x140-0x14F) — Titan V, V100.
    NvidiaVolta,
    /// Turing/Ampere/Ada/Hopper/Blackwell — SM75+.
    NvidiaModern,
    /// AMD GPU (detected via PCI vendor ID 0x1002, not BOOT0).
    AmdRdna,
    /// Unknown or undetected.
    Unknown,
}

impl GpuEra {
    /// Detect GPU era from the PMC_BOOT_0 register value.
    pub fn from_boot0(boot0: u32) -> Self {
        let chip = (boot0 >> 20) & 0x1FF;
        match chip {
            0x0E0..=0x0FF => Self::NvidiaKepler,
            0x140..=0x14F => Self::NvidiaVolta,
            0x160..=0x1FF => Self::NvidiaModern,
            _ if chip >= 0x100 && chip < 0x160 => Self::NvidiaModern,
            _ => Self::Unknown,
        }
    }
}

impl std::fmt::Display for GpuEra {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NvidiaKepler => write!(f, "NVIDIA Kepler"),
            Self::NvidiaVolta => write!(f, "NVIDIA Volta"),
            Self::NvidiaModern => write!(f, "NVIDIA Modern"),
            Self::AmdRdna => write!(f, "AMD RDNA"),
            Self::Unknown => write!(f, "Unknown"),
        }
    }
}

// ── AMD RDNA domain map stub ─────────────────────────────────────────────

/// AMD RDNA2+ BAR0 domain layout (stub — needs captures from agentReagents).
#[derive(Debug, Clone, Copy)]
pub struct RdnaDomainMap;

/// Preliminary RDNA2 domain ranges — to be populated from captures.
pub const RDNA2_DOMAINS: &[DomainRange] = &[
    DomainRange { name: "SMN",     start: 0x000000, end: 0x001000, priority: 3 },
    DomainRange { name: "MMHUB",   start: 0x068000, end: 0x06A000, priority: 5 },
    DomainRange { name: "GFX",     start: 0x028000, end: 0x030000, priority: 10 },
    DomainRange { name: "SDMA0",   start: 0x004A00, end: 0x004E00, priority: 20 },
];

impl DomainMap for RdnaDomainMap {
    fn classify(&self, offset: usize, region_hint: &str) -> (&'static str, u32) {
        let (name, prio) = classify_from_table(RDNA2_DOMAINS, offset);
        if name != "UNKNOWN" {
            return (name, prio);
        }
        match region_hint {
            "GFX" => ("GFX", 10),
            "MMHUB" => ("MMHUB", 5),
            "SDMA" | "SDMA0" => ("SDMA0", 20),
            _ => ("UNKNOWN", 99),
        }
    }

    fn domain_table(&self) -> &[DomainRange] {
        RDNA2_DOMAINS
    }
}

/// AMD RDNA boot sequence stub.
#[derive(Debug)]
pub struct RdnaBootSequence;

const RDNA_PHASES: &[BootPhase] = &[
    BootPhase {
        name: "smu_init",
        priority_min: 0,
        priority_max: 5,
        required: true,
    },
    BootPhase {
        name: "mmhub_init",
        priority_min: 5,
        priority_max: 10,
        required: true,
    },
    BootPhase {
        name: "gfx_init",
        priority_min: 10,
        priority_max: 30,
        required: true,
    },
];

impl BootSequence for RdnaBootSequence {
    fn phases(&self) -> &[BootPhase] {
        RDNA_PHASES
    }

    fn domain_map(&self) -> &dyn DomainMap {
        &RdnaDomainMap
    }

    fn description(&self) -> &str {
        "AMD RDNA2+ (stub): SMU init -> MMHUB -> GFX engine"
    }
}

// ── Architecture dispatch ────────────────────────────────────────────────

/// Select the appropriate domain map for a GPU era.
pub fn domain_map_for_era(era: GpuEra) -> &'static dyn DomainMap {
    match era {
        GpuEra::NvidiaKepler => &KeplerDomainMap,
        GpuEra::NvidiaVolta | GpuEra::NvidiaModern => &VoltaDomainMap,
        GpuEra::AmdRdna => &RdnaDomainMap,
        GpuEra::Unknown => &KeplerDomainMap, // safe default
    }
}

/// Select the appropriate boot sequence for a GPU era.
pub fn boot_sequence_for_era(era: GpuEra) -> &'static dyn BootSequence {
    match era {
        GpuEra::NvidiaKepler => &KeplerBootSequence,
        GpuEra::NvidiaVolta | GpuEra::NvidiaModern => &VoltaBootSequence,
        GpuEra::AmdRdna => &RdnaBootSequence,
        GpuEra::Unknown => &KeplerBootSequence,
    }
}

/// Select replay hooks matching the GPU era.
pub fn replay_hooks_for_era(era: GpuEra) -> Box<dyn ReplayHooks> {
    match era {
        GpuEra::NvidiaKepler => Box::new(KeplerPllHooks::default()),
        GpuEra::NvidiaVolta | GpuEra::NvidiaModern => Box::new(VoltaReplayHooks::default()),
        GpuEra::AmdRdna | GpuEra::Unknown => Box::new(NoHooks),
    }
}

// ── Sovereign boot result ────────────────────────────────────────────────

/// Result of a sovereign boot attempt across any architecture.
#[derive(Debug)]
pub struct SovereignBootResult {
    /// GPU era that was booted.
    pub era: GpuEra,
    /// PMC_BOOT_0 chip identification.
    pub boot0: u32,
    /// Per-phase replay results, keyed by phase name.
    pub phase_results: Vec<(String, ReplayResult)>,
    /// Firmware snapshot after boot (NVIDIA only for now).
    pub firmware_snapshot: Option<FirmwareSnapshot>,
    /// Whether the GPU appears alive after boot.
    pub alive: bool,
    /// Step-by-step log.
    pub log: Vec<String>,
}

// ── Sovereign boot entry point ───────────────────────────────────────────

/// Boot a GPU from cold/warm state using the architecture-appropriate
/// domain map, boot sequence, and replay hooks.
///
/// Probes the GPU via PMC_BOOT_0 to auto-detect the architecture era,
/// then selects the right domain map, boot sequence, and replay hooks.
/// This is the universal entry point that replaces architecture-specific
/// boot functions.
pub fn sovereign_boot(
    bar0: &MappedBar,
    recipe_path: &Path,
    era_override: Option<GpuEra>,
) -> Result<SovereignBootResult, DriverError> {
    let boot0 = bar0.read_u32(replay::PMC_BOOT_0).unwrap_or(0xFFFF_FFFF);
    let era = era_override.unwrap_or_else(|| GpuEra::from_boot0(boot0));
    let seq = boot_sequence_for_era(era);
    let hooks = replay_hooks_for_era(era);
    let mut log = Vec::new();

    log.push(format!(
        "sovereign boot: {} (BOOT0={boot0:#010x}) — {}",
        era,
        seq.description()
    ));

    let full_recipe = k80_cold_boot::load_recipe_auto(recipe_path)?;
    log.push(format!(
        "recipe: {} steps loaded from {}",
        full_recipe.len(),
        recipe_path.display()
    ));

    // Pre-boot probe (NVIDIA).
    let pre_snap = if !matches!(era, GpuEra::AmdRdna) {
        let snap = firmware_probe::capture_firmware_snapshot(bar0, "sovereign-pre-boot");
        firmware_probe::log_firmware_summary(&snap);
        log.push(format!(
            "pre-boot: BOOT0={:#010x} arch={}",
            snap.boot0, snap.architecture
        ));
        Some(snap)
    } else {
        None
    };

    let ptimer_pre = replay::poll_ptimer_ticking(bar0, 100);
    log.push(format!("pre-boot PTIMER: ticking={ptimer_pre}"));

    let mut phase_results = Vec::new();
    let mut alive = false;

    for phase in seq.phases() {
        if phase.name == "clock" && !phase.required && ptimer_pre {
            log.push(format!(
                "phase '{}': skipped (PTIMER already ticking)",
                phase.name
            ));
            continue;
        }

        let steps: Vec<RecipeStep> = full_recipe
            .iter()
            .filter(|s| s.priority >= phase.priority_min && s.priority < phase.priority_max)
            .cloned()
            .collect();

        if steps.is_empty() {
            log.push(format!(
                "phase '{}': 0 steps (priority {}..{})",
                phase.name, phase.priority_min, phase.priority_max
            ));
            continue;
        }

        log.push(format!(
            "phase '{}': {} steps (priority {}..{})",
            phase.name,
            steps.len(),
            phase.priority_min,
            phase.priority_max
        ));

        let result = replay::apply_recipe_phased(bar0, &steps, hooks.as_ref())?;
        log.push(format!(
            "phase '{}': applied={} failed={} ptimer={}",
            phase.name, result.applied, result.failed, result.ptimer_ticking
        ));

        alive = result.is_alive();
        phase_results.push((phase.name.to_string(), result));
    }

    let post_snap = if !matches!(era, GpuEra::AmdRdna) {
        let snap = firmware_probe::capture_firmware_snapshot(bar0, "sovereign-post-boot");
        firmware_probe::log_firmware_summary(&snap);

        if let Some(pre) = &pre_snap {
            let diffs = firmware_probe::diff_snapshots(pre, &snap);
            log.push(format!("firmware diff: {} registers changed", diffs.len()));
        }
        Some(snap)
    } else {
        None
    };

    Ok(SovereignBootResult {
        era,
        boot0,
        phase_results,
        firmware_snapshot: post_snap,
        alive,
        log,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gpu_era_from_boot0_kepler() {
        assert_eq!(GpuEra::from_boot0(0x0F22_D0A1), GpuEra::NvidiaKepler);
        assert_eq!(GpuEra::from_boot0(0x0E40_0000), GpuEra::NvidiaKepler);
    }

    #[test]
    fn gpu_era_from_boot0_volta() {
        assert_eq!(GpuEra::from_boot0(0x1400_00A1), GpuEra::NvidiaVolta);
    }

    #[test]
    fn gpu_era_from_boot0_modern() {
        // Turing TU102 chip=0x162
        assert_eq!(GpuEra::from_boot0(0x1620_00A1), GpuEra::NvidiaModern);
    }

    #[test]
    fn gpu_era_display() {
        assert_eq!(GpuEra::NvidiaKepler.to_string(), "NVIDIA Kepler");
        assert_eq!(GpuEra::NvidiaVolta.to_string(), "NVIDIA Volta");
        assert_eq!(GpuEra::AmdRdna.to_string(), "AMD RDNA");
    }

    #[test]
    fn domain_map_dispatch_kepler() {
        let map = domain_map_for_era(GpuEra::NvidiaKepler);
        assert_eq!(map.classify(0x136400, "").0, "ROOT_PLL");
        assert_eq!(map.classify(0x009400, "").0, "PTIMER");
    }

    #[test]
    fn domain_map_dispatch_volta() {
        let map = domain_map_for_era(GpuEra::NvidiaVolta);
        assert_eq!(map.classify(0x087500, "").0, "SEC2");
        assert_eq!(map.classify(0x409100, "").0, "FECS");
    }

    #[test]
    fn domain_map_dispatch_amd() {
        let map = domain_map_for_era(GpuEra::AmdRdna);
        assert_eq!(map.classify(0x068100, "").0, "MMHUB");
    }

    #[test]
    fn boot_sequence_dispatch_kepler() {
        let seq = boot_sequence_for_era(GpuEra::NvidiaKepler);
        assert!(seq.description().contains("Kepler"));
        assert_eq!(seq.phases()[0].name, "clock");
        assert!(seq.phases()[0].required);
    }

    #[test]
    fn boot_sequence_dispatch_volta() {
        let seq = boot_sequence_for_era(GpuEra::NvidiaVolta);
        assert!(seq.description().contains("Volta"));
        assert!(!seq.phases()[0].required);
    }

    #[test]
    fn boot_sequence_dispatch_amd() {
        let seq = boot_sequence_for_era(GpuEra::AmdRdna);
        assert!(seq.description().contains("RDNA"));
    }

    #[test]
    fn replay_hooks_kepler() {
        let hooks = replay_hooks_for_era(GpuEra::NvidiaKepler);
        let debug = format!("{hooks:?}");
        assert!(debug.contains("Kepler"));
    }

    #[test]
    fn replay_hooks_volta() {
        let hooks = replay_hooks_for_era(GpuEra::NvidiaVolta);
        let debug = format!("{hooks:?}");
        assert!(debug.contains("Volta"));
    }

    #[test]
    fn replay_hooks_amd() {
        let hooks = replay_hooks_for_era(GpuEra::AmdRdna);
        let debug = format!("{hooks:?}");
        assert!(debug.contains("NoHooks"));
    }

    #[test]
    fn rdna_domain_map_classifies_known_ranges() {
        let map = RdnaDomainMap;
        assert_eq!(map.classify(0x068100, "").0, "MMHUB");
        assert_eq!(map.classify(0x029000, "").0, "GFX");
        assert_eq!(map.classify(0x004B00, "").0, "SDMA0");
        assert_eq!(map.classify(0x000100, "").0, "SMN");
    }

    #[test]
    fn rdna_domain_map_region_hint_fallback() {
        let map = RdnaDomainMap;
        assert_eq!(map.classify(0xFFF000, "GFX").0, "GFX");
        assert_eq!(map.classify(0xFFF000, "MMHUB").0, "MMHUB");
        assert_eq!(map.classify(0xFFF000, "UNKNOWN").0, "UNKNOWN");
    }

    #[test]
    fn rdna_boot_sequence_has_three_phases() {
        let seq = RdnaBootSequence;
        assert_eq!(seq.phases().len(), 3);
        assert_eq!(seq.phases()[0].name, "smu_init");
        assert_eq!(seq.phases()[1].name, "mmhub_init");
        assert_eq!(seq.phases()[2].name, "gfx_init");
    }

    #[test]
    fn sovereign_boot_result_carries_era() {
        let result = SovereignBootResult {
            era: GpuEra::NvidiaKepler,
            boot0: 0x0F22_D0A1,
            phase_results: vec![],
            firmware_snapshot: None,
            alive: false,
            log: vec!["test".into()],
        };
        assert_eq!(result.era, GpuEra::NvidiaKepler);
        assert!(!result.alive);
    }

    #[test]
    fn all_eras_get_valid_dispatch() {
        let eras = [
            GpuEra::NvidiaKepler,
            GpuEra::NvidiaVolta,
            GpuEra::NvidiaModern,
            GpuEra::AmdRdna,
            GpuEra::Unknown,
        ];
        for era in &eras {
            let map = domain_map_for_era(*era);
            let seq = boot_sequence_for_era(*era);
            assert!(
                !map.domain_table().is_empty(),
                "domain table empty for {era}"
            );
            assert!(
                !seq.phases().is_empty(),
                "boot phases empty for {era}"
            );
        }
    }
}
