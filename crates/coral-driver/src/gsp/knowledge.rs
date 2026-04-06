// SPDX-License-Identifier: AGPL-3.0-or-later
//! Sovereign GSP knowledge base — learned from all available hardware.
//!
//! Aggregates initialization knowledge from multiple GPU architectures
//! and vendors to build a cross-architecture understanding of GPU
//! compute initialization. This knowledge drives both:
//!
//! - **Init on old hardware**: Apply learned init sequences to GPUs
//!   without firmware (Volta, older Turing)
//! - **Optimization on modern hardware**: Dispatch hints, workgroup
//!   sizing, memory placement based on observed hardware behavior

use super::firmware_parser::{FirmwareFormat, GrFirmwareBlobs};
use super::firmware_source::nvidia_firmware_base;
use super::firmware_source::{FilesystemFirmwareSource, NvidiaFirmwareSource};
use super::gr_init::GrInitSequence;
use std::collections::{BTreeMap, BTreeSet};

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

/// Cross-architecture GPU knowledge base.
#[derive(Debug, Default)]
pub struct GpuKnowledge {
    architectures: BTreeMap<String, ArchKnowledge>,
}

impl GpuKnowledge {
    /// Create a new empty knowledge base.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Learn from all available NVIDIA firmware on this system.
    ///
    /// Uses [`FilesystemFirmwareSource`](crate::gsp::FilesystemFirmwareSource) (default
    /// `/lib/firmware/nvidia/`, override `CORALREEF_NVIDIA_FIRMWARE_PATH`). For tests or custom
    /// layouts, call [`Self::learn_nvidia_firmware_with_source`].
    pub fn learn_nvidia_firmware(&mut self) {
        let source = FilesystemFirmwareSource::new();
        self.learn_nvidia_firmware_with_source(&source);
    }

    /// Learn from NVIDIA firmware supplied by `source` (filesystem, in-memory mock, etc.).
    ///
    /// If `list_chips` fails (for example unreadable directory), behaves like an empty firmware
    /// tree: nothing is learned. Chips whose firmware fails to load are skipped.
    pub fn learn_nvidia_firmware_with_source(&mut self, source: &dyn NvidiaFirmwareSource) {
        let nvidia_chips = source.list_chips().unwrap_or_default();
        for chip in nvidia_chips {
            if let Ok(blobs) = source.load_gr_firmware(&chip) {
                let gr_init = GrInitSequence::from_blobs(&blobs);
                let register_count = blobs.unique_bundle_addrs().len();
                let has_firmware = has_gsp_or_pmu(&chip);
                let sm = sm_for_chip(&chip);

                let address_space = detect_address_space(&blobs);
                self.architectures.insert(
                    chip.clone(),
                    ArchKnowledge {
                        chip,
                        sm,
                        vendor: GpuVendor::Nvidia,
                        has_firmware,
                        format: Some(blobs.format),
                        address_space,
                        gr_blobs: Some(blobs),
                        gr_init: Some(gr_init),
                        register_count,
                    },
                );
            }
        }
    }

    /// Get knowledge for a specific chip.
    #[must_use]
    pub fn get(&self, chip: &str) -> Option<&ArchKnowledge> {
        self.architectures.get(chip)
    }

    /// List all known architectures.
    #[must_use]
    pub fn known_chips(&self) -> Vec<&str> {
        self.architectures.keys().map(String::as_str).collect()
    }

    /// List chips that need sovereign GSP (no firmware).
    #[must_use]
    pub fn needs_sovereign_gsp(&self) -> Vec<&str> {
        self.architectures
            .values()
            .filter(|a| !a.has_firmware)
            .map(|a| a.chip.as_str())
            .collect()
    }

    /// List chips that can teach us (have firmware + init sequences).
    #[must_use]
    pub fn can_teach(&self) -> Vec<&str> {
        self.architectures
            .values()
            .filter(|a| a.has_firmware && a.gr_init.is_some())
            .map(|a| a.chip.as_str())
            .collect()
    }

