// SPDX-License-Identifier: AGPL-3.0-only
//! Per-subsystem register validation against nouveau reference snapshots.
//!
//! Each subsystem validator compares a set of key registers from the running GPU
//! state (read via BAR0) against expected values from a nouveau reference BAR0
//! snapshot. This supports the trace-and-replace strategy: validate that our Rust
//! implementation produces identical register state to nouveau at each init stage.

use crate::error::DriverError;
use crate::vfio::device::MappedBar;
use std::collections::BTreeMap;

/// A single register comparison result.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RegisterComparison {
    pub offset: usize,
    pub name: String,
    pub actual: u32,
    pub expected: u32,
    pub mask: u32,
    pub matches: bool,
}

/// Result of validating one subsystem.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct SubsystemValidation {
    pub subsystem: String,
    pub total_probes: usize,
    pub matched: usize,
    pub mismatched: usize,
    pub unreadable: usize,
    pub comparisons: Vec<RegisterComparison>,
}

impl SubsystemValidation {
    pub fn passed(&self) -> bool {
        self.mismatched == 0 && self.unreadable == 0
    }

    pub fn summary(&self) -> String {
        let status = if self.passed() { "PASS" } else { "FAIL" };
        format!(
            "[{status}] {}: {}/{} matched, {} mismatched, {} unreadable",
            self.subsystem, self.matched, self.total_probes, self.mismatched, self.unreadable,
        )
    }
}

/// Register probe definition: offset, name, and optional comparison mask.
/// A mask of 0xFFFFFFFF means exact match; other masks allow ignoring volatile bits.
struct Probe {
    offset: usize,
    name: &'static str,
    mask: u32,
}

const fn probe(offset: usize, name: &'static str) -> Probe {
    Probe {
        offset,
        name,
        mask: 0xFFFF_FFFF,
    }
}

const fn probe_masked(offset: usize, name: &'static str, mask: u32) -> Probe {
    Probe {
        offset,
        name,
        mask,
    }
}

fn validate_probes(
    bar0: &MappedBar,
    subsystem: &str,
    probes: &[Probe],
    reference: &BTreeMap<usize, u32>,
) -> SubsystemValidation {
    let mut comparisons = Vec::with_capacity(probes.len());
    let mut matched = 0usize;
    let mut mismatched = 0usize;
    let mut unreadable = 0usize;

    for p in probes {
        let actual = match bar0.read_u32(p.offset) {
            Ok(v) => v,
            Err(_) => {
                unreadable += 1;
                comparisons.push(RegisterComparison {
                    offset: p.offset,
                    name: p.name.to_string(),
                    actual: 0xDEAD_DEAD,
                    expected: reference.get(&p.offset).copied().unwrap_or(0),
                    mask: p.mask,
                    matches: false,
                });
                continue;
            }
        };

        let expected = reference.get(&p.offset).copied().unwrap_or(0);
        let matches = (actual & p.mask) == (expected & p.mask);

        if matches {
            matched += 1;
        } else {
            mismatched += 1;
        }

        comparisons.push(RegisterComparison {
            offset: p.offset,
            name: p.name.to_string(),
            actual,
            expected,
            mask: p.mask,
            matches,
        });
    }

    SubsystemValidation {
        subsystem: subsystem.to_string(),
        total_probes: probes.len(),
        matched,
        mismatched,
        unreadable,
        comparisons,
    }
}

// ─── Subsystem 1: PMC + Engine Gating ─────────────────────────────────

const PMC_PROBES: &[Probe] = &[
    probe(0x000000, "PMC_BOOT_0"),
    probe(0x000004, "PMC_BOOT_1"),
    probe(0x000200, "PMC_ENABLE"),
    probe(0x000204, "PMC_ENABLE_HI"),
    probe(0x000260, "PMC_UNK260"),
    probe_masked(0x000100, "PMC_INTR_0", 0xFFFF_0000),
    probe(0x000600, "PMC_DEVICE_ENABLE"),
];

/// Validate PMC + Engine Gating state against nouveau reference.
pub fn validate_pmc(
    bar0: &MappedBar,
    reference: &BTreeMap<usize, u32>,
) -> SubsystemValidation {
    validate_probes(bar0, "PMC_ENGINE_GATING", PMC_PROBES, reference)
}

// ─── Subsystem 2: PRIV_RING + Topology ────────────────────────────────

