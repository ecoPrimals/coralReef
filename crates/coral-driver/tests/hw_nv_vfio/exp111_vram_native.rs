// SPDX-License-Identifier: AGPL-3.0-or-later

//! Exp 111: VRAM-Native Page Tables
//!
//! Addresses the HS+MMU paradox from Exp 110: legacy PDEs give HS via VRAM
//! physical fallback but break post-auth DMA; correct PDEs give working DMA
//! but route code to sysmem, failing HS auth.
//!
//! This experiment places the ENTIRE page table chain + ACR payload + WPR in
//! VRAM with correct upper-8-byte PDEs, all VRAM PTEs, and VRAM instance
//! block bind. The MMU walker should follow correct PDEs → resolve to VRAM →
//! BL reads code from VRAM → HS auth succeeds → post-auth DMA also works.
//!
//! Run:
//! ```sh
//! CORALREEF_VFIO_BDF=0000:03:00.0 cargo test -p coral-driver --features vfio \
//!   --test hw_nv_vfio exp111_vram_native -- --ignored --nocapture
//! ```

use crate::ember_client;
use crate::glowplug_client::GlowPlugClient;
use crate::helpers::init_tracing;
use coral_driver::nv::identity;
use coral_driver::nv::vfio_compute::acr_boot::{
    AcrFirmwareSet, attempt_dual_phase_boot, attempt_vram_native_acr_boot,
};

mod freg111 {
    pub const SEC2_BASE: usize = 0x087000;
    pub const SCTL: usize = 0x240;
    pub const PC: usize = 0x030;
    pub const EXCI: usize = 0x148;
    pub const MAILBOX0: usize = 0x040;
    pub const BOOT0: usize = 0x0000_0000;
}

fn discover_bdf() -> String {
    if let Ok(bdf) = std::env::var("CORALREEF_VFIO_BDF") {
        return bdf;
    }
    let driver_path = "/sys/bus/pci/drivers/vfio-pci";
    if let Ok(entries) = std::fs::read_dir(driver_path) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.contains(':') && name.contains('.') {
                let vendor_path = format!("{driver_path}/{name}/vendor");
                if let Ok(vendor) = std::fs::read_to_string(&vendor_path)
                    && vendor.trim() == "0x10de"
                {
                    eprintln!("Auto-discovered VFIO GPU: {name}");
                    return name;
                }
            }
        }
    }
    panic!("No VFIO GPU found. Set CORALREEF_VFIO_BDF or bind an NVIDIA GPU to vfio-pci.");
}

fn nouveau_cycle(bdf: &str) {
    let mut gp = GlowPlugClient::connect().expect("GlowPlug connection");
    gp.swap(bdf, "nouveau").expect("swap→nouveau");
    std::thread::sleep(std::time::Duration::from_secs(3));
    gp.swap(bdf, "vfio-pci").expect("swap→vfio-pci");
    std::thread::sleep(std::time::Duration::from_millis(500));
}

