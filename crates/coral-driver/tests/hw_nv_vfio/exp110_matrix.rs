// SPDX-License-Identifier: AGPL-3.0-or-later

//! Exp 110: Consolidation Matrix
//!
//! Runs a 12-combination sweep of the 6 critical ACR boot variables on
//! auto-discovered VFIO hardware.  Each run cycles through nouveau for a
//! clean reset, then attempts ACR boot with the specified `BootConfig`.
//!
//! Run:
//! ```sh
//! CORALREEF_VFIO_BDF=0000:03:00.0 cargo test -p coral-driver --features vfio \
//!   --test hw_nv_vfio exp110_consolidation_matrix -- --ignored --nocapture
//! ```

use crate::ember_client;
use crate::glowplug_client::GlowPlugClient;
use crate::helpers::init_tracing;
use coral_driver::nv::identity;
use coral_driver::nv::vfio_compute::acr_boot::{
    AcrFirmwareSet, BootConfig, attempt_sysmem_acr_boot_with_config,
};

mod freg110 {
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

fn detect_chip(bar0: &coral_driver::vfio::device::MappedBar) -> (&'static str, u32) {
    let boot0 = bar0.read_u32(freg110::BOOT0).unwrap_or(0);
    let sm = identity::boot0_to_sm(boot0).unwrap_or(0);
    let chip = identity::chip_name(sm);
    let variant = identity::chipset_variant(boot0);
    eprintln!("Chip: {variant} (sm={sm}, BOOT0={boot0:#010x}) → firmware: {chip}");
    (chip, boot0)
}

fn build_matrix() -> Vec<(u32, BootConfig, &'static str)> {
    vec![
        (
            1,
            BootConfig {
                pde_upper: false,
                acr_vram_pte: false,
                blob_size_zero: true,
                bind_vram: false,
                imem_preload: false,
                tlb_invalidate: false,
            },
            "Exp 095 baseline",
        ),
        (
            2,
            BootConfig {
                pde_upper: false,
                acr_vram_pte: false,
                blob_size_zero: true,
                bind_vram: false,
                imem_preload: false,
                tlb_invalidate: true,
            },
            "+ TLB",
        ),
        (
            3,
            BootConfig {
                pde_upper: true,
                acr_vram_pte: false,
                blob_size_zero: true,
                bind_vram: false,
                imem_preload: false,
                tlb_invalidate: true,
            },
            "Correct PDEs, skip blob",
        ),
        (
            4,
            BootConfig {
                pde_upper: true,
                acr_vram_pte: true,
                blob_size_zero: true,
                bind_vram: false,
                imem_preload: false,
                tlb_invalidate: true,
            },
            "+ VRAM code PTEs",
        ),
        (
            5,
            BootConfig {
                pde_upper: false,
                acr_vram_pte: false,
                blob_size_zero: false,
                bind_vram: false,
                imem_preload: false,
                tlb_invalidate: false,
            },
            "Old PDEs, full init",
        ),
        (
            6,
            BootConfig {
                pde_upper: false,
                acr_vram_pte: false,
                blob_size_zero: false,
                bind_vram: false,
                imem_preload: false,
                tlb_invalidate: true,
            },
            "+ TLB",
        ),
        (
            7,
            BootConfig {
                pde_upper: true,
                acr_vram_pte: false,
                blob_size_zero: false,
                bind_vram: false,
                imem_preload: false,
                tlb_invalidate: true,
            },
            "Correct PDEs, full init",
        ),
        (
            8,
            BootConfig {
                pde_upper: true,
                acr_vram_pte: true,
                blob_size_zero: false,
                bind_vram: false,
                imem_preload: false,
                tlb_invalidate: true,
            },
            "+ VRAM code PTEs, full init",
        ),
        (
            9,
            BootConfig {
                pde_upper: true,
                acr_vram_pte: true,
                blob_size_zero: false,
                bind_vram: true,
                imem_preload: false,
                tlb_invalidate: true,
            },
            "All-VRAM path",
        ),
        (
            10,
            BootConfig {
                pde_upper: true,
                acr_vram_pte: false,
                blob_size_zero: true,
                bind_vram: false,
                imem_preload: true,
                tlb_invalidate: true,
            },
            "Pre-load interference",
        ),
        (
            11,
            BootConfig {
                pde_upper: false,
                acr_vram_pte: false,
                blob_size_zero: true,
                bind_vram: false,
                imem_preload: true,
                tlb_invalidate: false,
            },
            "Pre-load + old PDEs",
        ),
        (
            12,
            BootConfig {
                pde_upper: true,
                acr_vram_pte: true,
                blob_size_zero: true,
                bind_vram: false,
                imem_preload: false,
                tlb_invalidate: true,
            },
            "Correct PDEs + VRAM code + skip blob",
        ),
    ]
}

#[allow(
    dead_code,
    reason = "fields read via Debug formatting in experiment output"
)]
struct RunResult {
    combo: u32,
    label: String,
    sctl: u32,
    hs: bool,
    exci: u32,
    tracepc_count: u32,
    crash_pc: u32,
    dmem_readable: bool,
    mailbox0_cleared: bool,
    error: Option<String>,
}

