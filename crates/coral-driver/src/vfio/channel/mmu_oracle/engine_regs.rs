// SPDX-License-Identifier: AGPL-3.0-or-later
//! Engine and falcon register capture for cross-driver comparison.
//!
//! Reads key GPU engine registers (PFIFO, PMU, FECS, GPCCS, SEC2, MMU)
//! via BAR0 PRAMIN window. Extracted from `capture.rs` for module size
//! hygiene -- the register tables are data-heavy.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::capture::Bar0Rw;

/// Engine/falcon register state snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineRegisters {
    pub pfifo: BTreeMap<String, u32>,
    pub pmu: BTreeMap<String, u32>,
    pub fecs: BTreeMap<String, u32>,
    pub gpccs: BTreeMap<String, u32>,
    pub sec2: BTreeMap<String, u32>,
    pub mmu: BTreeMap<String, u32>,
    pub misc: BTreeMap<String, u32>,
}

/// Register offset + name pair for declarative capture.
type RegTable = &'static [(u32, &'static str)];

static PFIFO_REGS: RegTable = &[
    (0x002100, "PFIFO_INTR"),
    (0x002140, "PFIFO_INTR_EN"),
    (0x002200, "PFIFO_ENABLE"),
    (0x002204, "PFIFO_SCHED_EN"),
    (0x002208, "PFIFO_CONTROL"),
    (0x002254, "PFIFO_SCHED_STATUS"),
    (0x002270, "PFIFO_RUNLIST_BASE"),
    (0x002274, "PFIFO_RUNLIST_SUBMIT"),
    (0x002634, "PFIFO_PBDMA_MAP"),
];

static PBDMA0_REGS: RegTable = &[
    (0x040000, "PBDMA0_GP_BASE_LO"),
    (0x040004, "PBDMA0_GP_BASE_HI"),
    (0x040008, "PBDMA0_GP_FETCH"),
    (0x04000C, "PBDMA0_GP_GET"),
    (0x040010, "PBDMA0_GP_PUT"),
    (0x040014, "PBDMA0_GP_ENTRY0"),
    (0x040018, "PBDMA0_GP_ENTRY1"),
    (0x040044, "PBDMA0_STATUS"),
    (0x040048, "PBDMA0_CHANNEL"),
    (0x04004C, "PBDMA0_SIGNATURE"),
    (0x040054, "PBDMA0_USERD_LO"),
    (0x040058, "PBDMA0_USERD_HI"),
    (0x040080, "PBDMA0_TARGET"),
    (0x0400B0, "PBDMA0_INTR"),
    (0x0400C0, "PBDMA0_HCE_CTRL"),
    (0x040100, "PBDMA0_METHOD0"),
];

static PMU_REGS: RegTable = &[
    (0x10A000, "PMU_FALCON_IRQSSET"),
    (0x10A004, "PMU_FALCON_IRQSCLR"),
    (0x10A008, "PMU_FALCON_IRQSTAT"),
    (0x10A010, "PMU_FALCON_IRQMSET"),
    (0x10A014, "PMU_FALCON_IRQMCLR"),
    (0x10A040, "PMU_FALCON_MAILBOX0"),
    (0x10A044, "PMU_FALCON_MAILBOX1"),
    (0x10A080, "PMU_FALCON_OS"),
    (0x10A100, "PMU_FALCON_CPUCTL"),
    (0x10A104, "PMU_FALCON_BOOTVEC"),
    (0x10A108, "PMU_FALCON_HWCFG"),
    (0x10A10C, "PMU_FALCON_DMACTL"),
    (0x10A110, "PMU_FALCON_ENGCTL"),
    (0x10A118, "PMU_FALCON_CURCTX"),
    (0x10A11C, "PMU_FALCON_NXTCTX"),
    (0x10A4C0, "PMU_QUEUE_HEAD0"),
    (0x10A4C4, "PMU_QUEUE_HEAD1"),
    (0x10A4C8, "PMU_QUEUE_TAIL0"),
    (0x10A4CC, "PMU_QUEUE_TAIL1"),
];

