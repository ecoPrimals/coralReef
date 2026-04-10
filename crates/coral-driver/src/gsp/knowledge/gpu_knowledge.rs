// SPDX-License-Identifier: AGPL-3.0-or-later

use std::collections::{BTreeMap, BTreeSet};

use super::super::firmware_parser::GrFirmwareBlobs;
use super::super::firmware_source::{FilesystemFirmwareSource, NvidiaFirmwareSource};
use super::super::gr_init::GrInitSequence;
use super::chip::{detect_address_space, has_gsp_or_pmu, sm_for_chip};
use super::types::{
    ArchKnowledge, GenerationStats, GpuVendor, KnowledgeSummary, RegisterTransferMap,
};

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
