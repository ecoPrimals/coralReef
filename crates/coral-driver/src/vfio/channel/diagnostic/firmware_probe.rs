// SPDX-License-Identifier: AGPL-3.0-only
//! Systematic firmware state capture for any NVIDIA GPU.
//!
//! Captures a [`FirmwareSnapshot`] containing the complete observable state of
//! every falcon engine (FECS, GPCCS, PMU, SEC2), GPU-wide power/scheduling
//! registers, and clock domain status. Snapshots are JSON-serializable and
//! diffable across states (pre-swap, post-swap, pre-dispatch, post-dispatch).
//!
//! This is the foundation of the firmware learning matrix: every interaction
//! with the GPU's firmware produces a snapshot, and diffs reveal which
//! registers matter for each operation.

use serde::{Deserialize, Serialize};

use crate::vfio::device::MappedBar;

use super::super::registers::{falcon, pccsr, pfifo, pmc};

/// Complete firmware state snapshot for one GPU.
///
/// Captures every observable register across all falcon engines and GPU-wide
/// subsystems. Designed to be taken at multiple points during a GPU lifecycle
/// operation and then diffed to reveal which registers changed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FirmwareSnapshot {
    /// When this snapshot was taken (epoch milliseconds).
    pub timestamp_ms: u64,
    /// Human label for the snapshot context (e.g. "pre-swap", "post-warm-fecs").
    pub label: String,
    /// GPU identity from BOOT0 (offset 0x000000).
    pub boot0: u32,
    /// GPU architecture decoded from BOOT0 (e.g. "Volta (GV140)").
    pub architecture: String,

    /// FECS (Front-End Command Scheduler) falcon at BAR0 + 0x409000.
    pub fecs: FalconSnapshot,
    /// GPCCS (GPC Command Scheduler) falcon at BAR0 + 0x41A000.
    pub gpccs: FalconSnapshot,
    /// PMU (Power Management Unit) falcon at BAR0 + 0x10A000.
    pub pmu: FalconSnapshot,
    /// SEC2 (Security Engine 2) falcon at BAR0 + 0x087000.
    pub sec2: FalconSnapshot,

    /// GPU-wide power state (PMC, PTIMER).
    pub power: PowerSnapshot,
    /// PFIFO scheduler and PBDMA state.
    pub pfifo_state: PfifoSnapshot,
    /// GR (graphics/compute) engine state.
    pub gr: GrSnapshot,
    /// Clock/PLL domain status.
    pub clocks: ClockSnapshot,
}

/// State of a single falcon microcontroller.
///
/// Each NVIDIA GPU has multiple falcon engines. This struct captures the
/// complete observable state of one engine via BAR0 register reads.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FalconSnapshot {
    /// Engine name (e.g. "FECS", "GPCCS", "PMU", "SEC2").
    pub name: String,
    /// BAR0 base address for this falcon.
    pub base: u32,

    /// CPUCTL register — encodes run/halt/reset state.
    pub cpuctl: u32,
    /// True if CPUCTL bit 5 (STOPPED) is set — CPU stopped/idle.
    pub stopped: bool,
    /// True if CPUCTL bit 4 (HALTED) is set — firmware halted.
    pub halted: bool,
    /// SCTL (Security Control) — bits 12-13 encode security mode.
    pub sctl: u32,
    /// Decoded security mode string (e.g. "NS (no security)", "HS (high security)").
    pub security_mode: String,
    /// Program counter snapshot.
    pub pc: u32,
    /// Exception info register — `[31:16]` = cause, `[15:0]` = PC at exception.
    pub exci: u32,
    /// Boot vector address (entry point on STARTCPU).
    pub bootvec: u32,
    /// Hardware config — encodes IMEM/DMEM sizes and security mode bit.
    pub hwcfg: u32,
    /// IMEM capacity in bytes (decoded from HWCFG `[8:0]` × 256).
    pub imem_size_bytes: u32,
    /// DMEM capacity in bytes (decoded from HWCFG `[17:9]` × 256).
    pub dmem_size_bytes: u32,

    /// MAILBOX0 — host↔falcon communication register.
    pub mailbox0: u32,
    /// MAILBOX1 — secondary mailbox.
    pub mailbox1: u32,

    /// IRQSTAT — pending interrupt bitmap.
    pub irqstat: u32,
    /// IRQMODE — interrupt routing configuration.
    pub irqmode: u32,

    /// FBIF_TRANSCFG — falcon bus interface DMA aperture mode.
    pub fbif_transcfg: u32,
    /// DMACTL — DMA control register.
    pub dmactl: u32,
    /// ITFEN — interface enable (bit 2 = DMA access enable).
    pub itfen: u32,

    /// MTHD_DATA register (base + 0x500) — last method parameter written.
    pub mthd_data: u32,
    /// MTHD_STATUS register (base + 0x800) — method completion status.
    pub mthd_status: u32,

    /// First 8 IMEM words read via PIO — fingerprints the loaded firmware.
    pub imem_fingerprint: Vec<u32>,

    /// Whether this falcon appears reachable (not PRI timeout / 0xDEADDEAD).
    pub reachable: bool,
}