static FECS_REGS: RegTable = &[
    (0x409800, "FECS_FALCON_OS"),
    (0x409840, "FECS_FALCON_MAILBOX0"),
    (0x409844, "FECS_FALCON_MAILBOX1"),
    (0x409900, "FECS_FALCON_CPUCTL"),
    (0x409904, "FECS_FALCON_BOOTVEC"),
    (0x409908, "FECS_FALCON_HWCFG"),
    (0x409918, "FECS_FALCON_CURCTX"),
    (0x40991C, "FECS_FALCON_NXTCTX"),
    (0x409A00, "FECS_FALCON_IRQSSET"),
    (0x409A04, "FECS_FALCON_IRQSCLR"),
    (0x409A08, "FECS_FALCON_IRQSTAT"),
    (0x409A10, "FECS_FALCON_IRQMSET"),
    (0x409B00, "FECS_CTX_STATE"),
    (0x409B04, "FECS_CTX_CONTROL"),
    (0x409C18, "FECS_FECS_ENGINE_STATUS"),
];

static GPCCS_REGS: RegTable = &[
    (0x502800, "GPCCS_FALCON_OS"),
    (0x502840, "GPCCS_FALCON_MAILBOX0"),
    (0x502844, "GPCCS_FALCON_MAILBOX1"),
    (0x502900, "GPCCS_FALCON_CPUCTL"),
    (0x502904, "GPCCS_FALCON_BOOTVEC"),
    (0x502908, "GPCCS_FALCON_HWCFG"),
];

static SEC2_REGS: RegTable = &[
    (0x840000, "SEC2_FALCON_IRQSSET"),
    (0x840004, "SEC2_FALCON_IRQSCLR"),
    (0x840008, "SEC2_FALCON_IRQSTAT"),
    (0x840040, "SEC2_FALCON_MAILBOX0"),
    (0x840044, "SEC2_FALCON_MAILBOX1"),
    (0x840080, "SEC2_FALCON_OS"),
    (0x840100, "SEC2_FALCON_CPUCTL"),
    (0x840104, "SEC2_FALCON_BOOTVEC"),
    (0x840108, "SEC2_FALCON_HWCFG"),
];

static MMU_REGS: RegTable = &[
    (0x100C80, "PFB_MMU_CTRL"),
    (0x100C84, "PFB_MMU_INVALIDATE_PDB"),
    (0x100CB8, "PFB_MMU_INVALIDATE"),
    (0x100E10, "PFB_PRI_MMU_FAULT_STATUS"),
    (0x100E14, "PFB_PRI_MMU_FAULT_ADDR_LO"),
    (0x100E18, "PFB_PRI_MMU_FAULT_ADDR_HI"),
    (0x100E1C, "PFB_PRI_MMU_FAULT_INFO"),
    (0x104A20, "HUBTLB_ERR"),
];

static MISC_REGS: RegTable = &[
    (0x000000, "BOOT0"),
    (0x000004, "BOOT1"),
    (0x000100, "PMC_INTR"),
    (0x000140, "PMC_INTR_EN"),
    (0x000200, "PMC_ENABLE"),
    (0x000204, "PMC_ENABLE_1"),
    (0x001700, "BAR0_WINDOW"),
    (0x120058, "PRIV_RING_INTR_STATUS"),
    (0x12004C, "PRIV_RING_COMMAND"),
];

fn read_regs(bar0: &Bar0Rw, table: RegTable) -> BTreeMap<String, u32> {
    table
        .iter()
        .map(|&(off, name)| (name.into(), bar0.read_u32(off as usize)))
        .collect()
}

/// Capture engine register state for cross-driver comparison.
pub(super) fn capture_engine_registers(bar0: &Bar0Rw) -> EngineRegisters {
    let mut pfifo = read_regs(bar0, PFIFO_REGS);
    pfifo.extend(read_regs(bar0, PBDMA0_REGS));

    EngineRegisters {
        pfifo,
        pmu: read_regs(bar0, PMU_REGS),
        fecs: read_regs(bar0, FECS_REGS),
        gpccs: read_regs(bar0, GPCCS_REGS),
        sec2: read_regs(bar0, SEC2_REGS),
        mmu: read_regs(bar0, MMU_REGS),
        misc: read_regs(bar0, MISC_REGS),
    }
}