    /// Compare init sequences between two architectures.
    ///
    /// Returns the number of common register addresses (by value,
    /// not by position) between two chips' bundle init sequences.
    #[must_use]
    pub fn common_registers(&self, chip_a: &str, chip_b: &str) -> usize {
        let a = self
            .get(chip_a)
            .and_then(|k| k.gr_blobs.as_ref())
            .map(GrFirmwareBlobs::unique_bundle_addrs);
        let b = self
            .get(chip_b)
            .and_then(|k| k.gr_blobs.as_ref())
            .map(GrFirmwareBlobs::unique_bundle_addrs);

        match (a, b) {
            (Some(a_addrs), Some(b_addrs)) => {
                a_addrs.iter().filter(|addr| b_addrs.contains(addr)).count()
            }
            _ => 0,
        }
    }

    /// Build a register transfer map from a teacher chip to a target chip.
    ///
    /// Identifies which registers from the teacher's init sequence are
    /// relevant to the target, and which target-specific registers the
    /// teacher cannot provide (must come from the target's own firmware).
    #[must_use]
    pub fn transfer_map(&self, teacher: &str, target: &str) -> Option<RegisterTransferMap> {
        let t_know = self.get(teacher)?;
        let s_know = self.get(target)?;

        let t_addrs: BTreeSet<u32> = t_know
            .gr_blobs
            .as_ref()?
            .unique_bundle_addrs()
            .into_iter()
            .collect();
        let s_addrs: BTreeSet<u32> = s_know
            .gr_blobs
            .as_ref()?
            .unique_bundle_addrs()
            .into_iter()
            .collect();

        let common: BTreeSet<u32> = t_addrs.intersection(&s_addrs).copied().collect();
        let teacher_only: BTreeSet<u32> = t_addrs.difference(&s_addrs).copied().collect();
        let target_only: BTreeSet<u32> = s_addrs.difference(&t_addrs).copied().collect();

        Some(RegisterTransferMap {
            teacher: teacher.to_string(),
            target: target.to_string(),
            common_registers: common,
            teacher_only_registers: teacher_only,
            target_only_registers: target_only,
        })
    }

    /// Find the best teacher for a target chip.
    ///
    /// Selects the teacher with the highest register overlap, strongly
    /// preferring same address space (method vs BAR0), then architecturally
    /// closer chips.
    #[must_use]
    pub fn best_teacher_for(&self, target: &str) -> Option<String> {
        let target_know = self.get(target)?;
        let target_sm = target_know.sm?;
        let target_as = target_know.address_space;
        let teachers = self.can_teach();

        teachers
            .into_iter()
            .filter(|t| *t != target)
            .max_by_key(|t| {
                let common = self.common_registers(target, t);
                let same_as = self.get(t).is_some_and(|k| k.address_space == target_as);
                let sm_dist = self
                    .get(t)
                    .and_then(|k| k.sm)
                    .map_or(0, |s| 1000u32.saturating_sub(s.abs_diff(target_sm)));
                // Same address space is the strongest signal (10000 bonus)
                let as_bonus: usize = if same_as { 10000 } else { 0 };
                (as_bonus + common, sm_dist as usize)
            })
            .map(str::to_string)
    }

    /// Compute the "generational register evolution" across all known architectures.
    ///
    /// Returns per-generation stats showing how registers evolve.
    #[must_use]
    pub fn generational_evolution(&self) -> Vec<GenerationStats> {
        let generations: BTreeMap<u32, Vec<&str>> = {
            let mut map: BTreeMap<u32, Vec<&str>> = BTreeMap::new();
            for arch in self.architectures.values() {
                if let Some(sm) = arch.sm {
                    map.entry(sm).or_default().push(&arch.chip);
                }
            }
            map
        };

        let mut stats = Vec::new();
        let gen_keys: Vec<u32> = generations.keys().copied().collect();

        for (i, &sm) in gen_keys.iter().enumerate() {
            let chips = &generations[&sm];
            let representative = chips[0];
            let reg_count = self.get(representative).map_or(0, |k| k.register_count);
            let has_firmware = self.get(representative).is_some_and(|k| k.has_firmware);

            let overlap_with_prev = if i > 0 {
                let prev_chip = generations[&gen_keys[i - 1]][0];
                Some(self.common_registers(representative, prev_chip))
            } else {
                None
            };

            stats.push(GenerationStats {
                sm,
                chip_count: chips.len(),
                representative: representative.to_string(),
                unique_registers: reg_count,
                has_firmware,
                overlap_with_previous: overlap_with_prev,
            });
        }

        stats
    }

    /// Insert architecture knowledge for testing (no firmware required).
    #[cfg(test)]
    pub fn insert_for_test(&mut self, arch: ArchKnowledge) {
        self.architectures.insert(arch.chip.clone(), arch);
    }