/// GPU-wide power state from PMC and PTIMER.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PowerSnapshot {
    /// PMC_ENABLE (0x200) — bitmask of powered engine domains.
    pub pmc_enable: u32,
    /// PMC_INTR (0x100) — pending interrupt sources.
    pub pmc_intr: u32,
    /// NV_PTIMER_TIME_0 (0x9400) — lower 32 bits of GPU timer.
    pub ptimer_low: u32,
    /// NV_PTIMER_TIME_1 (0x9410) — upper 32 bits of GPU timer.
    pub ptimer_high: u32,
    /// True if PTIMER advanced between two reads (GPU clocks are running).
    pub ptimer_ticking: bool,
}

/// PFIFO scheduler and PBDMA state.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PfifoSnapshot {
    /// PFIFO_ENABLE (0x2200) — pre-Volta only; 0 on Volta+ (use PMC bit 8).
    pub enable: u32,
    /// PFIFO_INTR (0x2100) — pending PFIFO interrupts.
    pub intr: u32,
    /// PFIFO_INTR_EN (0x2140) — interrupt enable mask.
    pub intr_en: u32,
    /// SCHED_EN (0x2504) — scheduler enable status.
    pub sched_en: u32,
    /// SCHED_DISABLE (0x2630) — scheduler disable status.
    pub sched_disable: u32,
    /// PBDMA_MAP (0x2004) — bitmask of present PBDMAs.
    pub pbdma_map: u32,
    /// Number of PBDMAs present (popcount of `pbdma_map`).
    pub pbdma_count: u32,
    /// Channel 0 instance block pointer (PCCSR).
    pub channel0_inst: u32,
    /// Channel 0 state (PCCSR).
    pub channel0_state: u32,
}

/// GR (graphics/compute) engine state.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GrSnapshot {
    /// PGRAPH enable status (0x400500).
    pub gr_enable: u32,
    /// PGRAPH interrupt status (0x400100).
    pub gr_intr: u32,
    /// PGRAPH interrupt enable (0x40013C).
    pub gr_intr_en: u32,
    /// PGRAPH status (0x400700).
    pub gr_status: u32,
    /// FECS exception config register (FECS_BASE + 0xC24).
    pub fecs_exception_reg: u32,
}

/// Clock/PLL domain status — critical for K80 cold boot validation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ClockSnapshot {
    /// PCLOCK PLL 0 status (0x137000).
    pub ppll_0: u32,
    /// PCLOCK PLL 1 status (0x137020).
    pub ppll_1: u32,
    /// PBUS BAR1 block binding (0x1704).
    pub pbus_bar1_block: u32,
    /// PTIMER config register (0x9000).
    pub ptimer_cfg: u32,
}

