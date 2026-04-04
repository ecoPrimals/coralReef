// SPDX-License-Identifier: AGPL-3.0-only
//! Exp 146: Cold GPU Fabric Probe & System Memory ACR Boot
//!
//! Discovery: Enabling memory subsystem engines without trained DRAM
//! corrupts the PRI ring. BIOS POST doesn't train DRAM on secondary GPUs.
//!
//! This experiment:
//! A. Restores PMC_ENABLE to safe cold value (recovery from prior corruption)
//! B. Probes GPU state to determine if VRAM is accessible
//! C. If cold: attempts system memory ACR boot (no VRAM needed)
//! D. If warm: delegates to VRAM-based ACR boot path

use crate::helpers::init_tracing;
use coral_driver::vfio::device::{MappedBar, VfioDevice};

mod r146 {
    pub const BOOT0: usize = 0x000000;
    pub const PMC_ENABLE: usize = 0x000200;
    pub const SEC2_BASE: usize = 0x087000;
    pub const CPUCTL: usize = 0x100;
    pub const SCTL: usize = 0x240;
    pub const PC: usize = 0x030;
    pub const EXCI: usize = 0x148;
    pub const DMACTL: usize = 0x10C;
    pub const ITFEN: usize = 0x048;

    pub const BAR0_WINDOW: usize = 0x001700;
    pub const PRAMIN_BASE: usize = 0x700000;
    pub const PRI_RING_INTR: usize = 0x120058;
}

fn reg(bar0: &MappedBar, addr: usize) -> u32 {
    bar0.read_u32(addr).unwrap_or(0xDEAD_DEAD)
}

fn wreg(bar0: &MappedBar, addr: usize, val: u32) {
    let _ = bar0.write_u32(addr, val);
}

fn is_pri_fault(val: u32) -> bool {
    val & 0xFFF0_0000 == 0xBAD0_0000
}

