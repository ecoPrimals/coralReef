// SPDX-License-Identifier: AGPL-3.0-or-later

use super::super::firmware_parser::{FirmwareFormat, GrFirmwareBlobs};
use super::super::gr_init::GrInitSequence;

use super::GpuKnowledge;
use super::chip::{detect_address_space, sm_for_chip};
use super::types::{AddressSpace, ArchKnowledge, GpuVendor, KnowledgeSummary, RegisterTransferMap};

fn bundle_bytes(addr: u32, value: u32) -> Vec<u8> {
    let mut v = Vec::with_capacity(8);
    v.extend_from_slice(&addr.to_le_bytes());
    v.extend_from_slice(&value.to_le_bytes());
    v
}

#[test]
fn detect_address_space_method_offset_when_first_pair_below_bar0() {
    let blobs =
        GrFirmwareBlobs::from_legacy_bytes(&bundle_bytes(0x1_0000, 1), &[], &[], &[], "test_chip");
    assert_eq!(detect_address_space(&blobs), AddressSpace::MethodOffset);
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
    assert_eq!(detect_address_space(&blobs), AddressSpace::Bar0Mmio);
}

#[test]
fn detect_address_space_unknown_when_bundle_empty() {
    let blobs = GrFirmwareBlobs::from_legacy_bytes(&[], &[], &[], &[], "empty");
    assert_eq!(detect_address_space(&blobs), AddressSpace::Unknown);
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
    let blobs_teacher = GrFirmwareBlobs::from_legacy_bytes(&teacher_hi, &[], &[], &[], "teacher");

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
    let blobs2 = GrFirmwareBlobs::from_legacy_bytes(&bundle_bytes(0x100, 0), &[], &[], &[], "y");
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