const PRI_TIMEOUT: u32 = 0xBADF_1201;
const DEAD_DEAD: u32 = 0xDEAD_DEAD;

fn is_unreachable(val: u32) -> bool {
    val == PRI_TIMEOUT || val == DEAD_DEAD || val == 0xFFFF_FFFF
}

/// Decode the GPU architecture name from a BOOT0 register value.
#[must_use]
pub fn decode_architecture(boot0: u32) -> String {
    let chip = (boot0 >> 20) & 0x1FF;
    match chip {
        0x0E0..=0x0EF => format!("Kepler (GK{:X})", chip),
        0x100..=0x10F => format!("Maxwell (GM{:X})", chip),
        0x120..=0x13F => format!("Pascal (GP{:X})", chip),
        0x140..=0x14F => format!("Volta (GV{:X})", chip),
        0x160..=0x16F => format!("Turing (TU{:X})", chip),
        0x170..=0x17F => format!("Ampere (GA{:X})", chip),
        0x190..=0x19F => format!("Ada (AD{:X})", chip),
        0x1B0..=0x1BF => format!("Blackwell (GB{:X})", chip),
        _ => format!("Unknown (chip={chip:#05x})"),
    }
}

/// Decode the falcon security mode from an SCTL register value.
#[must_use]
pub fn decode_security(sctl: u32) -> String {
    match (sctl >> 12) & 3 {
        0 => "NS (no security)".to_string(),
        1 => "LS (light security)".to_string(),
        2 => "HS (high security)".to_string(),
        3 => "HS+ (locked)".to_string(),
        _ => "unknown".to_string(),
    }
}

fn epoch_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn capture_falcon(bar0: &MappedBar, name: &str, base: usize) -> FalconSnapshot {
    let r = |offset: usize| bar0.read_u32(base + offset).unwrap_or(DEAD_DEAD);

    let cpuctl = r(falcon::CPUCTL);
    let reachable = !is_unreachable(cpuctl);

    if !reachable {
        return FalconSnapshot {
            name: name.to_string(),
            base: base as u32,
            cpuctl,
            stopped: false,
            halted: false,
            sctl: DEAD_DEAD,
            security_mode: "unreachable".to_string(),
            pc: 0,
            exci: 0,
            bootvec: 0,
            hwcfg: 0,
            imem_size_bytes: 0,
            dmem_size_bytes: 0,
            mailbox0: 0,
            mailbox1: 0,
            irqstat: 0,
            irqmode: 0,
            fbif_transcfg: 0,
            dmactl: 0,
            itfen: 0,
            mthd_data: 0,
            mthd_status: 0,
            imem_fingerprint: Vec::new(),
            reachable: false,
        };
    }

    let sctl = r(falcon::SCTL);
    let hwcfg = r(falcon::HWCFG);

    // IMEM fingerprint: read first 8 words via PIO
    let mut imem_fp = Vec::with_capacity(8);
    let imemc_val = 1u32 << 25;
    let _ = bar0.write_u32(base + falcon::IMEMC, imemc_val);
    for _ in 0..8 {
        let word = bar0.read_u32(base + falcon::IMEMD).unwrap_or(0);
        imem_fp.push(word);
    }

    FalconSnapshot {
        name: name.to_string(),
        base: base as u32,
        cpuctl,
        stopped: cpuctl & falcon::CPUCTL_STOPPED != 0,
        halted: cpuctl & falcon::CPUCTL_HALTED != 0,
        sctl,
        security_mode: decode_security(sctl),
        pc: r(falcon::PC),
        exci: r(falcon::EXCI),
        bootvec: r(falcon::BOOTVEC),
        hwcfg,
        imem_size_bytes: falcon::imem_size_bytes(hwcfg),
        dmem_size_bytes: falcon::dmem_size_bytes(hwcfg),
        mailbox0: r(falcon::MAILBOX0),
        mailbox1: r(falcon::MAILBOX1),
        irqstat: r(falcon::IRQSTAT),
        irqmode: r(falcon::IRQMODE),
        fbif_transcfg: r(falcon::FBIF_TRANSCFG),
        dmactl: r(falcon::DMACTL),
        itfen: r(falcon::ITFEN),
        mthd_data: r(falcon::MTHD_DATA),
        mthd_status: r(falcon::MTHD_STATUS),
        imem_fingerprint: imem_fp,
        reachable: true,
    }
}

