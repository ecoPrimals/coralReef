// SPDX-License-Identifier: AGPL-3.0-only

//! Exp 113: Resolve TRAP (cause=0x20) in dual-phase boot.
//!
//! Exp 112 achieved HS via dual-phase boot but the BL traps (EXCI=0x201f0000)
//! during the WPR copy-to-target phase. This experiment tests variants to
//! identify and resolve the trap cause.
//!
//! Variants:
//! A: No hot-swap (legacy PDEs throughout) — isolates hot-swap as cause
//! B: blob_size=0 (skip blob DMA) — confirms trap is in blob processing
//! C: WPR2 boundaries pre-set — tests if BL validates WPR2 hardware
//! D: Delayed hot-swap (100ms) — tests timing sensitivity
//! E: No hot-swap + WPR2 set — combines C+A
//!
//! Run all:
//! ```sh
//! cargo test -p coral-driver --features vfio --test hw_nv_vfio \
//!   exp113 -- --ignored --nocapture --test-threads=1
//! ```

use crate::ember_client;
use crate::glowplug_client::GlowPlugClient;
use crate::helpers::init_tracing;
use coral_driver::nv::identity;
use coral_driver::nv::vfio_compute::acr_boot::{
    AcrFirmwareSet, DualPhaseConfig, attempt_dual_phase_boot_cfg,
};
use coral_driver::vfio::memory::MemoryRegion;

mod freg113 {
    pub const SEC2_BASE: usize = 0x087000;
    pub const SCTL: usize = 0x240;
    pub const PC: usize = 0x030;
    pub const EXCI: usize = 0x148;
    pub const MAILBOX0: usize = 0x040;
    #[allow(
        dead_code,
        reason = "register constant for future trap analysis experiments"
    )]
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
                    return name;
                }
            }
        }
    }
    panic!("No VFIO GPU found.");
}

fn nouveau_cycle(bdf: &str) {
    let mut gp = GlowPlugClient::connect().expect("GlowPlug connection");
    gp.swap(bdf, "nouveau").expect("swap→nouveau");
    std::thread::sleep(std::time::Duration::from_secs(3));
    gp.swap(bdf, "vfio-pci").expect("swap→vfio-pci");
    std::thread::sleep(std::time::Duration::from_millis(500));
}

struct VariantResult {
    label: String,
    hs: bool,
    sctl: u32,
    exci: u32,
    pc: u32,
    mb0: u32,
    trap: bool,
    wpr_fecs: u32,
    wpr_gpccs: u32,
}

fn run_variant(bdf: &str, fw: &AcrFirmwareSet, label: &str, cfg: DualPhaseConfig) -> VariantResult {
    let sep = "-".repeat(60);
    eprintln!("\n{sep}");
    eprintln!("  Variant: {label}");
    eprintln!("  Config: {cfg}");
    eprintln!("{sep}");

    nouveau_cycle(bdf);
    let fds = ember_client::request_fds(bdf).expect("ember fds");
    let vfio_dev = coral_driver::vfio::VfioDevice::from_received(bdf, fds).expect("VfioDevice");
    let bar0 = vfio_dev.map_bar(0).expect("map_bar(0)");

    let result = attempt_dual_phase_boot_cfg(&bar0, fw, &cfg);
    for note in &result.notes {
        eprintln!("  | {note}");
    }

    let base = freg113::SEC2_BASE;
    let sctl = bar0.read_u32(base + freg113::SCTL).unwrap_or(0);
    let exci = bar0.read_u32(base + freg113::EXCI).unwrap_or(0);
    let pc = bar0.read_u32(base + freg113::PC).unwrap_or(0);
    let mb0 = bar0.read_u32(base + freg113::MAILBOX0).unwrap_or(0xDEAD);
    let hs = sctl & 0x02 != 0;
    let trap = (exci >> 24) == 0x20;

    // WPR status from VRAM
    let wpr_fecs = coral_driver::vfio::memory::PraminRegion::new(&bar0, 0x70000, 64)
        .ok()
        .and_then(|r| r.read_u32(20).ok())
        .unwrap_or(0xFF);
    let wpr_gpccs = coral_driver::vfio::memory::PraminRegion::new(&bar0, 0x70000, 64)
        .ok()
        .and_then(|r| r.read_u32(44).ok())
        .unwrap_or(0xFF);

    eprintln!(
        "  => HS={hs} SCTL={sctl:#010x} EXCI={exci:#010x} PC={pc:#06x} MB0={mb0:#010x} TRAP={trap} WPR=[{wpr_fecs},{wpr_gpccs}]"
    );

    drop(bar0);
    drop(vfio_dev);

    VariantResult {
        label: label.to_string(),
        hs,
        sctl,
        exci,
        pc,
        mb0,
        trap,
        wpr_fecs,
        wpr_gpccs,
    }
}

