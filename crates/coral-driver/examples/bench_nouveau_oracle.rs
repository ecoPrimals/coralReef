// SPDX-License-Identifier: AGPL-3.0-only
//! Nouveau MMU oracle — dump page table state from a nouveau-bound Titan V.
//!
//! Usage:
//!   cargo run --example bench_nouveau_oracle -- <BDF>
//!   cargo run --example bench_nouveau_oracle -- 0000:03:00.0
//!
//! The target card MUST be bound to the `nouveau` kernel driver.
//! Use `coralctl swap <BDF> nouveau` to swap from vfio first.

fn main() {
    let bdf = std::env::args().nth(1).unwrap_or_else(|| {
        eprintln!("Usage: bench_nouveau_oracle <BDF>");
        eprintln!("  e.g. bench_nouveau_oracle 0000:03:00.0");
        eprintln!("\nThe card must be bound to nouveau (not vfio-pci).");
        std::process::exit(1);
    });

    eprintln!("=== Nouveau MMU Oracle ===");
    eprintln!("Target: {bdf}");

    // Verify the card is on nouveau.
    let driver_path = format!("/sys/bus/pci/devices/{bdf}/driver");
    match std::fs::read_link(&driver_path) {
        Ok(link) => {
            let driver = link
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown");
            eprintln!("Current driver: {driver}");
            if driver != "nouveau" {
                eprintln!("WARNING: card is on '{driver}', not 'nouveau'.");
                eprintln!("Page table walking requires nouveau to have initialized the MMU.");
            }
        }
        Err(e) => {
            eprintln!("Cannot read driver link ({driver_path}): {e}");
            eprintln!("Card may be unbound. Proceeding anyway...");
        }
    }

    eprintln!();

    match coral_driver::vfio::channel::nouveau_oracle::read_nouveau_page_tables(&bdf) {
        Ok(dump) => {
            coral_driver::vfio::channel::nouveau_oracle::print_comparison_report(&dump);
        }
        Err(e) => {
            eprintln!("Oracle failed: {e}");
            std::process::exit(1);
        }
    }
}
