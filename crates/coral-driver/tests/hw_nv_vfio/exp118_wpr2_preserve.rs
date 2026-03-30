// SPDX-License-Identifier: AGPL-3.0-only

//! Exp 118: WPR2 Preservation via No-Reset Swap + SBR Trigger
//!
//! Exp 117 showed: WPR2 IS VALID during nouveau (0x2FFE00000..0x2FFE40000),
//! falcons running with SCTL=0x7021 (FWSEC-authenticated). The swap KILLS
//! everything. But Exp 103 showed we can disable PCI reset before the swap.
//!
//! Key insight: Exp 103 checked `sctl & 0x02` for HS mode — this is FALSE
//! for SCTL=0x7021 (FWSEC mode). So Exp 103 missed surviving falcons.
//!
//! Phases:
//!   A: No-reset swap — disable PCI reset, swap to vfio, check WPR2/falcons
//!   B: If falcons survived, send BOOTSTRAP_FALCON directly
//!   C: If WPR2 died, try SBR to trigger FWSEC → re-establish WPR2
//!   D: Capture nouveau's WPR2 content (256 KiB) for analysis
//!
//! Run:
//! ```sh
//! cargo test -p coral-driver --features vfio --test hw_nv_vfio \
//!   exp118 -- --ignored --nocapture --test-threads=1
//! ```

use crate::ember_client;
use crate::glowplug_client::GlowPlugClient;
use crate::helpers::init_tracing;
use coral_driver::gsp::RegisterAccess;
use coral_driver::nv::bar0::Bar0Access;
use coral_driver::nv::vfio_compute::acr_boot::{
    AcrFirmwareSet, BootConfig, FalconBootvecOffsets, attempt_acr_mailbox_command,
    attempt_sysmem_acr_boot_with_config,
};
use coral_driver::vfio::device::MappedBar;
use coral_driver::vfio::memory::{MemoryRegion, PraminRegion};

mod r118 {
    pub const SEC2_BASE: u32 = 0x087000;
    pub const FECS_BASE: u32 = 0x409000;
    pub const GPCCS_BASE: u32 = 0x41a000;

    pub const CPUCTL: u32 = 0x100;
    pub const SCTL: u32 = 0x240;
    pub const PC: u32 = 0x030;
    pub const EXCI: u32 = 0x148;
    pub const MAILBOX0: u32 = 0x040;
    pub const MAILBOX1: u32 = 0x044;

    pub const CPUCTL_HALTED: u32 = 1 << 4;
    pub const CPUCTL_STOPPED: u32 = 1 << 5;
    pub const INDEXED_WPR: u32 = 0x100CD4;
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
                if let Ok(vendor) = std::fs::read_to_string(&vendor_path) {
                    if vendor.trim() == "0x10de" {
                        return name;
                    }
                }
            }
        }
    }
    panic!("No VFIO GPU found.");
}

fn read_wpr2(bar0: &MappedBar) -> (u64, u64, bool) {
    let _ = bar0.write_u32(r118::INDEXED_WPR as usize, 2);
    let raw_s = bar0.read_u32(r118::INDEXED_WPR as usize).unwrap_or(0);
    let _ = bar0.write_u32(r118::INDEXED_WPR as usize, 3);
    let raw_e = bar0.read_u32(r118::INDEXED_WPR as usize).unwrap_or(0);
    let start = ((raw_s as u64) & 0xFFFF_FF00) << 8;
    let end = ((raw_e as u64) & 0xFFFF_FF00) << 8;
    let valid = start > 0 && end > start && (end - start) > 0x1000;
    (start, end, valid)
}