/// Capture a complete firmware snapshot from the GPU at BAR0.
///
/// This is a non-destructive read-only operation. It does not modify any
/// GPU state — only reads registers. Safe to call at any point in the
/// GPU lifecycle.
pub fn capture_firmware_snapshot(bar0: &MappedBar, label: &str) -> FirmwareSnapshot {
    let r = |reg: usize| bar0.read_u32(reg).unwrap_or(DEAD_DEAD);

    let boot0 = r(0);

    // Power
    let pmc_enable = r(pmc::ENABLE);
    let pmc_intr = r(0x100); // PMC_INTR
    let ptimer_lo = r(0x9400);
    std::thread::sleep(std::time::Duration::from_millis(1));
    let ptimer_lo2 = r(0x9400);
    let ptimer_hi = r(0x9410);

    // PFIFO
    let pfifo_en = r(pfifo::ENABLE);
    let pfifo_intr = r(pfifo::INTR);
    let pfifo_intr_en = r(pfifo::INTR_EN);
    let sched_en = r(pfifo::SCHED_EN);
    let sched_dis = r(0x2630);
    let pbdma_map = r(pfifo::PBDMA_MAP);
    let pbdma_count = pbdma_map.count_ones();

    // GR engine
    let gr_enable = r(0x400500);
    let gr_intr = r(0x400100);
    let gr_intr_en = r(0x40013C);
    let gr_status = r(0x400700);

    // FECS exception register
    let fecs_exc = r(falcon::FECS_BASE + falcon::EXCEPTION_REG);

    // Clocks
    let ppll_0 = r(0x137000);
    let ppll_1 = r(0x137020);
    let pbus_bar1 = r(0x1704);
    let ptimer_cfg = r(0x9000);

    FirmwareSnapshot {
        timestamp_ms: epoch_ms(),
        label: label.to_string(),
        boot0,
        architecture: decode_architecture(boot0),

        fecs: capture_falcon(bar0, "FECS", falcon::FECS_BASE),
        gpccs: capture_falcon(bar0, "GPCCS", falcon::GPCCS_BASE),
        pmu: capture_falcon(bar0, "PMU", falcon::PMU_BASE),
        sec2: capture_falcon(bar0, "SEC2", falcon::SEC2_BASE),

        power: PowerSnapshot {
            pmc_enable,
            pmc_intr,
            ptimer_low: ptimer_lo,
            ptimer_high: ptimer_hi,
            ptimer_ticking: ptimer_lo2 != ptimer_lo,
        },

        pfifo_state: PfifoSnapshot {
            enable: pfifo_en,
            intr: pfifo_intr,
            intr_en: pfifo_intr_en,
            sched_en,
            sched_disable: sched_dis,
            pbdma_map,
            pbdma_count,
            channel0_inst: r(pccsr::inst(0)),
            channel0_state: r(pccsr::channel(0)),
        },

        gr: GrSnapshot {
            gr_enable,
            gr_intr,
            gr_intr_en,
            gr_status,
            fecs_exception_reg: fecs_exc,
        },

        clocks: ClockSnapshot {
            ppll_0,
            ppll_1,
            pbus_bar1_block: pbus_bar1,
            ptimer_cfg,
        },
    }
}

