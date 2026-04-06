// SPDX-License-Identifier: AGPL-3.0-or-later
//! Quick diagnostic: step through nouveau ioctl chain to isolate EINVAL.

use coral_driver::ComputeDevice;
use coral_driver::drm::{self, DrmDevice};
use coral_driver::nv::ioctl::diag;

fn main() {
    println!("╔═══════════════════════════════════════════════╗");
    println!("║  DRM Ioctl Chain Diagnostic                  ║");
    println!("╚═══════════════════════════════════════════════╝\n");

    let nodes = drm::enumerate_render_nodes();
    for info in &nodes {
        println!(
            "  {} — driver: {}, version: {}.{}",
            info.path, info.driver, info.version_major, info.version_minor
        );
    }
    println!();

    for info in &nodes {
        if info.driver != "nouveau" {
            println!("  Skipping {} ({}) — not nouveau\n", info.path, info.driver);
            continue;
        }

        println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        println!("  Testing: {} ({})", info.path, info.driver);
        println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");

        let dev = match DrmDevice::open(&info.path) {
            Ok(d) => d,
            Err(e) => {
                println!("  FAIL: cannot open: {e}\n");
                continue;
            }
        };
        println!("  Opened {}", info.path);

        let fd = dev.fd();

        // Step 1: VM_INIT
        print!("  Step 1: VM_INIT (new UAPI)... ");
        match diag::probe_new_uapi_support(fd) {
            Ok(()) => println!("OK ✓"),
            Err(e) => println!("FAILED ({e})"),
        }

        // Step 2: Channel alloc diagnostics
        println!("  Step 2: Channel allocation diagnostics...");
        let compute_class = coral_driver::nv::uvm::VOLTA_COMPUTE_A;
        let diags = diag::diagnose_channel_alloc(fd, compute_class);
        for d in &diags {
            match &d.result {
                Ok(ch) => println!("    ✓ {} → channel {ch}", d.description),
                Err(e) => println!("    ✗ {} → {e}", d.description),
            }
        }

        // Step 3: Hex dump
        println!("\n  {}", diag::dump_channel_alloc_hex(compute_class));

        // Step 4: Full device open
        print!("  Step 4: Full NvDevice::open_path... ");
        match coral_driver::nv::NvDevice::open_path(&info.path, 70) {
            Ok(mut nv) => {
                println!("OK (SM70)");
                // Step 5: GEM alloc
                print!("  Step 5: GEM alloc (4096 bytes)... ");
                match nv.alloc(4096, coral_driver::MemoryDomain::Vram) {
                    Ok(handle) => {
                        println!("OK (handle={handle:?})");
                        let _ = nv.free(handle);
                    }
                    Err(e) => println!("FAILED ({e})"),
                }
            }
            Err(e) => println!("FAILED ({e})"),
        }
        println!();
    }
}
