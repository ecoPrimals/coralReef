// SPDX-License-Identifier: AGPL-3.0-or-later
//! Dispatch optimizer — learned hints for compute workloads.
//!
//! Uses knowledge gathered from firmware analysis and runtime observation
//! to provide hardware-aware dispatch hints:
//!
//! - Optimal workgroup size for the target architecture
//! - Memory placement recommendations (shared vs global)
//! - Whether native FP64 is available or DF64 emulation is needed
//! - Register pressure estimates from firmware register counts
//!
//! This module does NOT touch hardware — it purely analyzes the knowledge
//! base and produces advisory hints for compute routing.

use super::knowledge::{AddressSpace, GpuKnowledge};

/// Hardware-aware dispatch hints for a specific GPU.
#[derive(Debug, Clone, serde::Serialize)]
pub struct DispatchHints {
    /// Target chip codename.
    pub chip: String,
    /// SM architecture version.
    pub sm: u32,
    /// Recommended workgroup size (threads per workgroup).
    pub recommended_workgroup_size: u32,
    /// Maximum concurrent workgroups per SM.
    pub max_workgroups_per_sm: u32,
    /// Whether native FP64 at full rate is available.
    pub native_fp64_full_rate: bool,
    /// Whether the chip needs sovereign GSP for compute.
    pub needs_sovereign_init: bool,
    /// Address space for init data.
    pub init_address_space: AddressSpace,
    /// Register complexity (unique registers in init).
    pub init_register_count: usize,
    /// Best teacher chip for learning init patterns.
    pub best_teacher: Option<String>,
    /// Coverage percentage from best teacher.
    pub teacher_coverage_pct: Option<f64>,
}

/// Build dispatch hints for all known architectures.
#[must_use]
pub fn build_dispatch_hints(kb: &GpuKnowledge) -> Vec<DispatchHints> {
    kb.known_chips()
        .into_iter()
        .filter_map(|chip| build_hint_for(kb, chip))
        .collect()
}

/// Build dispatch hint for a single chip.
#[must_use]
pub fn build_hint_for(kb: &GpuKnowledge, chip: &str) -> Option<DispatchHints> {
    let arch = kb.get(chip)?;
    let sm = arch.sm?;

    let best_teacher = kb.best_teacher_for(chip);
    let teacher_coverage = best_teacher
        .as_ref()
        .and_then(|t| kb.transfer_map(t, chip).map(|m| m.coverage_pct()));

    let profile = crate::nv::generation::profile_for_sm(sm);
    let fp64_full = profile.has_full_rate_fp64 || chip_has_full_fp64(chip);

    Some(DispatchHints {
        chip: chip.to_string(),
        sm,
        recommended_workgroup_size: profile.recommended_workgroup_size,
        max_workgroups_per_sm: profile.max_cta_per_sm,
        native_fp64_full_rate: fp64_full,
        needs_sovereign_init: !arch.has_firmware,
        init_address_space: arch.address_space,
        init_register_count: arch.register_count,
        best_teacher,
        teacher_coverage_pct: teacher_coverage,
    })
}