#[test]
#[ignore = "requires VFIO-bound GPU hardware + GlowPlug + Ember"]
fn exp113_trap_analysis() {
    init_tracing();

    let banner = "#".repeat(60);
    eprintln!("\n{banner}");
    eprintln!("#  Exp 113: TRAP Analysis (dual-phase boot variants)        #");
    eprintln!("{banner}\n");

    let bdf = discover_bdf();
    eprintln!("Target BDF: {bdf}");

    // Detect chip
    nouveau_cycle(&bdf);
    let fds = ember_client::request_fds(&bdf).expect("ember fds");
    let vfio_dev = coral_driver::vfio::VfioDevice::from_received(&bdf, fds).expect("VfioDevice");
    let bar0 = vfio_dev.map_bar(0).expect("map_bar(0)");
    let boot0 = bar0.read_u32(0).unwrap_or(0);
    let sm = identity::boot0_to_sm(boot0).unwrap_or(0);
    let chip = identity::chip_name(sm);
    eprintln!(
        "Chip: {} (sm={sm}) → firmware: {chip}",
        identity::chipset_variant(boot0)
    );
    drop(bar0);
    drop(vfio_dev);

    let fw = AcrFirmwareSet::load(chip).expect("firmware load");

    let results = vec![
        // Variant A: No hot-swap (pure legacy PDEs) — isolate hot-swap as cause
        run_variant(
            &bdf,
            &fw,
            "A: No hot-swap (legacy PDEs throughout)",
            DualPhaseConfig {
                skip_hotswap: true,
                skip_blob_dma: false,
                ..Default::default()
            },
        ),
        // Variant B: blob_size=0 — confirm HS without trap
        run_variant(
            &bdf,
            &fw,
            "B: blob_size=0 (skip blob DMA)",
            DualPhaseConfig {
                skip_blob_dma: true,
                ..Default::default()
            },
        ),
        // Variant C: WPR2 pre-set + full init
        run_variant(
            &bdf,
            &fw,
            "C: WPR2 pre-set + full init",
            DualPhaseConfig {
                set_wpr2: true,
                ..Default::default()
            },
        ),
        // Variant D: Delayed hot-swap (100ms)
        run_variant(
            &bdf,
            &fw,
            "D: Delayed hot-swap (100ms)",
            DualPhaseConfig {
                hotswap_delay_us: 100_000,
                ..Default::default()
            },
        ),
        // Variant E: No hot-swap + WPR2 set
        run_variant(
            &bdf,
            &fw,
            "E: No hot-swap + WPR2 set",
            DualPhaseConfig {
                skip_hotswap: true,
                set_wpr2: true,
                ..Default::default()
            },
        ),
    ];

    // ── Summary table ──
    let summ = "=".repeat(100);
    eprintln!("\n\n{summ}");
    eprintln!("  Exp 113 RESULTS");
    eprintln!("{summ}");
    eprintln!(
        "  {:45} {:>4} {:>12} {:>12} {:>8} {:>12} {:>5} {:>5} {:>5}",
        "Variant", "HS", "SCTL", "EXCI", "PC", "MB0", "TRAP", "WF", "WG"
    );
    eprintln!("{}", "-".repeat(100));
    for r in &results {
        let wpr_name = |s: u32| match s {
            0 => "NONE",
            1 => "COPY",
            4 => "DONE",
            _ => "?",
        };
        eprintln!(
            "  {:45} {:>4} {:#012x} {:#012x} {:#08x} {:#012x} {:>5} {:>5} {:>5}",
            r.label,
            if r.hs { "YES" } else { "no" },
            r.sctl,
            r.exci,
            r.pc,
            r.mb0,
            if r.trap { "YES" } else { "no" },
            wpr_name(r.wpr_fecs),
            wpr_name(r.wpr_gpccs),
        );
    }
    eprintln!("{summ}");

    // Analysis
    let a_trap = results[0].trap;
    let b_trap = results[1].trap;
    let c_trap = results[2].trap;
    let b_hs = results[1].hs;

    eprintln!("\n  Analysis:");
    if a_trap {
        eprintln!("  - Variant A (no hot-swap) TRAPS → hot-swap is NOT the cause");
    } else {
        eprintln!("  - Variant A (no hot-swap) no trap → hot-swap IS the cause!");
    }

    if b_hs && !b_trap {
        eprintln!("  - Variant B (blob_size=0) HS + no trap → trap is in blob processing code");
    }

    if !c_trap {
        eprintln!("  - Variant C (WPR2 set) no trap → WPR2 boundaries WERE the cause!");
    } else {
        eprintln!("  - Variant C (WPR2 set) still traps → WPR2 is not the (sole) cause");
    }

    eprintln!("\n=== Exp 113 Complete ===");
}