/// Compute a human-readable diff between two snapshots.
///
/// Returns a list of `(path, old_value, new_value)` for every register
/// that changed between `before` and `after`.
pub fn diff_snapshots(
    before: &FirmwareSnapshot,
    after: &FirmwareSnapshot,
) -> Vec<(String, String, String)> {
    let mut diffs = Vec::new();

    macro_rules! cmp {
        ($path:expr, $a:expr, $b:expr) => {
            if $a != $b {
                diffs.push((
                    $path.to_string(),
                    format!("{:#010x}", $a),
                    format!("{:#010x}", $b),
                ));
            }
        };
    }

    cmp!("boot0", before.boot0, after.boot0);
    cmp!(
        "power.pmc_enable",
        before.power.pmc_enable,
        after.power.pmc_enable
    );

    for (bf, af) in [
        (&before.fecs, &after.fecs),
        (&before.gpccs, &after.gpccs),
        (&before.pmu, &after.pmu),
        (&before.sec2, &after.sec2),
    ] {
        let n = &bf.name;
        cmp!(format!("{n}.cpuctl"), bf.cpuctl, af.cpuctl);
        cmp!(format!("{n}.sctl"), bf.sctl, af.sctl);
        cmp!(format!("{n}.pc"), bf.pc, af.pc);
        cmp!(format!("{n}.exci"), bf.exci, af.exci);
        cmp!(format!("{n}.mailbox0"), bf.mailbox0, af.mailbox0);
        cmp!(format!("{n}.mailbox1"), bf.mailbox1, af.mailbox1);
        cmp!(format!("{n}.mthd_status"), bf.mthd_status, af.mthd_status);
        cmp!(format!("{n}.irqstat"), bf.irqstat, af.irqstat);
        cmp!(
            format!("{n}.fbif_transcfg"),
            bf.fbif_transcfg,
            af.fbif_transcfg
        );
    }

    cmp!(
        "pfifo.enable",
        before.pfifo_state.enable,
        after.pfifo_state.enable
    );
    cmp!(
        "pfifo.sched_en",
        before.pfifo_state.sched_en,
        after.pfifo_state.sched_en
    );
    cmp!("gr.gr_enable", before.gr.gr_enable, after.gr.gr_enable);
    cmp!("gr.gr_status", before.gr.gr_status, after.gr.gr_status);

    diffs
}