fn falcon_state(bar0: &MappedBar, name: &str, base: usize) -> (u32, u32, u32) {
    let cpuctl = bar0
        .read_u32(base + r118::CPUCTL as usize)
        .unwrap_or(0xDEAD);
    let sctl = bar0.read_u32(base + r118::SCTL as usize).unwrap_or(0xDEAD);
    let pc = bar0.read_u32(base + r118::PC as usize).unwrap_or(0xDEAD);
    let exci = bar0.read_u32(base + r118::EXCI as usize).unwrap_or(0xDEAD);
    let mb0 = bar0
        .read_u32(base + r118::MAILBOX0 as usize)
        .unwrap_or(0xDEAD);
    let halted = cpuctl & r118::CPUCTL_HALTED != 0;
    let stopped = cpuctl & r118::CPUCTL_STOPPED != 0;
    let alive = !halted && !stopped && cpuctl != 0xDEAD;
    eprintln!(
        "  {name:6}: cpuctl={cpuctl:#010x} SCTL={sctl:#06x} PC={pc:#06x} EXCI={exci:#010x} \
         MB0={mb0:#010x} alive={alive}"
    );
    (cpuctl, sctl, pc)
}

fn disable_pci_reset(bdf: &str) -> Result<(), String> {
    let path = format!("/sys/bus/pci/devices/{bdf}/reset_method");
    std::fs::write(&path, "").map_err(|e| format!("write {path}: {e}"))
}

fn restore_pci_reset(bdf: &str) {
    let path = format!("/sys/bus/pci/devices/{bdf}/reset_method");
    let _ = std::fs::write(&path, "flr\nbus\n");
}

fn dump_wpr_headers(bar0: &MappedBar, vram_base: u64, label: &str) {
    eprintln!("  WPR headers at {vram_base:#x} ({label}):");
    match PraminRegion::new(bar0, vram_base as u32, 264) {
        Ok(rgn) => {
            for i in 0..11 {
                let off = i * 24;
                let falcon_id = rgn.read_u32(off).unwrap_or(0xDEAD);
                if falcon_id == 0xFFFF_FFFF || falcon_id == 0xDEAD_DEAD {
                    break;
                }
                let status = rgn.read_u32(off + 20).unwrap_or(0);
                let sname = match status {
                    0 => "NONE",
                    1 => "COPY",
                    4 => "VALID_DONE",
                    6 => "BOOT_READY",
                    _ => "?",
                };
                let fname = match falcon_id {
                    0 => "PMU",
                    2 => "FECS",
                    3 => "GPCCS",
                    7 => "SEC2",
                    _ => "???",
                };
                eprintln!("    [{i}] falcon={falcon_id}({fname}) status={status}({sname})");
            }
        }
        Err(e) => eprintln!("    PRAMIN failed: {e}"),
    }
}