#[test]
#[ignore = "requires VFIO-bound GPU hardware + GlowPlug + Ember"]
fn exp111_vram_native() {
    init_tracing();

    let banner = "#".repeat(60);
    eprintln!("\n{banner}");
    eprintln!("#  Exp 111: VRAM-Native Page Tables                        #");
    eprintln!("{banner}\n");

    let bdf = discover_bdf();
    eprintln!("Target BDF: {bdf}");

    // Detect chip
    nouveau_cycle(&bdf);
    let fds = ember_client::request_fds(&bdf).expect("ember fds");
    let vfio_dev = coral_driver::vfio::VfioDevice::from_received(&bdf, fds).expect("VfioDevice");
    let bar0 = vfio_dev.map_bar(0).expect("map_bar(0)");
    let boot0 = bar0.read_u32(freg111::BOOT0).unwrap_or(0);
    let sm = identity::boot0_to_sm(boot0).unwrap_or(0);
    let chip = identity::chip_name(sm);
    eprintln!(
        "Chip: {} (sm={sm}, BOOT0={boot0:#010x}) → firmware: {chip}",
        identity::chipset_variant(boot0)
    );
    drop(bar0);
    drop(vfio_dev);

    let fw = AcrFirmwareSet::load(chip).expect("firmware load");

    // ── Run 1: VRAM-native with blob_size=0 (skip internal blob DMA) ──
    let sep = "=".repeat(60);
    eprintln!("\n{sep}");
    eprintln!("  Run 1: VRAM-native, skip_blob_dma=true");
    eprintln!("{sep}");

    nouveau_cycle(&bdf);
    let fds = ember_client::request_fds(&bdf).expect("ember fds run1");
    let vfio_dev =
        coral_driver::vfio::VfioDevice::from_received(&bdf, fds).expect("VfioDevice run1");
    let bar0 = vfio_dev.map_bar(0).expect("map_bar(0)");

    let result1 = attempt_vram_native_acr_boot(&bar0, &fw, true);
    eprintln!("\n  Run 1 notes:");
    for note in &result1.notes {
        eprintln!("  | {note}");
    }
    let sctl1 = bar0
        .read_u32(freg111::SEC2_BASE + freg111::SCTL)
        .unwrap_or(0);
    let exci1 = bar0
        .read_u32(freg111::SEC2_BASE + freg111::EXCI)
        .unwrap_or(0);
    let pc1 = bar0.read_u32(freg111::SEC2_BASE + freg111::PC).unwrap_or(0);
    let mb01 = bar0
        .read_u32(freg111::SEC2_BASE + freg111::MAILBOX0)
        .unwrap_or(0xDEAD);
    let hs1 = sctl1 & 0x02 != 0;
    eprintln!("  => SCTL={sctl1:#010x} HS={hs1} EXCI={exci1:#010x} PC={pc1:#06x} MB0={mb01:#010x}");
    drop(bar0);
    drop(vfio_dev);

    // ── Run 2: VRAM-native with full init (blob_size preserved) ──
    eprintln!("\n{sep}");
    eprintln!("  Run 2: VRAM-native, skip_blob_dma=false (full init)");
    eprintln!("{sep}");

    nouveau_cycle(&bdf);
    let fds = ember_client::request_fds(&bdf).expect("ember fds run2");
    let vfio_dev =
        coral_driver::vfio::VfioDevice::from_received(&bdf, fds).expect("VfioDevice run2");
    let bar0 = vfio_dev.map_bar(0).expect("map_bar(0)");

    let result2 = attempt_vram_native_acr_boot(&bar0, &fw, false);
    eprintln!("\n  Run 2 notes:");
    for note in &result2.notes {
        eprintln!("  | {note}");
    }
    let sctl2 = bar0
        .read_u32(freg111::SEC2_BASE + freg111::SCTL)
        .unwrap_or(0);
    let exci2 = bar0
        .read_u32(freg111::SEC2_BASE + freg111::EXCI)
        .unwrap_or(0);
    let pc2 = bar0.read_u32(freg111::SEC2_BASE + freg111::PC).unwrap_or(0);
    let mb02 = bar0
        .read_u32(freg111::SEC2_BASE + freg111::MAILBOX0)
        .unwrap_or(0xDEAD);
    let hs2 = sctl2 & 0x02 != 0;
    eprintln!("  => SCTL={sctl2:#010x} HS={hs2} EXCI={exci2:#010x} PC={pc2:#06x} MB0={mb02:#010x}");
    drop(bar0);
    drop(vfio_dev);

    // ── Summary ──
    let summary = "=".repeat(80);
    eprintln!("\n\n{summary}");
    eprintln!("  Exp 111 RESULTS");
    eprintln!("{summary}");
    eprintln!(
        "  Run 1 (skip blob):  HS={hs1} SCTL={sctl1:#010x} EXCI={exci1:#010x} PC={pc1:#06x} MB0={mb01:#010x}"
    );
    eprintln!(
        "  Run 2 (full init):  HS={hs2} SCTL={sctl2:#010x} EXCI={exci2:#010x} PC={pc2:#06x} MB0={mb02:#010x}"
    );
    eprintln!("{summary}");

    if hs1 || hs2 {
        eprintln!("\n  *** HS MODE ACHIEVED WITH VRAM-NATIVE PAGE TABLES! ***");
        if hs2 && mb02 == 0 {
            eprintln!("  *** FULL INIT SUCCESS — MB0 CLEARED! FECS/GPCCS MAY BE ALIVE! ***");
        }
    } else {
        eprintln!("\n  No HS with VRAM-native PTs. The HS auth mechanism may require");
        eprintln!("  physical VRAM addressing (not virtual-to-VRAM). Consider:");
        eprintln!("  - Dual-phase boot (legacy PDEs for HS, then rewrite for full init)");
        eprintln!("  - FBIF physical override during BL execution only");
    }

    eprintln!("\n=== Exp 111 Complete ===");
}