/// Print a concise firmware status summary to the tracing log.
///
/// Logs one line per falcon engine and one GPU-wide summary line.
/// Unreachable falcons are logged at WARN level.
pub fn log_firmware_summary(snap: &FirmwareSnapshot) {
    tracing::info!(
        boot0 = format_args!("{:#010x}", snap.boot0),
        arch = %snap.architecture,
        label = %snap.label,
        "firmware snapshot"
    );

    for falcon in [&snap.fecs, &snap.gpccs, &snap.pmu, &snap.sec2] {
        if falcon.reachable {
            tracing::info!(
                name = %falcon.name,
                cpuctl = format_args!("{:#010x}", falcon.cpuctl),
                stopped = falcon.stopped,
                halted = falcon.halted,
                security = %falcon.security_mode,
                pc = format_args!("{:#06x}", falcon.pc),
                mailbox0 = format_args!("{:#010x}", falcon.mailbox0),
                imem_kb = falcon.imem_size_bytes / 1024,
                dmem_kb = falcon.dmem_size_bytes / 1024,
                "  falcon"
            );
        } else {
            tracing::warn!(name = %falcon.name, "  falcon UNREACHABLE (PRI timeout)");
        }
    }

    tracing::info!(
        pmc = format_args!("{:#010x}", snap.power.pmc_enable),
        ptimer = snap.power.ptimer_ticking,
        pfifo = format_args!("{:#010x}", snap.pfifo_state.enable),
        pbdma_count = snap.pfifo_state.pbdma_count,
        gr = format_args!("{:#010x}", snap.gr.gr_enable),
        "  GPU state"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Architecture decode ─────────────────────────────────────────────

    #[test]
    fn decode_kepler_gk210() {
        // GK210 BOOT0 = 0x0f22_d0a1 → chip = (0x0f22_d0a1 >> 20) & 0x1FF = 0x0f2 → actually
        // K80 die reports chip 0x0EA (GK210B)
        let boot0 = 0x0EA0_0000u32 | 0xA1;
        let arch = decode_architecture(boot0);
        assert!(arch.starts_with("Kepler"), "got: {arch}");
    }

    #[test]
    fn decode_volta_gv100() {
        let boot0 = 0x1400_0000u32 | 0xA1;
        let arch = decode_architecture(boot0);
        assert!(arch.starts_with("Volta"), "got: {arch}");
        assert!(arch.contains("GV140"), "got: {arch}");
    }

    #[test]
    fn decode_blackwell_gb206() {
        let boot0 = 0x1B60_0000u32 | 0xA1;
        let arch = decode_architecture(boot0);
        assert!(arch.starts_with("Blackwell"), "got: {arch}");
    }

    #[test]
    fn decode_unknown_chip() {
        let arch = decode_architecture(0xFFFF_FFFF);
        assert!(arch.contains("Unknown"), "got: {arch}");
    }

    // ── Security decode ─────────────────────────────────────────────────

    #[test]
    fn decode_security_ns() {
        assert_eq!(decode_security(0x0000), "NS (no security)");
    }

    #[test]
    fn decode_security_ls() {
        assert_eq!(decode_security(0x1000), "LS (light security)");
    }

    #[test]
    fn decode_security_hs() {
        assert_eq!(decode_security(0x2000), "HS (high security)");
    }

    #[test]
    fn decode_security_hs_plus() {
        assert_eq!(decode_security(0x3000), "HS+ (locked)");
    }

    #[test]
    fn decode_security_ignores_low_bits() {
        assert_eq!(decode_security(0x20FF), "HS (high security)");
    }

    // ── Unreachable detection ───────────────────────────────────────────

    #[test]
    fn pri_timeout_is_unreachable() {
        assert!(is_unreachable(0xBADF_1201));
    }

    #[test]
    fn dead_dead_is_unreachable() {
        assert!(is_unreachable(0xDEAD_DEAD));
    }

    #[test]
    fn all_ones_is_unreachable() {
        assert!(is_unreachable(0xFFFF_FFFF));
    }

    #[test]
    fn normal_value_is_reachable() {
        assert!(!is_unreachable(0x0000_0030));
    }

    #[test]
    fn zero_is_reachable() {
        assert!(!is_unreachable(0));
    }

    // ── Snapshot diff ───────────────────────────────────────────────────

    fn make_falcon(name: &str, cpuctl: u32, sctl: u32) -> FalconSnapshot {
        FalconSnapshot {
            name: name.to_string(),
            base: 0x409000,
            cpuctl,
            stopped: cpuctl & falcon::CPUCTL_STOPPED != 0,
            halted: cpuctl & falcon::CPUCTL_HALTED != 0,
            sctl,
            security_mode: decode_security(sctl),
            pc: 0,
            exci: 0,
            bootvec: 0,
            hwcfg: 0,
            imem_size_bytes: 0,
            dmem_size_bytes: 0,
            mailbox0: 0,
            mailbox1: 0,
            irqstat: 0,
            irqmode: 0,
            fbif_transcfg: 0,
            dmactl: 0,
            itfen: 0,
            mthd_data: 0,
            mthd_status: 0,
            imem_fingerprint: Vec::new(),
            reachable: true,
        }
    }

    fn make_snapshot(label: &str, fecs_cpuctl: u32, pmc_enable: u32) -> FirmwareSnapshot {
        FirmwareSnapshot {
            timestamp_ms: 0,
            label: label.to_string(),
            boot0: 0x1400_00A1,
            architecture: "Volta (GV140)".to_string(),
            fecs: make_falcon("FECS", fecs_cpuctl, 0x2000),
            gpccs: make_falcon("GPCCS", 0x30, 0x2000),
            pmu: make_falcon("PMU", 0x00, 0x0000),
            sec2: make_falcon("SEC2", 0x00, 0x0000),
            power: PowerSnapshot {
                pmc_enable,
                pmc_intr: 0,
                ptimer_low: 0x1000,
                ptimer_high: 0,
                ptimer_ticking: true,
            },
            pfifo_state: PfifoSnapshot {
                enable: 1,
                intr: 0,
                intr_en: 0,
                sched_en: 1,
                sched_disable: 0,
                pbdma_map: 0x3,
                pbdma_count: 2,
                channel0_inst: 0,
                channel0_state: 0,
            },
            gr: GrSnapshot {
                gr_enable: 1,
                gr_intr: 0,
                gr_intr_en: 0,
                gr_status: 0,
                fecs_exception_reg: 0,
            },
            clocks: ClockSnapshot {
                ppll_0: 0x0001_0000,
                ppll_1: 0x0001_3701,
                pbus_bar1_block: 0,
                ptimer_cfg: 0,
            },
        }
    }

    #[test]
    fn diff_identical_snapshots_is_empty() {
        let a = make_snapshot("a", 0x30, 0x0000_1100);
        let b = make_snapshot("b", 0x30, 0x0000_1100);
        let diffs = diff_snapshots(&a, &b);
        assert!(diffs.is_empty(), "expected no diffs, got {diffs:?}");
    }

    #[test]
    fn diff_detects_cpuctl_change() {
        let a = make_snapshot("pre", 0x30, 0x1100);
        let b = make_snapshot("post", 0x20, 0x1100);
        let diffs = diff_snapshots(&a, &b);
        let cpuctl_diff = diffs.iter().find(|(p, _, _)| p == "FECS.cpuctl");
        assert!(cpuctl_diff.is_some(), "missing FECS.cpuctl diff");
        let (_, old, new) = cpuctl_diff.unwrap();
        assert_eq!(old, "0x00000030");
        assert_eq!(new, "0x00000020");
    }

    #[test]
    fn diff_detects_pmc_enable_change() {
        let a = make_snapshot("cold", 0x30, 0x0000_0000);
        let b = make_snapshot("warm", 0x30, 0x0000_1100);
        let diffs = diff_snapshots(&a, &b);
        assert!(
            diffs.iter().any(|(p, _, _)| p == "power.pmc_enable"),
            "missing pmc_enable diff"
        );
    }

    // ── JSON serialization roundtrip ────────────────────────────────────

    #[test]
    fn firmware_snapshot_json_roundtrip() {
        let snap = make_snapshot("test", 0x30, 0x1100);
        let json = serde_json::to_string_pretty(&snap).expect("serialize");
        let back: FirmwareSnapshot = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.label, "test");
        assert_eq!(back.fecs.cpuctl, 0x30);
        assert_eq!(back.power.pmc_enable, 0x1100);
        assert_eq!(back.architecture, "Volta (GV140)");
    }

    #[test]
    fn falcon_snapshot_json_preserves_imem_fingerprint() {
        let mut f = make_falcon("FECS", 0x30, 0x2000);
        f.imem_fingerprint = vec![0xAABB_CCDD, 0x1122_3344, 0x0, 0x0, 0x0, 0x0, 0x0, 0x0];
        let json = serde_json::to_string(&f).expect("serialize");
        let back: FalconSnapshot = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.imem_fingerprint[0], 0xAABB_CCDD);
        assert_eq!(back.imem_fingerprint[1], 0x1122_3344);
        assert_eq!(back.imem_fingerprint.len(), 8);
    }

    // ── Epoch helper ────────────────────────────────────────────────────

    #[test]
    fn epoch_ms_is_nonzero_and_recent() {
        let ms = epoch_ms();
        assert!(ms > 1_700_000_000_000, "epoch_ms too small: {ms}");
    }
}
