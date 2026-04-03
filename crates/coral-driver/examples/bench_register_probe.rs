// SPDX-License-Identifier: AGPL-3.0-only
//! Quick register probe -- read key GPU register state via BAR0 mmap.
//!
//! Usage:
//!   sudo ./target/release/examples/bench_register_probe <BDF>

use coral_driver::vfio::sysfs_bar0::{DEFAULT_BAR0_SIZE, SysfsBar0};

fn main() {
    let bdf = std::env::args().nth(1).unwrap_or_else(|| {
        eprintln!("Usage: bench_register_probe <BDF>");
        std::process::exit(1);
    });

    let bar0 = SysfsBar0::open(&bdf, DEFAULT_BAR0_SIZE).unwrap_or_else(|e| {
        eprintln!("Cannot open BAR0: {e}");
        std::process::exit(1);
    });

    let r = |off: usize| -> u32 { bar0.read_u32(off) };

    println!("=== GPU Register Probe: {bdf} ===");
    println!("BOOT0           = {:#010x}", r(0x000000));
    println!("PMC_ENABLE      = {:#010x}", r(0x000200));
    println!("PMC_ENABLE_2    = {:#010x}", r(0x000204));
    println!("PFIFO_ENABLE    = {:#010x}", r(0x002200));
    println!("PFIFO_SCHED_EN  = {:#010x}", r(0x002204));
    println!("PFIFO_INTR      = {:#010x}", r(0x002100));
    println!("PFIFO_INTR_EN   = {:#010x}", r(0x002140));
    println!("BAR2_BLOCK      = {:#010x}", r(0x001714));
    println!("PRIV_RING_INTR  = {:#010x}", r(0x012070));
    println!("PMC_INTR        = {:#010x}", r(0x000100));
    println!("PBDMA_MAP       = {:#010x}", r(0x002004));
    println!("PCCSR_INST_CH0  = {:#010x}", r(0x800000));
    println!("PCCSR_CHAN_CH0  = {:#010x}", r(0x800004));

    let pmc = r(0x000200);
    println!("\n--- PMC bits ---");
    println!("  PFIFO (bit 0)  = {}", pmc & 1);
    println!("  PFIFO (bit 1)  = {}", (pmc >> 1) & 1);
    println!("  PGRAPH (bit 12)= {}", (pmc >> 12) & 1);
    println!("  CE0 (bit 6)    = {}", (pmc >> 6) & 1);

    println!("\n--- PBDMA state ---");
    for pid in 0..4u32 {
        let b = 0x40000 + pid as usize * 0x2000;
        println!(
            "  PBDMA{pid}: SIG={:#010x} GP_BASE={:#010x} GP_GET={} GP_PUT={} STATUS={:#010x} INTR={:#010x}",
            r(b + 0xC0),
            r(b + 0x40),
            r(b + 0x4C),
            r(b + 0x54),
            r(b + 0xB0),
            r(b + 0x100),
        );
    }

    println!("\n--- PFIFO topology (0x2700+) ---");
    for i in 0..16u32 {
        let val = r(0x002700 + i as usize * 4);
        if val == 0 {
            break;
        }
        println!("  TOPO[{i:2}] = {val:#010x}");
    }

    println!("\n--- GV100 runlist registers (RL 0-4) ---");
    for rl in 0..5u32 {
        println!(
            "  RL{rl}: BASE={:#010x} SUBMIT={:#010x}",
            r(0x002270 + rl as usize * 0x10),
            r(0x002274 + rl as usize * 0x10),
        );
    }

    println!("\n--- PMU / devinit status ---");
    println!("  PMU_QUEUE_HEAD[0] = {:#010x}", r(0x10a4c0));
    println!("  PMU_FALCON_CPUCTL = {:#010x}", r(0x10a100));
    println!("  PMU_FALCON_OS     = {:#010x}", r(0x10a080));
    println!("  SEC2_FALCON_OS    = {:#010x}", r(0x840080));
    println!("  PGRAPH_STATUS     = {:#010x}", r(0x400700));
}
