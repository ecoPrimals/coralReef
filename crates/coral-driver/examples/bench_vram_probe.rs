// SPDX-License-Identifier: AGPL-3.0-or-later
//! VRAM accessibility probe -- test if PRAMIN window can read/write VRAM
//! on a cold VFIO card (no HBM2 training).
//!
//! Usage:
//!   Direct:  sudo ./target/release/examples/bench_vram_probe <BDF>
//!   Ember:   ./target/release/examples/bench_vram_probe --ember <BDF>

use coral_driver::nv::vfio_compute::RawVfioDevice;
use coral_driver::vfio::device::MappedBar;
use coral_driver::vfio::ember_client::EmberSession;

const BAR0_WINDOW: usize = 0x0000_1700;
const PRAMIN_BASE: usize = 0x0070_0000;

enum DeviceHandle {
    Direct(RawVfioDevice),
    Ember(EmberSession),
}

impl DeviceHandle {
    fn bar0(&self) -> &MappedBar {
        match self {
            Self::Direct(raw) => &raw.bar0,
            Self::Ember(session) => &session.bar0,
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let use_ember = args.iter().any(|a| a == "--ember");
    let bdf = args
        .iter()
        .skip(1)
        .find(|a| !a.starts_with('-'))
        .cloned()
        .unwrap_or_else(|| {
            eprintln!("Usage: bench_vram_probe [--ember] <BDF>");
            std::process::exit(1);
        });

    println!(
        "=== VRAM Probe: {bdf} (mode: {}) ===",
        if use_ember { "ember" } else { "direct" }
    );

    let handle = if use_ember {
        DeviceHandle::Ember(EmberSession::connect(&bdf).unwrap_or_else(|e| {
            eprintln!("Cannot connect to ember: {e}");
            std::process::exit(1);
        }))
    } else {
        DeviceHandle::Direct(RawVfioDevice::open(&bdf).unwrap_or_else(|e| {
            eprintln!("Cannot open: {e}");
            std::process::exit(1);
        }))
    };

    let bar0 = handle.bar0();

    let boot0 = bar0.read_u32(0).unwrap_or(0xDEAD);
    println!("BOOT0 = {boot0:#010x}");

    let _ = bar0.write_u32(0x200, 0xFFFF_FFFF);
    std::thread::sleep(std::time::Duration::from_millis(50));
    println!("PMC_ENABLE = {:#010x}", bar0.read_u32(0x200).unwrap_or(0));

    let saved_window = bar0.read_u32(BAR0_WINDOW).unwrap_or(0);
    println!("Current BAR0_WINDOW = {saved_window:#010x}");

    println!("\n--- VRAM probe via PRAMIN (window=0) ---");
    let _ = bar0.write_u32(BAR0_WINDOW, 0);
    std::thread::sleep(std::time::Duration::from_millis(5));

    for i in 0..8 {
        let val = bar0.read_u32(PRAMIN_BASE + i * 4).unwrap_or(0xDEAD);
        println!("  VRAM[{:#06x}] = {val:#010x}", i * 4);
    }

    println!("\n--- VRAM write-readback test ---");
    let test_off = PRAMIN_BASE + 0x100;
    let pre = bar0.read_u32(test_off).unwrap_or(0xDEAD);
    println!("  Before: VRAM[0x100] = {pre:#010x}");

    let _ = bar0.write_u32(test_off, 0xDEAD_BEEF);
    std::thread::sleep(std::time::Duration::from_millis(1));
    let post = bar0.read_u32(test_off).unwrap_or(0xDEAD);
    println!("  After 0xDEADBEEF: VRAM[0x100] = {post:#010x}");

    let vram_works = post == 0xDEAD_BEEF;
    if vram_works {
        println!("  VRAM write-readback: SUCCESS");
        let _ = bar0.write_u32(test_off, pre);
    } else {
        println!("  VRAM write-readback: FAILED (HBM2 not trained?)");
    }

    println!("\n--- BAR2 DMA loopback test ---");
    let bar2_block = bar0.read_u32(0x1714).unwrap_or(0xDEAD);
    println!("  BAR2_BLOCK = {bar2_block:#010x}");

    let _ = bar0.write_u32(BAR0_WINDOW, saved_window);

    println!("\n=== VRAM accessible: {vram_works} ===");

    match handle {
        DeviceHandle::Direct(raw) => raw.leak(),
        DeviceHandle::Ember(_) => {}
    }
}