const TOPOLOGY_PROBES: &[Probe] = &[
    probe(0x120100, "PRI_RING_INTR"),
    probe(0x122000, "PRI_MASTER_CTRL"),
    probe(0x022430, "PTOP_DEVICE_INFO_0"),
    probe(0x022434, "PTOP_DEVICE_INFO_1"),
    probe(0x022438, "PTOP_DEVICE_INFO_2"),
    probe(0x02243C, "PTOP_DEVICE_INFO_3"),
    probe(0x022440, "PTOP_DEVICE_INFO_4"),
    probe(0x022448, "PTOP_DEVICE_INFO_5"),
];

/// Validate PRIV_RING + Topology discovery state against nouveau reference.
pub fn validate_topology(
    bar0: &MappedBar,
    reference: &BTreeMap<usize, u32>,
) -> SubsystemValidation {
    validate_probes(bar0, "PRI_TOPOLOGY", TOPOLOGY_PROBES, reference)
}

// ─── Subsystem 3: PFB + Memory Controller ─────────────────────────────

const PFB_PROBES: &[Probe] = &[
    probe(0x100000, "PFB_CFG0"),
    probe(0x100004, "PFB_CFG1"),
    probe(0x100800, "FBHUB_CFG"),
    probe(0x100804, "FBHUB_NUM_ACTIVE_LTCS"),
    probe(0x100C10, "PFB_NISO_FLUSH_SYSMEM_ADDR"),
    probe(0x100C80, "PFB_NISO_UNK_C80"),
    probe(0x100CC8, "PFB_NISO_UNK_CC8"),
    probe(0x12006C, "FBP_BROADCAST_COUNT"),
];

/// Validate PFB + Memory Controller state against nouveau reference.
pub fn validate_pfb(
    bar0: &MappedBar,
    reference: &BTreeMap<usize, u32>,
) -> SubsystemValidation {
    validate_probes(bar0, "PFB_MEMORY", PFB_PROBES, reference)
}

// ─── Subsystem 4: Falcon Boot Chain ───────────────────────────────────

const FALCON_PROBES: &[Probe] = &[
    // SEC2
    probe(0x840100, "SEC2_CPUCTL"),
    probe(0x840104, "SEC2_BOOTVEC"),
    probe(0x840240, "SEC2_SCTL"),
    probe(0x840110, "SEC2_PC"),
    probe(0x840040, "SEC2_MAILBOX0"),
    probe(0x840044, "SEC2_MAILBOX1"),
    probe(0x840108, "SEC2_HWCFG"),
    // ACR
    probe(0x862100, "ACR_CPUCTL"),
    probe(0x862240, "ACR_SCTL"),
    // FECS
    probe(0x409100, "FECS_CPUCTL"),
    probe(0x409104, "FECS_BOOTVEC"),
    probe(0x409240, "FECS_SCTL"),
    probe(0x409110, "FECS_PC"),
    probe(0x409040, "FECS_MAILBOX0"),
    probe(0x409044, "FECS_MAILBOX1"),
    probe(0x409108, "FECS_HWCFG"),
    // GPCCS
    probe(0x41A100, "GPCCS_CPUCTL"),
    probe(0x41A104, "GPCCS_BOOTVEC"),
    probe(0x41A240, "GPCCS_SCTL"),
    probe(0x41A110, "GPCCS_PC"),
    probe(0x41A040, "GPCCS_MAILBOX0"),
    probe(0x41A108, "GPCCS_HWCFG"),
    // PMU
    probe(0x10A100, "PMU_CPUCTL"),
    probe(0x10A240, "PMU_SCTL"),
    probe(0x10A040, "PMU_MAILBOX0"),
    probe(0x10A044, "PMU_MAILBOX1"),
    probe(0x10A108, "PMU_HWCFG"),
];

/// Validate Falcon Boot Chain state against nouveau reference.
///
/// This captures the exact register state of all falcon processors (SEC2, ACR,
/// FECS, GPCCS, PMU) for comparison with nouveau's post-boot state. The PRAMIN
/// addresses where firmware was loaded and the trigger registers are inferred
/// from differences in CPUCTL, BOOTVEC, SCTL, and PC values.
pub fn validate_falcons(
    bar0: &MappedBar,
    reference: &BTreeMap<usize, u32>,
) -> SubsystemValidation {
    validate_probes(bar0, "FALCON_BOOT", FALCON_PROBES, reference)
}