#[test]
#[ignore = "requires VFIO-bound GPU hardware + GlowPlug + Ember"]
fn exp118_wpr2_preserve() {
    init_tracing();

    let banner = "#".repeat(70);
    eprintln!("\n{banner}");
    eprintln!("#  Exp 118: WPR2 Preservation — No-Reset Swap + SBR Trigger        #");
    eprintln!("#  If we prevent the reset, WPR2 and falcons should survive.        #");
    eprintln!("{banner}\n");

    let bdf = discover_bdf();
    eprintln!("Target BDF: {bdf}");
    let eq = "=".repeat(70);

    // ══════════════════════════════════════════════════════════════════════
    // PHASE A: No-reset swap — preserve nouveau's GPU state
    // ══════════════════════════════════════════════════════════════════════
    eprintln!("\n{eq}");
    eprintln!("  PHASE A: No-reset swap (disable PCI reset before vfio bind)");
    eprintln!("{eq}");

    let mut gp = GlowPlugClient::connect().expect("GlowPlug connection");

    eprintln!("\n── A1: Swap to nouveau ──");
    gp.swap(&bdf, "nouveau").expect("swap→nouveau");
    std::thread::sleep(std::time::Duration::from_secs(4));

    eprintln!("\n── A2: Read WPR2 via sysfs while nouveau active ──");
    let sysfs_dev = format!("/sys/bus/pci/devices/{bdf}");
    let mut bar0_sysfs =
        Bar0Access::from_sysfs_device(&sysfs_dev).expect("BAR0 sysfs while nouveau is bound");

    // Indexed WPR2 read
    let _ = bar0_sysfs.write_u32(r118::INDEXED_WPR, 2);
    let wpr2_s_raw = bar0_sysfs.read_u32(r118::INDEXED_WPR).unwrap_or(0);
    let _ = bar0_sysfs.write_u32(r118::INDEXED_WPR, 3);
    let wpr2_e_raw = bar0_sysfs.read_u32(r118::INDEXED_WPR).unwrap_or(0);
    let wpr2_start = ((wpr2_s_raw as u64) & 0xFFFF_FF00) << 8;
    let wpr2_end = ((wpr2_e_raw as u64) & 0xFFFF_FF00) << 8;
    let wpr2_valid = wpr2_start > 0 && wpr2_end > wpr2_start;
    eprintln!("  WPR2 (nouveau): start={wpr2_start:#x} end={wpr2_end:#x} valid={wpr2_valid}");

    // Falcon state during nouveau
    let sec2_sctl_nouveau = bar0_sysfs
        .read_u32(r118::SEC2_BASE + r118::SCTL)
        .unwrap_or(0);
    let fecs_sctl_nouveau = bar0_sysfs
        .read_u32(r118::FECS_BASE + r118::SCTL)
        .unwrap_or(0);
    let gpccs_sctl_nouveau = bar0_sysfs
        .read_u32(r118::GPCCS_BASE + r118::SCTL)
        .unwrap_or(0);
    eprintln!("  SEC2  SCTL={sec2_sctl_nouveau:#06x} (during nouveau)");
    eprintln!("  FECS  SCTL={fecs_sctl_nouveau:#06x} (during nouveau)");
    eprintln!("  GPCCS SCTL={gpccs_sctl_nouveau:#06x} (during nouveau)");

    drop(bar0_sysfs);

    eprintln!("\n── A3: Disable PCI reset, swap to vfio-pci ──");
    match disable_pci_reset(&bdf) {
        Ok(()) => eprintln!("  PCI reset disabled for {bdf}"),
        Err(e) => eprintln!("  WARNING: Could not disable reset: {e}"),
    }

    gp.swap(&bdf, "vfio-pci").expect("swap→vfio-pci (no reset)");
    std::thread::sleep(std::time::Duration::from_millis(500));
    restore_pci_reset(&bdf);
    eprintln!("  PCI reset method restored");

    let fds = ember_client::request_fds(&bdf).expect("ember fds");
    let vfio_dev = coral_driver::vfio::VfioDevice::from_received(&bdf, fds).expect("VfioDevice");
    let bar0 = vfio_dev.map_bar(0).expect("map_bar(0)");

    eprintln!("\n── A4: Post-swap state (NO RESET) ──");
    let (sec2_cpu_a, sec2_sctl_a, _sec2_pc_a) =
        falcon_state(&bar0, "SEC2", r118::SEC2_BASE as usize);
    let (fecs_cpu_a, _fecs_sctl_a, _) = falcon_state(&bar0, "FECS", r118::FECS_BASE as usize);
    let (gpccs_cpu_a, _gpccs_sctl_a, _) = falcon_state(&bar0, "GPCCS", r118::GPCCS_BASE as usize);

    let (w2s_a, w2e_a, w2v_a) = read_wpr2(&bar0);
    eprintln!("  WPR2 (post-swap): start={w2s_a:#x} end={w2e_a:#x} valid={w2v_a}");

    let sec2_alive = sec2_cpu_a & r118::CPUCTL_HALTED == 0
        && sec2_cpu_a & r118::CPUCTL_STOPPED == 0
        && sec2_cpu_a != 0xDEAD;
    let fecs_alive = fecs_cpu_a & r118::CPUCTL_HALTED == 0
        && fecs_cpu_a & r118::CPUCTL_STOPPED == 0
        && fecs_cpu_a != 0xDEAD;
    let gpccs_alive = gpccs_cpu_a & r118::CPUCTL_HALTED == 0
        && gpccs_cpu_a & r118::CPUCTL_STOPPED == 0
        && gpccs_cpu_a != 0xDEAD;

    eprintln!("\n  ┌─────────────────────────────────────────────┐");
    eprintln!("  │ PHASE A VERDICT:                            │");
    eprintln!("  │ WPR2 survived:      {:<25}│", w2v_a);
    eprintln!("  │ SEC2 survived:      {:<25}│", sec2_alive);
    eprintln!("  │ FECS survived:      {:<25}│", fecs_alive);
    eprintln!("  │ GPCCS survived:     {:<25}│", gpccs_alive);
    eprintln!("  └─────────────────────────────────────────────┘");

    // ══════════════════════════════════════════════════════════════════════
    // PHASE B: If falcons survived, try direct BOOTSTRAP_FALCON
    // ══════════════════════════════════════════════════════════════════════
    if sec2_alive && w2v_a {
        eprintln!("\n{eq}");
        eprintln!("  PHASE B: Falcons survived — direct BOOTSTRAP_FALCON");
        eprintln!("{eq}");

        if fecs_alive && gpccs_alive {
            eprintln!("  *** FECS AND GPCCS ALREADY RUNNING — NO BOOTSTRAP NEEDED ***");
            eprintln!("  *** THIS IS THE BREAKTHROUGH — COMPUTE PIPELINE READY ***");
        } else {
            eprintln!("  SEC2 alive, sending BOOTSTRAP_FALCON...");
            let fw = AcrFirmwareSet::load("gv100").expect("firmware load");
            let bootvec = FalconBootvecOffsets {
                gpccs: fw.gpccs_bl.bl_imem_off(),
                fecs: fw.fecs_bl.bl_imem_off(),
            };

            let mb_result = attempt_acr_mailbox_command(&bar0, &bootvec);
            eprintln!("  Strategy: {}", mb_result.strategy);
            eprintln!("  Success: {}", mb_result.success);
            for note in &mb_result.notes {
                eprintln!("  | {note}");
            }

            eprintln!("\n── B2: Post-BOOTSTRAP state ──");
            falcon_state(&bar0, "SEC2", r118::SEC2_BASE as usize);
            falcon_state(&bar0, "FECS", r118::FECS_BASE as usize);
            falcon_state(&bar0, "GPCCS", r118::GPCCS_BASE as usize);
        }

        // Check WPR status at nouveau's WPR2 address
        if wpr2_valid {
            dump_wpr_headers(&bar0, wpr2_start, "nouveau WPR2 location");
        }
    } else if sec2_alive {
        eprintln!("\n{eq}");
        eprintln!("  PHASE B: SEC2 alive but WPR2 invalid — try ACR boot with known WPR2 addr");
        eprintln!("{eq}");

        let fw = AcrFirmwareSet::load("gv100").expect("firmware load");
        let container = vfio_dev.dma_backend();

        let cfg = BootConfig {
            pde_upper: true,
            acr_vram_pte: false,
            blob_size_zero: true,
            bind_vram: false,
            imem_preload: false,
            tlb_invalidate: true,
        };

        let result = attempt_sysmem_acr_boot_with_config(&bar0, &fw, container, &cfg);
        eprintln!("  Strategy: {}", result.strategy);
        eprintln!("  Success: {}", result.success);
        for note in &result.notes {
            eprintln!("  | {note}");
        }
    } else {
        eprintln!("\n  PHASE B: SKIPPED — SEC2 did not survive the swap");
    }

    // ══════════════════════════════════════════════════════════════════════
    // PHASE C: GlowPlug-mediated resets to trigger FWSEC re-execution
    // ══════════════════════════════════════════════════════════════════════
    if !w2v_a {
        eprintln!("\n{eq}");
        eprintln!("  PHASE C: GlowPlug-mediated resets → trigger FWSEC → WPR2");
        eprintln!("{eq}");

        // Drop existing VFIO resources before reset experiments
        drop(bar0);
        drop(vfio_dev);

        // C1: Try remove-rescan (PCI hot-plug simulation — most likely to trigger FWSEC)
        for (step, method) in [("C1", "remove-rescan"), ("C2", "sbr"), ("C3", "auto")] {
            eprintln!("\n── {step}: Reset via GlowPlug method='{method}' ──");

            // Fresh nouveau cycle to establish known state
            let mut gp2 = match GlowPlugClient::connect() {
                Ok(g) => g,
                Err(e) => {
                    eprintln!("  GlowPlug connect failed: {e} — skipping {method}");
                    std::thread::sleep(std::time::Duration::from_secs(3));
                    continue;
                }
            };
            if let Err(e) = gp2.swap(&bdf, "nouveau") {
                eprintln!("  swap→nouveau failed: {e} — skipping {method}");
                std::thread::sleep(std::time::Duration::from_secs(3));
                continue;
            }
            std::thread::sleep(std::time::Duration::from_secs(3));
            if let Err(e) = gp2.swap(&bdf, "vfio-pci") {
                eprintln!("  swap→vfio-pci failed: {e} — skipping {method}");
                std::thread::sleep(std::time::Duration::from_secs(3));
                continue;
            }
            std::thread::sleep(std::time::Duration::from_millis(500));

            // Trigger the reset via GlowPlug RPC (runs as root)
            match gp2.reset(&bdf, method) {
                Ok(result) => {
                    eprintln!("  Reset '{method}': OK — {result}");
                    std::thread::sleep(std::time::Duration::from_secs(2));
                }
                Err(e) => {
                    eprintln!("  Reset '{method}': FAILED — {e}");
                    continue;
                }
            }

            // Re-acquire VFIO fds after reset
            match ember_client::request_fds(&bdf) {
                Ok(fds) => {
                    match coral_driver::vfio::VfioDevice::from_received(&bdf, fds) {
                        Ok(vfio_dev) => {
                            match vfio_dev.map_bar(0) {
                                Ok(bar0) => {
                                    let (ws, we, wv) = read_wpr2(&bar0);
                                    eprintln!(
                                        "  Post-{method} WPR2: start={ws:#x} end={we:#x} valid={wv}"
                                    );
                                    falcon_state(&bar0, "SEC2", r118::SEC2_BASE as usize);

                                    if wv {
                                        eprintln!("\n  *** {method} RE-ESTABLISHED WPR2! ***");
                                        eprintln!("  WPR2 range: {ws:#x}..{we:#x}");

                                        // Boot ACR with the real WPR2
                                        let fw = AcrFirmwareSet::load("gv100").expect("firmware");
                                        let container = vfio_dev.dma_backend();
                                        let cfg = BootConfig {
                                            pde_upper: true,
                                            acr_vram_pte: false,
                                            blob_size_zero: true,
                                            bind_vram: false,
                                            imem_preload: false,
                                            tlb_invalidate: true,
                                        };
                                        let result = attempt_sysmem_acr_boot_with_config(
                                            &bar0, &fw, container, &cfg,
                                        );
                                        eprintln!("  ACR boot: success={}", result.success);
                                        for note in &result.notes {
                                            eprintln!("  | {note}");
                                        }

                                        let bootvec = FalconBootvecOffsets {
                                            gpccs: fw.gpccs_bl.bl_imem_off(),
                                            fecs: fw.fecs_bl.bl_imem_off(),
                                        };
                                        let mb = attempt_acr_mailbox_command(&bar0, &bootvec);
                                        eprintln!("  BOOTSTRAP: success={}", mb.success);
                                        for note in &mb.notes {
                                            eprintln!("  | {note}");
                                        }

                                        eprintln!("\n── {step} Final State ──");
                                        falcon_state(&bar0, "SEC2", r118::SEC2_BASE as usize);
                                        falcon_state(&bar0, "FECS", r118::FECS_BASE as usize);
                                        falcon_state(&bar0, "GPCCS", r118::GPCCS_BASE as usize);
                                        dump_wpr_headers(&bar0, ws, "WPR2 after reset+ACR");

                                        break; // Success — stop trying other methods
                                    }
                                }
                                Err(e) => eprintln!("  BAR0 map after {method}: {e}"),
                            }
                        }
                        Err(e) => eprintln!("  VfioDevice after {method}: {e}"),
                    }
                }
                Err(e) => {
                    eprintln!("  Ember fds after {method}: {e}");
                    // After remove-rescan, device might need re-binding
                    let mut gp3 = GlowPlugClient::connect().expect("GlowPlug");
                    let _ = gp3.swap(&bdf, "vfio-pci");
                    std::thread::sleep(std::time::Duration::from_millis(500));

                    if let Ok(fds) = ember_client::request_fds(&bdf) {
                        if let Ok(vfio_dev) =
                            coral_driver::vfio::VfioDevice::from_received(&bdf, fds)
                        {
                            if let Ok(bar0) = vfio_dev.map_bar(0) {
                                let (ws, we, wv) = read_wpr2(&bar0);
                                eprintln!(
                                    "  Post-rebind WPR2: start={ws:#x} end={we:#x} valid={wv}"
                                );
                                falcon_state(&bar0, "SEC2", r118::SEC2_BASE as usize);
                            }
                        }
                    }
                }
            }
        }
    }

    // ══════════════════════════════════════════════════════════════════════
    // PHASE D: Capture nouveau's WPR2 content for analysis
    // ══════════════════════════════════════════════════════════════════════
    if wpr2_valid {
        eprintln!("\n{eq}");
        eprintln!("  PHASE D: Capture nouveau's WPR2 content from VRAM");
        eprintln!("{eq}");

        // Need fresh nouveau cycle with sysfs BAR0 for PRAMIN access
        // (We need write access to BAR0_WINDOW for PRAMIN sliding)
        eprintln!("\n── D1: Swap to nouveau to capture WPR2 ──");
        // If we still have VFIO bar0 from previous phases, drop it
        // Since we might have dropped and re-acquired, check if we can still swap
        let swap_ok = {
            let mut gp2 = GlowPlugClient::connect().expect("GlowPlug");
            gp2.swap(&bdf, "nouveau").is_ok()
        };

        if swap_ok {
            std::thread::sleep(std::time::Duration::from_secs(4));

            let sysfs_dev = format!("/sys/bus/pci/devices/{bdf}");
            match Bar0Access::from_sysfs_device(&sysfs_dev) {
                Ok(mut sbar0) => {
                    eprintln!(
                        "  Reading WPR2 region from VRAM ({wpr2_start:#x}..{wpr2_end:#x})..."
                    );
                    let wpr2_size = (wpr2_end - wpr2_start) as usize;
                    let mut wpr2_data = vec![0u32; wpr2_size / 4];
                    let mut read_ok = true;

                    // PRAMIN window is at BAR0 + 0x700000 (1 MiB aperture).
                    // BAR0_WINDOW register (0x1700) selects which 1 MiB of VRAM is visible.
                    let pramin_base: usize = 0x700000;

                    for chunk_idx in 0..(wpr2_size / (1024 * 1024) + 1) {
                        let vram_page = wpr2_start + (chunk_idx as u64 * 1024 * 1024);
                        let window_val = (vram_page >> 16) as u32;
                        let _ = sbar0.write_u32(0x1700, window_val);

                        let chunk_start = chunk_idx * (1024 * 1024 / 4);
                        let chunk_words = ((wpr2_size / 4) - chunk_start).min(1024 * 1024 / 4);

                        for i in 0..chunk_words {
                            let offset_in_window =
                                ((wpr2_start as usize & 0xFFFFF) + i * 4) % (1024 * 1024);
                            let val = sbar0
                                .read_u32((pramin_base + offset_in_window) as u32)
                                .unwrap_or(0xDEAD_DEAD);
                            if chunk_start + i < wpr2_data.len() {
                                wpr2_data[chunk_start + i] = val;
                            }
                            if val == 0xDEAD_DEAD {
                                read_ok = false;
                            }
                        }
                    }

                    eprintln!(
                        "  WPR2 capture: {} words ({} KiB), read_ok={read_ok}",
                        wpr2_data.len(),
                        wpr2_data.len() * 4 / 1024
                    );

                    // Display first 64 bytes of WPR2 content
                    eprintln!("\n  WPR2 first 64 bytes:");
                    for row in 0..4u32 {
                        let mut hex = String::new();
                        for col in 0..4u32 {
                            let idx = (row * 4 + col) as usize;
                            if idx < wpr2_data.len() {
                                hex.push_str(&format!("{:08x} ", wpr2_data[idx]));
                            }
                        }
                        eprintln!("    {:#010x}: {hex}", wpr2_start + (row as u64) * 16);
                    }

                    // WPR header analysis
                    eprintln!("\n  WPR2 header analysis:");
                    for i in 0..11u32 {
                        let base = (i * 6) as usize; // 24 bytes = 6 u32s per header
                        if base + 5 >= wpr2_data.len() {
                            break;
                        }
                        let falcon_id = wpr2_data[base];
                        if falcon_id == 0xFFFF_FFFF {
                            break;
                        }
                        let lsb_off = wpr2_data[base + 1];
                        let status = wpr2_data[base + 5];
                        let fname = match falcon_id {
                            0 => "PMU",
                            2 => "FECS",
                            3 => "GPCCS",
                            7 => "SEC2",
                            _ => "???",
                        };
                        let sname = match status {
                            0 => "NONE",
                            1 => "COPY",
                            4 => "VALID_DONE",
                            5 => "VALID_SKIP",
                            6 => "BOOT_READY",
                            _ => "?",
                        };
                        eprintln!(
                            "    [{i}] falcon={falcon_id}({fname}) lsb={lsb_off:#x} status={status}({sname})"
                        );
                    }

                    // Count non-zero words
                    let nz = wpr2_data.iter().filter(|&&v| v != 0).count();
                    eprintln!(
                        "\n  Non-zero words: {nz}/{} ({:.1}%)",
                        wpr2_data.len(),
                        nz as f64 / wpr2_data.len() as f64 * 100.0
                    );

                    drop(sbar0);
                }
                Err(e) => eprintln!("  BAR0 sysfs failed: {e}"),
            }

            // Swap back to vfio
            let mut gp3 = GlowPlugClient::connect().expect("GlowPlug");
            let _ = gp3.swap(&bdf, "vfio-pci");
            std::thread::sleep(std::time::Duration::from_millis(500));
        } else {
            eprintln!("  Could not swap to nouveau for WPR2 capture");
        }
    }

    // ══════════════════════════════════════════════════════════════════════
    // SUMMARY
    // ══════════════════════════════════════════════════════════════════════
    let sep = "=".repeat(70);
    eprintln!("\n{sep}");
    eprintln!("  Exp 118 RESULTS");
    eprintln!("{sep}");
    eprintln!("  WPR2 valid (nouveau):     {wpr2_valid}");
    if wpr2_valid {
        eprintln!("  WPR2 range:               {wpr2_start:#x}..{wpr2_end:#x}");
    }
    eprintln!("  WPR2 survived no-reset:   {w2v_a}");
    eprintln!("  SEC2 survived no-reset:   {sec2_alive} (SCTL={sec2_sctl_a:#06x})");
    eprintln!("  FECS survived no-reset:   {fecs_alive}");
    eprintln!("  GPCCS survived no-reset:  {gpccs_alive}");
    eprintln!("{sep}");

    if fecs_alive && gpccs_alive {
        eprintln!("\n  *** ALL FALCONS SURVIVED — COMPUTE PIPELINE READY ***");
    } else if sec2_alive && w2v_a {
        eprintln!("\n  *** SEC2 + WPR2 survived — BOOTSTRAP_FALCON should work ***");
    }

    eprintln!("\n=== Exp 118 Complete ===");
}