    /// Summary of the knowledge base.
    #[must_use]
    pub fn summary(&self) -> KnowledgeSummary {
        let total = self.architectures.len();
        let with_firmware = self
            .architectures
            .values()
            .filter(|a| a.has_firmware)
            .count();
        let needs_gsp = total - with_firmware;
        let total_registers: usize = self.architectures.values().map(|a| a.register_count).sum();

        KnowledgeSummary {
            architectures_known: total,
            with_native_firmware: with_firmware,
            needs_sovereign_gsp: needs_gsp,
            total_unique_registers: total_registers,
        }
    }
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

/// Detect address space from parsed firmware blobs.
fn detect_address_space(blobs: &GrFirmwareBlobs) -> AddressSpace {
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
fn has_gsp_or_pmu(chip: &str) -> bool {
    let base = nvidia_firmware_base().join(chip);
    base.join("gsp").is_dir() || base.join("pmu").is_dir()
}

/// Map chip codename to SM version.
fn sm_for_chip(chip: &str) -> Option<u32> {
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

#[cfg(test)]
mod tests {
    use super::super::firmware_parser::{FirmwareFormat, GrFirmwareBlobs};
    use super::super::gr_init::GrInitSequence;

    use super::*;

    fn bundle_bytes(addr: u32, value: u32) -> Vec<u8> {
        let mut v = Vec::with_capacity(8);
        v.extend_from_slice(&addr.to_le_bytes());
        v.extend_from_slice(&value.to_le_bytes());
        v
    }

    #[test]
    fn detect_address_space_method_offset_when_first_pair_below_bar0() {
        let blobs = GrFirmwareBlobs::from_legacy_bytes(
            &bundle_bytes(0x1_0000, 1),
            &[],
            &[],
            &[],
            "test_chip",
        );
        assert_eq!(
            super::detect_address_space(&blobs),
            AddressSpace::MethodOffset
        );
    }

    #[test]
    fn detect_address_space_bar0_when_first_pair_in_mmio_range() {
        let blobs = GrFirmwareBlobs::from_legacy_bytes(
            &bundle_bytes(0x0040_0000, 1),
            &[],
            &[],
            &[],
            "test_chip",
        );
        assert_eq!(super::detect_address_space(&blobs), AddressSpace::Bar0Mmio);
    }

    #[test]
    fn detect_address_space_unknown_when_bundle_empty() {
        let blobs = GrFirmwareBlobs::from_legacy_bytes(&[], &[], &[], &[], "empty");
        assert_eq!(super::detect_address_space(&blobs), AddressSpace::Unknown);
    }

    #[test]
    fn transfer_map_with_inserted_architectures() {
        let mut kb = GpuKnowledge::new();
        let blobs_a = GrFirmwareBlobs::from_legacy_bytes(
            &{
                let mut b = bundle_bytes(0x100, 1);
                b.extend_from_slice(&bundle_bytes(0x200, 2));
                b
            },
            &[],
            &[],
            &[],
            "chip_a",
        );
        let blobs_b = GrFirmwareBlobs::from_legacy_bytes(
            &{
                let mut b = bundle_bytes(0x200, 1);
                b.extend_from_slice(&bundle_bytes(0x300, 2));
                b
            },
            &[],
            &[],
            &[],
            "chip_b",
        );
        kb.insert_for_test(ArchKnowledge {
            chip: "chip_a".into(),
            sm: Some(86),
            vendor: GpuVendor::Nvidia,
            has_firmware: true,
            format: Some(FirmwareFormat::Legacy),
            address_space: AddressSpace::MethodOffset,
            gr_blobs: Some(blobs_a),
            gr_init: Some(GrInitSequence {
                chip: "chip_a".into(),
                writes: vec![],
            }),
            register_count: 2,
        });
        kb.insert_for_test(ArchKnowledge {
            chip: "chip_b".into(),
            sm: Some(89),
            vendor: GpuVendor::Nvidia,
            has_firmware: true,
            format: Some(FirmwareFormat::Legacy),
            address_space: AddressSpace::MethodOffset,
            gr_blobs: Some(blobs_b),
            gr_init: Some(GrInitSequence {
                chip: "chip_b".into(),
                writes: vec![],
            }),
            register_count: 2,
        });

        assert_eq!(kb.common_registers("chip_a", "chip_b"), 1);

        let map = kb.transfer_map("chip_a", "chip_b").expect("transfer map");
        assert_eq!(map.common_registers.len(), 1);
        assert!(map.common_registers.contains(&0x200));
        assert!(map.teacher_only_registers.contains(&0x100));
        assert!(map.target_only_registers.contains(&0x300));
        assert!((0.0..=100.0).contains(&map.coverage_pct()));
    }

    #[test]
    fn best_teacher_prefers_same_address_space_and_overlap() {
        let mut kb = GpuKnowledge::new();
        let shared = bundle_bytes(0x5000, 0);
        let blobs_target = GrFirmwareBlobs::from_legacy_bytes(&shared, &[], &[], &[], "target");
        let mut teacher_hi = shared.clone();
        teacher_hi.extend_from_slice(&bundle_bytes(0x6000, 0));
        let blobs_teacher =
            GrFirmwareBlobs::from_legacy_bytes(&teacher_hi, &[], &[], &[], "teacher");

        kb.insert_for_test(ArchKnowledge {
            chip: "target".into(),
            sm: Some(89),
            vendor: GpuVendor::Nvidia,
            has_firmware: false,
            format: Some(FirmwareFormat::Legacy),
            address_space: AddressSpace::MethodOffset,
            gr_blobs: Some(blobs_target),
            gr_init: Some(GrInitSequence {
                chip: "target".into(),
                writes: vec![],
            }),
            register_count: 1,
        });
        kb.insert_for_test(ArchKnowledge {
            chip: "teacher".into(),
            sm: Some(86),
            vendor: GpuVendor::Nvidia,
            has_firmware: true,
            format: Some(FirmwareFormat::Legacy),
            address_space: AddressSpace::MethodOffset,
            gr_blobs: Some(blobs_teacher),
            gr_init: Some(GrInitSequence {
                chip: "teacher".into(),
                writes: vec![],
            }),
            register_count: 2,
        });

        assert_eq!(kb.best_teacher_for("target").as_deref(), Some("teacher"));
    }

    #[test]
    fn generational_evolution_from_synthetic_entries() {
        let mut kb = GpuKnowledge::new();
        let blobs = GrFirmwareBlobs::from_legacy_bytes(&bundle_bytes(0x100, 0), &[], &[], &[], "x");
        kb.insert_for_test(ArchKnowledge {
            chip: "early".into(),
            sm: Some(75),
            vendor: GpuVendor::Nvidia,
            has_firmware: true,
            format: Some(FirmwareFormat::Legacy),
            address_space: AddressSpace::MethodOffset,
            gr_blobs: Some(blobs),
            gr_init: Some(GrInitSequence {
                chip: "early".into(),
                writes: vec![],
            }),
            register_count: 3,
        });
        let blobs2 =
            GrFirmwareBlobs::from_legacy_bytes(&bundle_bytes(0x100, 0), &[], &[], &[], "y");
        kb.insert_for_test(ArchKnowledge {
            chip: "late".into(),
            sm: Some(86),
            vendor: GpuVendor::Nvidia,
            has_firmware: false,
            format: Some(FirmwareFormat::Legacy),
            address_space: AddressSpace::MethodOffset,
            gr_blobs: Some(blobs2),
            gr_init: Some(GrInitSequence {
                chip: "late".into(),
                writes: vec![],
            }),
            register_count: 5,
        });

        let evo = kb.generational_evolution();
        assert_eq!(evo.len(), 2);
        assert!(evo[0].overlap_with_previous.is_none());
        assert_eq!(evo[1].overlap_with_previous, Some(1));
    }

    #[test]
    fn discover_and_learn() {
        let mut kb = GpuKnowledge::new();
        kb.learn_nvidia_firmware();

        let summary = kb.summary();
        tracing::debug!("Knowledge base: {summary:?}");

        let chips = kb.known_chips();
        tracing::debug!("Known chips: {chips:?}");

        let needs = kb.needs_sovereign_gsp();
        tracing::debug!("Needs sovereign GSP: {needs:?}");

        let teachers = kb.can_teach();
        tracing::debug!("Can teach: {teachers:?}");
    }

    #[test]
    fn cross_architecture_comparison() {
        let mut kb = GpuKnowledge::new();
        kb.learn_nvidia_firmware();

        let rich_chips: Vec<&str> = kb
            .known_chips()
            .into_iter()
            .filter(|c| kb.get(c).map_or(0, |k| k.register_count) > 100)
            .collect();

        tracing::debug!("=== Cross-architecture register comparison ===");
        for (i, a) in rich_chips.iter().enumerate() {
            let a_regs = kb.get(a).map_or(0, |k| k.register_count);
            let a_sm = kb.get(a).and_then(|k| k.sm).unwrap_or(0);
            tracing::debug!("{a} (SM{a_sm}): {a_regs} unique registers");
            for b in &rich_chips[i + 1..] {
                let common = kb.common_registers(a, b);
                let b_regs = kb.get(b).map_or(0, |k| k.register_count);
                let pct_a = if a_regs > 0 { common * 100 / a_regs } else { 0 };
                let pct_b = if b_regs > 0 { common * 100 / b_regs } else { 0 };
                tracing::debug!("  vs {b}: {common} common ({pct_a}% of {a}, {pct_b}% of {b})");
            }
        }
    }

    #[test]
    fn transfer_map_gv100() {
        let mut kb = GpuKnowledge::new();
        kb.learn_nvidia_firmware();

        let best = kb.best_teacher_for("gv100");
        tracing::debug!("Best teacher for GV100: {best:?}");

        if let Some(teacher) = &best
            && let Some(map) = kb.transfer_map(teacher, "gv100")
        {
            tracing::debug!(
                "{} -> gv100: {} common, {} teacher-only, {} target-only ({:.1}% coverage)",
                teacher,
                map.common_registers.len(),
                map.teacher_only_registers.len(),
                map.target_only_registers.len(),
                map.coverage_pct()
            );
        }

        // Also check GA102 as teacher
        if let Some(map) = kb.transfer_map("ga102", "gv100") {
            tracing::debug!(
                "ga102 -> gv100: {} common, {} teacher-only, {} target-only ({:.1}% coverage)",
                map.common_registers.len(),
                map.teacher_only_registers.len(),
                map.target_only_registers.len(),
                map.coverage_pct()
            );
        }
    }

    #[test]
    fn generational_evolution() {
        let mut kb = GpuKnowledge::new();
        kb.learn_nvidia_firmware();

        let evo = kb.generational_evolution();
        tracing::debug!("=== Generational Register Evolution ===");
        for gs in &evo {
            let overlap = gs
                .overlap_with_previous
                .map(|o| format!(" (overlap: {o})"))
                .unwrap_or_default();
            tracing::debug!(
                "SM{:2}: {} ({} chips) — {} regs, fw={}{overlap}",
                gs.sm,
                gs.representative,
                gs.chip_count,
                gs.unique_registers,
                gs.has_firmware
            );
        }
    }

    #[test]
    fn sm_mapping() {
        assert_eq!(sm_for_chip("gv100"), Some(70));
        assert_eq!(sm_for_chip("ga102"), Some(86));
        assert_eq!(sm_for_chip("ad104"), Some(89));
        assert_eq!(sm_for_chip("unknown"), None);
    }

    #[test]
    fn sm_for_chip_all_known_chips() {
        // Kepler (SM 32)
        assert_eq!(sm_for_chip("gk20a"), Some(32));

        // Maxwell (SM 52)
        assert_eq!(sm_for_chip("gm200"), Some(52));
        assert_eq!(sm_for_chip("gm204"), Some(52));
        assert_eq!(sm_for_chip("gm206"), Some(52));
        assert_eq!(sm_for_chip("gm20b"), Some(52));

        // Pascal (SM 60)
        assert_eq!(sm_for_chip("gp100"), Some(60));
        assert_eq!(sm_for_chip("gp102"), Some(60));
        assert_eq!(sm_for_chip("gp104"), Some(60));
        assert_eq!(sm_for_chip("gp106"), Some(60));
        assert_eq!(sm_for_chip("gp107"), Some(60));
        assert_eq!(sm_for_chip("gp108"), Some(60));
        assert_eq!(sm_for_chip("gp10b"), Some(60));

        // Volta (SM 70)
        assert_eq!(sm_for_chip("gv100"), Some(70));

        // Turing (SM 75)
        assert_eq!(sm_for_chip("tu102"), Some(75));
        assert_eq!(sm_for_chip("tu104"), Some(75));
        assert_eq!(sm_for_chip("tu106"), Some(75));
        assert_eq!(sm_for_chip("tu10x"), Some(75));
        assert_eq!(sm_for_chip("tu116"), Some(75));
        assert_eq!(sm_for_chip("tu117"), Some(75));

        // Ampere GA100 (SM 80)
        assert_eq!(sm_for_chip("ga100"), Some(80));

        // Ampere GA102/103/104/106/107 (SM 86)
        assert_eq!(sm_for_chip("ga102"), Some(86));
        assert_eq!(sm_for_chip("ga103"), Some(86));
        assert_eq!(sm_for_chip("ga104"), Some(86));
        assert_eq!(sm_for_chip("ga106"), Some(86));
        assert_eq!(sm_for_chip("ga107"), Some(86));

        // Ada Lovelace (SM 89)
        assert_eq!(sm_for_chip("ad102"), Some(89));
        assert_eq!(sm_for_chip("ad103"), Some(89));
        assert_eq!(sm_for_chip("ad104"), Some(89));
        assert_eq!(sm_for_chip("ad106"), Some(89));
        assert_eq!(sm_for_chip("ad107"), Some(89));

        // Unknown chips
        assert_eq!(sm_for_chip("unknown"), None);
        assert_eq!(sm_for_chip(""), None);
        assert_eq!(sm_for_chip("gb200"), None); // Blackwell not yet in table
    }

    #[test]
    fn empty_knowledge_base() {
        let kb = GpuKnowledge::new();
        assert!(kb.known_chips().is_empty());
        assert!(kb.needs_sovereign_gsp().is_empty());
        assert!(kb.can_teach().is_empty());
        assert!(kb.get("gv100").is_none());
        assert!(kb.best_teacher_for("gv100").is_none());
        let summary = kb.summary();
        assert_eq!(summary.architectures_known, 0);
        assert_eq!(summary.with_native_firmware, 0);
        assert_eq!(summary.needs_sovereign_gsp, 0);
    }

    #[test]
    fn empty_knowledge_common_registers() {
        let kb = GpuKnowledge::new();
        assert_eq!(kb.common_registers("gv100", "ga102"), 0);
    }

    #[test]
    fn empty_knowledge_transfer_map() {
        let kb = GpuKnowledge::new();
        assert!(kb.transfer_map("ga102", "gv100").is_none());
    }

    #[test]
    fn empty_knowledge_generational_evolution() {
        let kb = GpuKnowledge::new();
        let evo = kb.generational_evolution();
        assert!(evo.is_empty());
    }

    #[test]
    fn register_transfer_map_coverage() {
        use std::collections::BTreeSet;
        let map = RegisterTransferMap {
            teacher: "ga102".into(),
            target: "gv100".into(),
            common_registers: BTreeSet::from([0x100, 0x200]),
            teacher_only_registers: BTreeSet::from([0x300]),
            target_only_registers: BTreeSet::from([0x400, 0x500]),
        };
        let pct = map.coverage_pct();
        // 2 common of (2 common + 2 target_only) = 50%
        assert!(
            (49.0..=51.0).contains(&pct),
            "expected ~50% coverage, got {pct}"
        );
    }

    #[test]
    fn register_transfer_map_coverage_empty() {
        use std::collections::BTreeSet;
        let map = RegisterTransferMap {
            teacher: "a".into(),
            target: "b".into(),
            common_registers: BTreeSet::new(),
            teacher_only_registers: BTreeSet::new(),
            target_only_registers: BTreeSet::new(),
        };
        assert_eq!(map.coverage_pct(), 0.0);
    }

    #[test]
    fn address_space_equality() {
        assert_eq!(AddressSpace::MethodOffset, AddressSpace::MethodOffset);
        assert_ne!(AddressSpace::MethodOffset, AddressSpace::Bar0Mmio);
        assert_ne!(AddressSpace::Bar0Mmio, AddressSpace::Unknown);
    }

    #[test]
    fn gpu_vendor_equality() {
        assert_eq!(GpuVendor::Nvidia, GpuVendor::Nvidia);
        assert_ne!(GpuVendor::Nvidia, GpuVendor::Amd);
        assert_ne!(GpuVendor::Amd, GpuVendor::Other);
    }

    #[test]
    fn knowledge_summary_debug() {
        let summary = KnowledgeSummary {
            architectures_known: 5,
            with_native_firmware: 3,
            needs_sovereign_gsp: 2,
            total_unique_registers: 100,
        };
        let debug = format!("{summary:?}");
        assert!(debug.contains("5"));
        assert!(debug.contains("100"));
    }
}