#[test]
#[ignore = "requires VFIO-bound GPU hardware + GlowPlug + Ember"]
fn exp112_dual_phase_boot() {
    init_tracing();

    let banner = "#".repeat(60);
    eprintln!("\n{banner}");
    eprintln!("#  Exp 112: Dual-Phase Boot                                #");
    eprintln!("#  Legacy PDEs → HS → hot-swap → correct virtual DMA      #");
    eprintln!("{banner}\n");

    let bdf = discover_bdf();
    eprintln!("Target BDF: {bdf}");

    // Detect chip
    nouveau_cycle(&bdf);
    let fds = ember_client::request_fds(&bdf).expect("ember fds");
    let vfio_dev = coral_driver::vfio::VfioDevice::from_received(&bdf, fds).expect("VfioDevice");
    let bar0 = vfio_dev.map_bar(0).expect("map_bar(0)");
    let boot0 = bar0.read_u32(freg111::BOOT0).unwrap_or(0);
    let sm = identity::boot0_to_sm(boot0).unwrap_or(0);
    let chip = identity::chip_name(sm);
    eprintln!(
        "Chip: {} (sm={sm}) → firmware: {chip}",
        identity::chipset_variant(boot0)
    );
    drop(bar0);
    drop(vfio_dev);

    let fw = AcrFirmwareSet::load(chip).expect("firmware load");

    // ── Run: Dual-phase boot ──
    let sep = "=".repeat(60);
    eprintln!("\n{sep}");
    eprintln!("  Dual-phase boot: legacy PDEs → HS → hot-swap PDEs");
    eprintln!("{sep}");

    nouveau_cycle(&bdf);
    let fds = ember_client::request_fds(&bdf).expect("ember fds");
    let vfio_dev = coral_driver::vfio::VfioDevice::from_received(&bdf, fds).expect("VfioDevice");
    let bar0 = vfio_dev.map_bar(0).expect("map_bar(0)");

    let result = attempt_dual_phase_boot(&bar0, &fw);
    eprintln!("\n  Notes:");
    for note in &result.notes {
        eprintln!("  | {note}");
    }

    let sctl = bar0
        .read_u32(freg111::SEC2_BASE + freg111::SCTL)
        .unwrap_or(0);
    let exci = bar0
        .read_u32(freg111::SEC2_BASE + freg111::EXCI)
        .unwrap_or(0);
    let pc = bar0.read_u32(freg111::SEC2_BASE + freg111::PC).unwrap_or(0);
    let mb0 = bar0
        .read_u32(freg111::SEC2_BASE + freg111::MAILBOX0)
        .unwrap_or(0xDEAD);
    let hs = sctl & 0x02 != 0;

    let summary = "=".repeat(80);
    eprintln!("\n{summary}");
    eprintln!("  Exp 112 RESULT");
    eprintln!("{summary}");
    eprintln!("  HS={hs} SCTL={sctl:#010x} EXCI={exci:#010x} PC={pc:#06x} MB0={mb0:#010x}");
    eprintln!("{summary}");

    if hs {
        eprintln!("\n  *** HS MODE ACHIEVED WITH DUAL-PHASE BOOT! ***");
        if mb0 == 0 {
            eprintln!("  *** MB0 CLEARED — ACR may have completed successfully! ***");
        }
    }

    eprintln!("\n=== Exp 112 Complete ===");
}