/// Chip-specific full-rate FP64 override.
///
/// Catches cases where the chip's profile `has_full_rate_fp64` might be
/// conservative (e.g. the Pascal profile is keyed to GP100 but consumer
/// GP10x shares the same SM range).
fn chip_has_full_fp64(chip: &str) -> bool {
    matches!(chip, "gp100" | "gv100" | "ga100" | "gh100" | "gb100")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gsp::knowledge::{AddressSpace, ArchKnowledge, GpuKnowledge, GpuVendor};

    fn arch_stub(chip: &str, sm: u32, has_firmware: bool, register_count: usize) -> ArchKnowledge {
        ArchKnowledge {
            chip: chip.to_string(),
            sm: Some(sm),
            vendor: GpuVendor::Nvidia,
            has_firmware,
            format: None,
            address_space: AddressSpace::MethodOffset,
            gr_blobs: None,
            gr_init: None,
            register_count,
        }
    }

    #[test]
    fn build_hint_for_synthetic_sm_workgroup_and_fp64() {
        let mut kb = GpuKnowledge::new();
        kb.insert_for_test(arch_stub("gv100", 70, false, 12));
        kb.insert_for_test(arch_stub("tu104", 75, false, 8));
        kb.insert_for_test(arch_stub("ga102", 86, false, 20));
        kb.insert_for_test(arch_stub("ad102", 89, false, 24));
        kb.insert_for_test(arch_stub("gp100", 60, false, 5));

        let h70 = build_hint_for(&kb, "gv100").expect("gv100");
        assert_eq!(h70.sm, 70);
        assert_eq!(h70.recommended_workgroup_size, 128);
        assert_eq!(h70.max_workgroups_per_sm, 32);
        assert!(h70.native_fp64_full_rate);
        assert!(h70.needs_sovereign_init);

        let h75 = build_hint_for(&kb, "tu104").expect("tu104");
        assert_eq!(h75.recommended_workgroup_size, 256);
        assert_eq!(h75.max_workgroups_per_sm, 16);
        assert!(!h75.native_fp64_full_rate);

        let h86 = build_hint_for(&kb, "ga102").expect("ga102");
        assert_eq!(h86.recommended_workgroup_size, 256);
        assert!(!h86.native_fp64_full_rate);

        let gp = build_hint_for(&kb, "gp100").expect("gp100");
        assert_eq!(gp.sm, 60);
        assert!(gp.native_fp64_full_rate);
    }

    #[test]
    fn build_dispatch_hints_multi_chip_synthetic() {
        let mut kb = GpuKnowledge::new();
        kb.insert_for_test(arch_stub("gv100", 70, false, 1));
        kb.insert_for_test(arch_stub("ga102", 86, false, 2));
        kb.insert_for_test(arch_stub("ad102", 89, false, 3));

        let hints = build_dispatch_hints(&kb);
        assert_eq!(hints.len(), 3);

        let by_chip: std::collections::HashMap<_, _> =
            hints.iter().map(|h| (h.chip.as_str(), h)).collect();
        assert_eq!(by_chip["gv100"].recommended_workgroup_size, 128);
        assert_eq!(by_chip["ga102"].recommended_workgroup_size, 256);
        assert_eq!(by_chip["ad102"].max_workgroups_per_sm, 16);
        assert!(hints.iter().all(|h| h.needs_sovereign_init));
        assert!(hints.iter().all(|h| h.best_teacher.is_none()));
    }

    #[test]
    fn dispatch_hints_for_all() {
        let mut kb = GpuKnowledge::new();
        kb.learn_nvidia_firmware();

        let hints = build_dispatch_hints(&kb);
        assert!(!hints.is_empty());

        tracing::debug!("=== Dispatch Hints ===");
        for h in &hints {
            tracing::debug!(
                "{:8} SM{:2}: wg={:3} fp64={:5} sovereign={:5} regs={:4} teacher={:8} ({:.1}%)",
                h.chip,
                h.sm,
                h.recommended_workgroup_size,
                h.native_fp64_full_rate,
                h.needs_sovereign_init,
                h.init_register_count,
                h.best_teacher.as_deref().unwrap_or("-"),
                h.teacher_coverage_pct.unwrap_or(0.0),
            );
        }
    }

    #[test]
    fn volta_hints() {
        let mut kb = GpuKnowledge::new();
        kb.learn_nvidia_firmware();

        if let Some(h) = build_hint_for(&kb, "gv100") {
            assert_eq!(h.sm, 70);
            assert!(h.native_fp64_full_rate);
            assert!(h.needs_sovereign_init);
            assert_eq!(h.recommended_workgroup_size, 128);
            assert!(h.best_teacher.is_some());
            tracing::debug!("GV100 hints: {h:?}");
        }
    }
}