impl RunResult {
    fn hs_str(&self) -> &str {
        if self.error.is_some() {
            "ERR"
        } else if self.hs {
            "YES"
        } else {
            "no"
        }
    }
}

fn nouveau_cycle(bdf: &str) {
    let mut gp = GlowPlugClient::connect().expect("GlowPlug connection");
    gp.swap(bdf, "nouveau").expect("swap→nouveau");
    std::thread::sleep(std::time::Duration::from_secs(3));
    gp.swap(bdf, "vfio-pci").expect("swap→vfio-pci");
    std::thread::sleep(std::time::Duration::from_millis(500));
}

fn run_single(
    bdf: &str,
    combo: u32,
    config: &BootConfig,
    why: &str,
    fw: &AcrFirmwareSet,
) -> RunResult {
    let sep = "=".repeat(60);
    eprintln!("\n{sep}");
    eprintln!("  Combo #{combo}: {why}");
    eprintln!("  Config: {config}");
    eprintln!("{sep}");

    nouveau_cycle(bdf);

    let fds = match ember_client::request_fds(bdf) {
        Ok(f) => f,
        Err(e) => {
            return RunResult {
                combo,
                label: why.to_string(),
                sctl: 0,
                hs: false,
                exci: 0,
                tracepc_count: 0,
                crash_pc: 0,
                dmem_readable: false,
                mailbox0_cleared: false,
                error: Some(format!("ember fds: {e}")),
            };
        }
    };

    let vfio_dev = match coral_driver::vfio::VfioDevice::from_received(bdf, fds) {
        Ok(d) => d,
        Err(e) => {
            return RunResult {
                combo,
                label: why.to_string(),
                sctl: 0,
                hs: false,
                exci: 0,
                tracepc_count: 0,
                crash_pc: 0,
                dmem_readable: false,
                mailbox0_cleared: false,
                error: Some(format!("VfioDevice: {e}")),
            };
        }
    };
    let bar0 = vfio_dev.map_bar(0).expect("map_bar(0)");
    let container = vfio_dev.dma_backend();

    let result = attempt_sysmem_acr_boot_with_config(&bar0, fw, container, config);

    for note in &result.notes {
        eprintln!("  | {note}");
    }

    let base = freg110::SEC2_BASE;
    let sctl = bar0.read_u32(base + freg110::SCTL).unwrap_or(0);
    let exci = bar0.read_u32(base + freg110::EXCI).unwrap_or(0);
    let pc = bar0.read_u32(base + freg110::PC).unwrap_or(0);
    let mb0 = bar0.read_u32(base + freg110::MAILBOX0).unwrap_or(0xDEAD);
    let hs = sctl & 0x02 != 0;
    let tracepc_count = (exci >> 16) & 0xFF;

    let dmem_readable = {
        let _ = bar0.write_u32(base + 0x1C0, 0x200 | (1u32 << 25));
        let v = bar0.read_u32(base + 0x1C4).unwrap_or(0xDEAD);
        v != 0xDEAD && v != 0xDEAD_DEAD
    };

    eprintln!("  => SCTL={sctl:#010x} HS={hs} EXCI={exci:#010x} PC={pc:#06x} MB0={mb0:#010x}");

    RunResult {
        combo,
        label: why.to_string(),
        sctl,
        hs,
        exci,
        tracepc_count,
        crash_pc: pc,
        dmem_readable,
        mailbox0_cleared: mb0 == 0,
        error: None,
    }
}