#[test]
#[ignore = "requires VFIO-bound GPU hardware"]
fn exp146_recover_and_probe() {
    init_tracing();
    let eq = "=".repeat(70);
    let bdf = crate::helpers::vfio_bdf();

    eprintln!("{eq}");
    eprintln!("#  Exp 146: Cold GPU Recovery & Fabric Probe");
    eprintln!("{eq}");

    let fds = crate::ember_client::request_fds(&bdf).expect("ember fds");
    let device = VfioDevice::from_received(&bdf, fds).expect("VfioDevice");
    let bar0 = device.map_bar(0).expect("map BAR0");

    // ── Phase A: Restore PMC_ENABLE ──
    eprintln!("\n  PHASE A: PMC Recovery");

    let boot0 = reg(&bar0, r146::BOOT0);
    let pmc_now = reg(&bar0, r146::PMC_ENABLE);
    eprintln!("  BOOT0={boot0:#010x} PMC_ENABLE(now)={pmc_now:#010x}");

    // SEC2 probe before restore
    let sec2_cpuctl = reg(&bar0, r146::SEC2_BASE + r146::CPUCTL);
    eprintln!(
        "  SEC2 before: cpuctl={sec2_cpuctl:#010x} (PRI fault={})",
        is_pri_fault(sec2_cpuctl)
    );

    // The safe cold PMC value: only basic engines that BIOS left enabled
    let cold_pmc = 0x4000_0121_u32;

    if pmc_now != cold_pmc && is_pri_fault(sec2_cpuctl) {
        eprintln!("  Restoring PMC_ENABLE to cold value {cold_pmc:#010x}...");
        wreg(&bar0, r146::PMC_ENABLE, cold_pmc);
        let _ = reg(&bar0, r146::PMC_ENABLE);
        std::thread::sleep(std::time::Duration::from_millis(50));

        let pmc_after = reg(&bar0, r146::PMC_ENABLE);
        eprintln!("  PMC_ENABLE after restore: {pmc_after:#010x}");
    }

    // ── Phase B: Check SEC2 recovery ──
    eprintln!("\n  PHASE B: SEC2 Recovery Check");

    let sec2_cpuctl2 = reg(&bar0, r146::SEC2_BASE + r146::CPUCTL);
    let sec2_sctl = reg(&bar0, r146::SEC2_BASE + r146::SCTL);
    let sec2_pc = reg(&bar0, r146::SEC2_BASE + r146::PC);
    let sec2_exci = reg(&bar0, r146::SEC2_BASE + r146::EXCI);
    eprintln!(
        "  SEC2: cpuctl={sec2_cpuctl2:#010x} sctl={sec2_sctl:#010x} pc={sec2_pc:#06x} exci={sec2_exci:#010x}"
    );

    let sec2_alive = !is_pri_fault(sec2_cpuctl2);
    eprintln!(
        "  SEC2 status: {}",
        if sec2_alive { "ALIVE" } else { "DEAD (PRI faults)" }
    );

    if !sec2_alive {
        // Try full SEC2 PMC bit cycle
        eprintln!("  SEC2 still dead. Attempting SEC2 PMC bit cycle...");
        let p = reg(&bar0, r146::PMC_ENABLE);
        let mask = 1u32 << 5;

        // Disable SEC2
        wreg(&bar0, r146::PMC_ENABLE, p & !mask);
        let _ = reg(&bar0, r146::PMC_ENABLE);
        std::thread::sleep(std::time::Duration::from_millis(20));

        // Re-enable SEC2
        wreg(&bar0, r146::PMC_ENABLE, p | mask);
        let _ = reg(&bar0, r146::PMC_ENABLE);
        std::thread::sleep(std::time::Duration::from_millis(50));

        let sec2_cpuctl3 = reg(&bar0, r146::SEC2_BASE + r146::CPUCTL);
        eprintln!("  SEC2 after PMC cycle: cpuctl={sec2_cpuctl3:#010x}");

        if is_pri_fault(sec2_cpuctl3) {
            eprintln!("  SEC2 cannot be recovered. PRI ring is corrupted.");
            eprintln!("  Need FLR or reboot to recover.");

            // Last resort: try SBR (secondary bus reset) by writing 0x40 to bridge control
            // Or just disable ALL extra engines to try to clear the ring
            eprintln!("  Attempting PRI ring recovery: disable all extra engines...");
            wreg(&bar0, r146::PMC_ENABLE, 0x4000_0001); // bare minimum
            let _ = reg(&bar0, r146::PMC_ENABLE);
            std::thread::sleep(std::time::Duration::from_millis(100));

            // Try re-enabling just SEC2
            let p2 = reg(&bar0, r146::PMC_ENABLE);
            wreg(&bar0, r146::PMC_ENABLE, p2 | (1 << 5) | (1 << 8));
            let _ = reg(&bar0, r146::PMC_ENABLE);
            std::thread::sleep(std::time::Duration::from_millis(50));

            let sec2_cpuctl4 = reg(&bar0, r146::SEC2_BASE + r146::CPUCTL);
            let pri_ring = reg(&bar0, r146::PRI_RING_INTR);
            eprintln!(
                "  After minimal PMC: SEC2={sec2_cpuctl4:#010x} PRI_RING={pri_ring:#010x}"
            );
        }
    }

    // ── Phase C: PRAMIN test ──
    eprintln!("\n  PHASE C: PRAMIN Test");

    wreg(&bar0, r146::BAR0_WINDOW, 0x00000001); // point at VRAM 0x10000
    let pramin_val = reg(&bar0, r146::PRAMIN_BASE);
    eprintln!(
        "  PRAMIN@0x10000: {pramin_val:#010x} (PRI fault={})",
        is_pri_fault(pramin_val)
    );

    // ── Phase D: ITFEN register test ──
    let sec2_alive_final = !is_pri_fault(reg(&bar0, r146::SEC2_BASE + r146::CPUCTL));
    if sec2_alive_final {
        eprintln!("\n  PHASE D: ITFEN Register Probe");
        let base = r146::SEC2_BASE;
        let itfen = reg(&bar0, base + r146::ITFEN);
        eprintln!("  ITFEN current: {itfen:#010x}");

        // Write test: set bits [5:4]
        wreg(&bar0, base + r146::ITFEN, (itfen & !0x30) | 0x30);
        let itfen2 = reg(&bar0, base + r146::ITFEN);
        eprintln!("  ITFEN after set [5:4]: {itfen2:#010x}");

        // Restore
        wreg(&bar0, base + r146::ITFEN, itfen);
    }

    // ── Summary ──
    let sec2_final = reg(&bar0, r146::SEC2_BASE + r146::CPUCTL);
    let pmc_final = reg(&bar0, r146::PMC_ENABLE);
    let pramin_alive = !is_pri_fault(pramin_val);

    eprintln!("\n{eq}");
    eprintln!(
        "#  SEC2={} PRAMIN={} PMC={pmc_final:#010x}",
        if !is_pri_fault(sec2_final) { "ALIVE" } else { "DEAD" },
        if pramin_alive { "ALIVE" } else { "DEAD" },
    );
    if is_pri_fault(sec2_final) {
        eprintln!("#  GPU fabric corrupted by prior engine enable. Reboot required.");
    } else if !pramin_alive {
        eprintln!("#  SEC2 alive but VRAM dead. System memory ACR boot viable.");
    }
    eprintln!("{eq}");
}