/// Capture falcon boot trace: reads all falcon state registers and returns them
/// as a map for later comparison. This is the "trace" half of the falcon-trace todo.
pub fn capture_falcon_state(bar0: &MappedBar) -> BTreeMap<usize, u32> {
    let mut state = BTreeMap::new();
    for p in FALCON_PROBES {
        if let Ok(val) = bar0.read_u32(p.offset) {
            state.insert(p.offset, val);
        }
    }
    state
}

/// PRAMIN regions where nouveau loads falcon firmware blobs.
/// These are read-back addresses, not directly written by nouveau;
/// the firmware upload protocol uses PRAMIN window writes.
pub const FALCON_PRAMIN_REGIONS: &[(&str, usize, usize)] = &[
    ("SEC2_IMEM_BASE", 0x700000, 0x708000),
    ("SEC2_DMEM_BASE", 0x708000, 0x70C000),
    ("ACR_UCODE_BASE", 0x70C000, 0x710000),
];

/// Falcon trigger registers — these are written by nouveau to start each falcon.
pub const FALCON_TRIGGER_REGS: &[(&str, usize)] = &[
    ("SEC2_CPUCTL", 0x840100),
    ("SEC2_DMACTL", 0x84010C),
    ("SEC2_ITFEN", 0x840048),
    ("FECS_CPUCTL", 0x409100),
    ("GPCCS_CPUCTL", 0x41A100),
    ("PMU_CPUCTL", 0x10A100),
];

// ─── Subsystem 5: GR Engine Init ──────────────────────────────────────

const GR_PROBES: &[Probe] = &[
    probe_masked(0x400100, "PGRAPH_INTR", 0x0000_0000),
    probe(0x400108, "PGRAPH_FECS_INTR"),
    probe(0x400500, "GR_MASTER_ENABLE"),
    probe(0x400700, "GR_STATUS"),
    probe(0x404000, "PGRAPH_GR_STATUS"),
    probe(0x404600, "PGRAPH_EXCEPTION"),
    probe(0x409C24, "FECS_EXCEPTION_EN"),
    // GPC MMU and context registers configured by apply_dynamic_gr_init
    probe(0x418880, "GPC_MMU_CFG0"),
    probe(0x418890, "GPC_MMU_CFG2"),
    probe(0x418894, "GPC_MMU_CFG3"),
    probe(0x4188AC, "GPC_ACTIVE_LTCS"),
    probe(0x41833C, "GPC_LTC_BROADCAST"),
    // Hub and exception enables
    probe(0x400124, "PGRAPH_TRAP_EN"),
    probe(0x40013C, "PGRAPH_IBUS_INTR_EN"),
    probe(0x408030, "PGRAPH_DISPATCH_CFG"),
    probe(0x405840, "PGRAPH_ACTIVITY_0"),
    probe(0x407020, "PGRAPH_TILE_MAP"),
];

/// Validate GR Engine Init state against nouveau reference.
pub fn validate_gr(
    bar0: &MappedBar,
    reference: &BTreeMap<usize, u32>,
) -> SubsystemValidation {
    validate_probes(bar0, "GR_ENGINE_INIT", GR_PROBES, reference)
}

// ─── Subsystem 6: PFIFO + Channel ─────────────────────────────────────

const PFIFO_PROBES: &[Probe] = &[
    probe(0x002200, "PFIFO_ENABLE"),
    probe_masked(0x002100, "PFIFO_INTR_0", 0x0000_0000),
    probe(0x002140, "PFIFO_INTR_EN_0"),
    probe(0x002504, "PFIFO_SCHED_EN"),
    probe(0x002630, "PFIFO_SCHED_DISABLE"),
    probe(0x002004, "PFIFO_PBDMA_MAP"),
    // PBDMA0
    probe_masked(0x040100, "PBDMA0_INTR", 0x0000_0000),
    probe(0x040140, "PBDMA0_INTR_EN"),
    // PCCSR channel 0 (check if any channel is bound)
    probe(0x800000, "PCCSR_INST_0"),
    probe(0x800004, "PCCSR_CHANNEL_0"),
];

/// Validate PFIFO + Channel state against nouveau reference.
pub fn validate_pfifo(
    bar0: &MappedBar,
    reference: &BTreeMap<usize, u32>,
) -> SubsystemValidation {
    validate_probes(bar0, "PFIFO_CHANNEL", PFIFO_PROBES, reference)
}

// ─── Full pipeline validation ─────────────────────────────────────────