#[test]
#[ignore = "requires VFIO-bound GPU hardware + GlowPlug + Ember"]
fn exp110_consolidation_matrix() {
    init_tracing();

    let banner = "#".repeat(60);
    eprintln!("\n{banner}");
    eprintln!("#  Exp 110: Consolidation Matrix                            #");
    eprintln!("{banner}\n");

    let bdf = discover_bdf();
    eprintln!("Target BDF: {bdf}");

    // Initial cycle to get BAR0 for chip detection
    nouveau_cycle(&bdf);
    let fds = ember_client::request_fds(&bdf).expect("ember fds for chip detect");
    let vfio_dev = coral_driver::vfio::VfioDevice::from_received(&bdf, fds).expect("VfioDevice");
    let bar0 = vfio_dev.map_bar(0).expect("map_bar(0)");
    let (chip, _boot0) = detect_chip(&bar0);
    drop(bar0);
    drop(vfio_dev);

    let fw = AcrFirmwareSet::load(chip).expect("firmware load");
    let matrix = build_matrix();

    eprintln!("\nRunning {} combinations...\n", matrix.len());

    let mut results: Vec<RunResult> = Vec::new();
    for (combo, config, why) in &matrix {
        let r = run_single(&bdf, *combo, config, why, &fw);
        results.push(r);
    }

    // Summary table
    let sep120 = "=".repeat(120);
    eprintln!("\n\n{sep120}");
    eprintln!("  CONSOLIDATION MATRIX RESULTS");
    eprintln!("{sep120}");
    eprintln!(
        "{:>3} | {:>5} {:>8} {:>5} {:>5} {:>4} {:>3} | {:>3} | {:>10} | {:>4} | {:>3} | {:>5} | {:>4} | Why",
        "#",
        "pde",
        "vram_pte",
        "blob0",
        "bind",
        "imem",
        "tlb",
        "HS",
        "SCTL",
        "EXCI",
        "TPC",
        "PC",
        "MB0=0"
    );
    eprintln!("{}", "-".repeat(120));

    for (i, r) in results.iter().enumerate() {
        let (_, ref cfg, _) = matrix[i];
        eprintln!(
            "{:>3} | {:>5} {:>8} {:>5} {:>5} {:>4} {:>3} | {:>3} | {:#010x} | {:>4} | {:>3} | {:#06x} | {:>5} | {}",
            r.combo,
            if cfg.pde_upper { "upper" } else { "lower" },
            cfg.acr_vram_pte,
            cfg.blob_size_zero,
            if cfg.bind_vram { "VRAM" } else { "SYS" },
            cfg.imem_preload,
            cfg.tlb_invalidate,
            r.hs_str(),
            r.sctl,
            r.exci,
            r.tracepc_count,
            r.crash_pc,
            r.mailbox0_cleared,
            r.label,
        );
    }
    eprintln!("{}", "-".repeat(120));

    let hs_count = results.iter().filter(|r| r.hs).count();
    let mb0_count = results.iter().filter(|r| r.mailbox0_cleared).count();
    eprintln!(
        "\nHS achieved: {hs_count}/{} | MB0 cleared: {mb0_count}/{}",
        results.len(),
        results.len()
    );

    eprintln!("\n=== Exp 110 Complete ===");
}
