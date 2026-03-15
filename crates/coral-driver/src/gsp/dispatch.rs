// SPDX-License-Identifier: AGPL-3.0-only
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
//! base and produces advisory hints that `barraCuda` can use for routing.

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

    let fp64_full = has_full_rate_fp64(sm) || chip_has_full_fp64(chip);

    Some(DispatchHints {
        chip: chip.to_string(),
        sm,
        recommended_workgroup_size: workgroup_size_for_sm(sm),
        max_workgroups_per_sm: max_workgroups_for_sm(sm),
        native_fp64_full_rate: fp64_full,
        needs_sovereign_init: !arch.has_firmware,
        init_address_space: arch.address_space,
        init_register_count: arch.register_count,
        best_teacher,
        teacher_coverage_pct: teacher_coverage,
    })
}

/// Recommended workgroup size based on SM architecture.
///
/// Based on warp size (32) and observed optimal occupancy patterns.
fn workgroup_size_for_sm(sm: u32) -> u32 {
    match sm {
        // Maxwell/Volta: 128 (independent thread scheduling benefits smaller groups)
        50..=53 | 70..=72 => 128,
        // Pascal/Turing/Ampere/Ada+: 256
        _ => 256,
    }
}

/// Maximum concurrent workgroups per SM.
fn max_workgroups_for_sm(sm: u32) -> u32 {
    match sm {
        // Maxwell/Pascal/Volta: 32 CTA per SM
        50..=72 => 32,
        // Turing/Ampere/Ada+: 16 CTA per SM (larger warps)
        _ => 16,
    }
}

/// Whether this SM has full-rate FP64 (1:2 ratio with FP32).
///
/// Only GV100 (SM 7.0, Titan V / Tesla V100) has confirmed full-rate FP64.
/// GP100 (SM 6.0, Tesla P100) also has 1:2 FP64 but shares SM version with
/// consumer Pascal which is 1:32. We use chip-level detection here, so SM 6.0
/// gets "maybe" — the caller should refine with actual chip ID.
fn has_full_rate_fp64(sm: u32) -> bool {
    sm == 70
}

/// Chip-specific full-rate FP64 detection.
///
/// GP100 (Tesla P100) has 1:2 FP64 despite sharing SM 6.0 with consumer Pascal.
fn chip_has_full_fp64(chip: &str) -> bool {
    matches!(chip, "gp100" | "gv100")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dispatch_hints_for_all() {
        let mut kb = GpuKnowledge::new();
        kb.learn_nvidia_firmware();

        let hints = build_dispatch_hints(&kb);
        assert!(!hints.is_empty());

        eprintln!("=== Dispatch Hints ===");
        for h in &hints {
            eprintln!(
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
            eprintln!("GV100 hints: {h:?}");
        }
    }
}