/// Run all subsystem validations and return results.
pub fn validate_all(
    bar0: &MappedBar,
    reference: &BTreeMap<usize, u32>,
) -> Vec<SubsystemValidation> {
    vec![
        validate_pmc(bar0, reference),
        validate_topology(bar0, reference),
        validate_pfb(bar0, reference),
        validate_falcons(bar0, reference),
        validate_gr(bar0, reference),
        validate_pfifo(bar0, reference),
    ]
}

/// Load a nouveau reference BAR0 snapshot from JSON (as produced by snapshot-bar0.py).
pub fn load_reference_snapshot(
    path: &std::path::Path,
) -> Result<BTreeMap<usize, u32>, DriverError> {
    let content = std::fs::read_to_string(path).map_err(|e| {
        DriverError::DeviceNotFound(std::borrow::Cow::Owned(format!(
            "cannot read reference snapshot {}: {e}",
            path.display()
        )))
    })?;

    let raw: serde_json::Value = serde_json::from_str(&content).map_err(|e| {
        DriverError::DeviceNotFound(std::borrow::Cow::Owned(format!(
            "invalid JSON in {}: {e}",
            path.display()
        )))
    })?;

    let mut regs = BTreeMap::new();

    if let Some(regions) = raw.get("regions").and_then(|r| r.as_object()) {
        for (_name, region_regs) in regions {
            if let Some(obj) = region_regs.as_object() {
                for (offset_str, val_str) in obj {
                    if let Ok(offset) =
                        usize::from_str_radix(offset_str.trim_start_matches("0x"), 16)
                    {
                        if let Some(vs) = val_str.as_str() {
                            if let Ok(val) =
                                u32::from_str_radix(vs.trim_start_matches("0x"), 16)
                            {
                                regs.insert(offset, val);
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(regs)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mock_reference() -> BTreeMap<usize, u32> {
        let mut m = BTreeMap::new();
        m.insert(0x000000, 0x0F22_D0A1); // PMC_BOOT_0
        m.insert(0x000200, 0xFFFF_FFFF); // PMC_ENABLE
        m.insert(0x002200, 0x0000_0001); // PFIFO_ENABLE
        m.insert(0x400500, 0x0001_0001); // GR_MASTER_ENABLE
        m.insert(0x409100, 0x0000_0010); // FECS_CPUCTL (running)
        m
    }

    #[test]
    fn subsystem_validation_json_roundtrip() {
        let v = SubsystemValidation {
            subsystem: "PMC_ENGINE_GATING".to_string(),
            total_probes: 2,
            matched: 1,
            mismatched: 1,
            unreadable: 0,
            comparisons: vec![
                RegisterComparison {
                    offset: 0x200,
                    name: "PMC_ENABLE".to_string(),
                    actual: 0xFFFF_FFFF,
                    expected: 0xFFFF_FFFF,
                    mask: 0xFFFF_FFFF,
                    matches: true,
                },
                RegisterComparison {
                    offset: 0x000,
                    name: "PMC_BOOT_0".to_string(),
                    actual: 0x0F22_D0A1,
                    expected: 0x1234_5678,
                    mask: 0xFFFF_FFFF,
                    matches: false,
                },
            ],
        };

        let json = serde_json::to_string(&v).unwrap();
        let back: SubsystemValidation = serde_json::from_str(&json).unwrap();
        assert_eq!(back.subsystem, "PMC_ENGINE_GATING");
        assert!(!back.passed());
        assert_eq!(back.comparisons.len(), 2);
    }

    #[test]
    fn falcon_state_capture_keys() {
        let ref_map = mock_reference();
        assert_eq!(FALCON_PROBES.len(), 27);
        assert!(FALCON_PROBES.iter().any(|p| p.offset == 0x409100));
        assert!(ref_map.contains_key(&0x409100));
    }

    #[test]
    fn falcon_trigger_regs_defined() {
        assert!(FALCON_TRIGGER_REGS.len() >= 4);
        assert!(FALCON_TRIGGER_REGS
            .iter()
            .any(|(name, _)| *name == "SEC2_CPUCTL"));
        assert!(FALCON_TRIGGER_REGS
            .iter()
            .any(|(name, _)| *name == "FECS_CPUCTL"));
    }

    #[test]
    fn pramin_regions_defined() {
        assert!(FALCON_PRAMIN_REGIONS.len() >= 3);
        let sec2 = FALCON_PRAMIN_REGIONS
            .iter()
            .find(|(name, _, _)| *name == "SEC2_IMEM_BASE");
        assert!(sec2.is_some());
    }
}
