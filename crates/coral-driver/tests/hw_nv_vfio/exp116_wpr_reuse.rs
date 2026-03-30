// SPDX-License-Identifier: AGPL-3.0-only

//! Exp 116: WPR2 Discovery + blob_size=0 ACR Boot + BOOTSTRAP_FALCON
//!
//! ROOT CAUSE ANALYSIS (from nouveau source comparison):
//!
//! Nouveau's `gp102_acr_load_setup` does NOT set `ucode_blob_size` or `ucode_blob_base`
//! in the ACR descriptor. They stay at 0 (firmware defaults). The WPR is pre-populated
//! in VRAM via `gp102_acr_wpr_alloc` (NVKM_MEM_TARGET_INST), and the ACR firmware
//! reads it directly from `region_props[0].start_addr`.
//!
//! Our code sets `ucode_blob_size = wpr_size`, triggering a blob DMA code path that
//! stalls trying to copy into the hardware WPR2 region (which FWSEC controls).
//!
//! Fix: Set blob_size=0 (like nouveau), with WPR pre-populated in VRAM.
//!
//! Variants:
//!   A: WPR2 discovery — read hardware WPR2 boundaries, dump WPR headers from VRAM
//!   B: ACR boot with blob_size=0 + our WPR mirrored to VRAM + BOOTSTRAP_FALCON
//!   C: ACR boot with blob_size=0 + nouveau's WPR2 addresses + BOOTSTRAP_FALCON
//!
//! Run:
//! ```sh
//! cargo test -p coral-driver --features vfio --test hw_nv_vfio \
//!   exp116 -- --ignored --nocapture --test-threads=1
//! ```

use crate::ember_client;
use crate::glowplug_client::GlowPlugClient;
use crate::helpers::init_tracing;
use coral_driver::nv::vfio_compute::acr_boot::{
    AcrFirmwareSet, BootConfig, FalconBootvecOffsets, attempt_acr_mailbox_command,
    attempt_sysmem_acr_boot_with_config,
};
use coral_driver::vfio::device::MappedBar;
use coral_driver::vfio::memory::{MemoryRegion, PraminRegion};

mod freg116 {
    pub const SEC2_BASE: usize = 0x087000;
    pub const FECS_BASE: usize = 0x409000;
    pub const GPCCS_BASE: usize = 0x41a000;
    pub const CPUCTL: usize = 0x100;
    pub const SCTL: usize = 0x240;
    pub const PC: usize = 0x030;
    pub const EXCI: usize = 0x148;
    pub const MAILBOX0: usize = 0x040;
    pub const MAILBOX1: usize = 0x044;
    pub const BOOTVEC: usize = 0x104;
    pub const HWCFG: usize = 0x108;
    pub const MTHD_STATUS: usize = 0xC18;

    pub const CPUCTL_HALTED: u32 = 1 << 4;
    pub const CPUCTL_STOPPED: u32 = 1 << 5;
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

fn nouveau_cycle(bdf: &str) {
    let mut gp = GlowPlugClient::connect().expect("GlowPlug connection");
    gp.swap(bdf, "nouveau").expect("swap→nouveau");
    std::thread::sleep(std::time::Duration::from_secs(3));
    gp.swap(bdf, "vfio-pci").expect("swap→vfio-pci");
    std::thread::sleep(std::time::Duration::from_millis(500));
}

fn falcon_state(bar0: &MappedBar, name: &str, base: usize) {
    let r = |off: usize| bar0.read_u32(base + off).unwrap_or(0xDEAD);
    let cpuctl = r(freg116::CPUCTL);
    let sctl = r(freg116::SCTL);
    let pc = r(freg116::PC);
    let exci = r(freg116::EXCI);
    let mb0 = r(freg116::MAILBOX0);
    let bootvec = r(freg116::BOOTVEC);
    let halted = cpuctl & freg116::CPUCTL_HALTED != 0;
    let stopped = cpuctl & freg116::CPUCTL_STOPPED != 0;
    let running = !halted && !stopped && cpuctl != 0xDEAD;
    eprintln!(
        "  {name:6}: cpuctl={cpuctl:#010x} HALTED={halted:<5} STOPPED={stopped:<5} \
         RUNNING={running:<5} SCTL={sctl:#06x} PC={pc:#06x} EXCI={exci:#010x} \
         MB0={mb0:#010x} BOOTVEC={bootvec:#06x}"
    );
}

/// Read WPR2 boundaries from hardware registers (0x100CD4 indexed reads).
/// This is what nouveau's `gm200_acr_wpr_check` does.
fn read_wpr2_boundaries(bar0: &MappedBar) -> (u64, u64) {
    let _ = bar0.write_u32(0x100CD4, 2);
    let raw_start = bar0.read_u32(0x100CD4).unwrap_or(0);
    let start = ((raw_start as u64) & 0xFFFF_FF00) << 8;

    let _ = bar0.write_u32(0x100CD4, 3);
    let raw_end = bar0.read_u32(0x100CD4).unwrap_or(0);
    let end = (((raw_end as u64) & 0xFFFF_FF00) << 8) + 0x20000;

    (start, end)
}

/// Dump WPR headers from VRAM via PRAMIN at the given base address.
fn dump_wpr_headers_from_vram(bar0: &MappedBar, vram_base: u64) {
    eprintln!("  Dumping WPR headers from VRAM {vram_base:#x}...");

    let base32 = vram_base as u32;
    match PraminRegion::new(bar0, base32, 264) {
        Ok(rgn) => {
            for i in 0..11 {
                let off = i * 24;
                let falcon_id = rgn.read_u32(off).unwrap_or(0xDEAD);
                if falcon_id == 0xFFFF_FFFF || falcon_id == 0xDEAD_DEAD {
                    eprintln!("  WPR[{i}]: falcon_id=INVALID (sentinel)");
                    break;
                }
                let lsb_off = rgn.read_u32(off + 4).unwrap_or(0);
                let owner = rgn.read_u32(off + 8).unwrap_or(0);
                let lazy = rgn.read_u32(off + 12).unwrap_or(0);
                let version = rgn.read_u32(off + 16).unwrap_or(0);
                let status = rgn.read_u32(off + 20).unwrap_or(0);
                let status_str = match status {
                    0 => "NONE",
                    1 => "COPY",
                    2 => "VALIDATION_CODE_FAILED",
                    3 => "VALIDATION_DATA_FAILED",
                    4 => "VALIDATION_DONE",
                    5 => "VALIDATION_SKIPPED",
                    6 => "BOOTSTRAP_READY",
                    7 => "REVOCATION_CHECK_FAILED",
                    _ => "UNKNOWN",
                };
                let fname = match falcon_id {
                    0 => "PMU",
                    2 => "FECS",
                    3 => "GPCCS",
                    7 => "SEC2",
                    _ => "???",
                };
                eprintln!(
                    "  WPR[{i}]: falcon={falcon_id}({fname}) lsb_off={lsb_off:#x} \
                     owner={owner} lazy={lazy} ver={version:#x} status={status}({status_str})"
                );
            }
        }
        Err(e) => {
            eprintln!("  PRAMIN access failed at {base32:#x}: {e}");
        }
    }
}

#[test]
#[ignore = "requires VFIO-bound GPU hardware + GlowPlug + Ember"]
fn exp116_wpr_reuse() {
    init_tracing();

    let banner = "#".repeat(70);
    eprintln!("\n{banner}");
    eprintln!("#  Exp 116: WPR2 Discovery + blob_size=0 + BOOTSTRAP_FALCON       #");
    eprintln!("#  Root cause: ucode_blob_size > 0 triggers stalled blob DMA path  #");
    eprintln!("#  Fix: Set blob_size=0 like nouveau, pre-populate WPR in VRAM     #");
    eprintln!("{banner}\n");

    let bdf = discover_bdf();
    eprintln!("Target BDF: {bdf}");

    // ══════════════════════════════════════════════════════════════════════
    // VARIANT A: WPR2 Hardware Discovery
    // ══════════════════════════════════════════════════════════════════════
    let eq = "=".repeat(70);
    eprintln!("\n{eq}");
    eprintln!("  VARIANT A: WPR2 Hardware Discovery");
    eprintln!("{eq}");

    eprintln!("\n── A1: Nouveau Cycle ──");
    nouveau_cycle(&bdf);

    let fds = ember_client::request_fds(&bdf).expect("ember fds");
    let vfio_dev = coral_driver::vfio::VfioDevice::from_received(&bdf, fds).expect("VfioDevice");
    let bar0 = vfio_dev.map_bar(0).expect("map_bar(0)");

    eprintln!("\n── A2: Post-Nouveau Falcon State ──");
    falcon_state(&bar0, "SEC2", freg116::SEC2_BASE);
    falcon_state(&bar0, "FECS", freg116::FECS_BASE);
    falcon_state(&bar0, "GPCCS", freg116::GPCCS_BASE);

    eprintln!("\n── A3: WPR2 Hardware Boundaries ──");
    let (wpr2_start, wpr2_end) = read_wpr2_boundaries(&bar0);
    let wpr2_size = if wpr2_end > wpr2_start {
        wpr2_end - wpr2_start
    } else {
        0
    };
    eprintln!(
        "  WPR2: start={wpr2_start:#x} end={wpr2_end:#x} size={wpr2_size:#x} ({} KiB)",
        wpr2_size / 1024
    );

    let wpr2_valid = wpr2_start > 0 && wpr2_end > wpr2_start && wpr2_size > 0x1000;
    eprintln!("  WPR2 valid: {wpr2_valid}");

    if wpr2_valid {
        eprintln!("\n── A4: WPR Headers from WPR2 Region ──");
        dump_wpr_headers_from_vram(&bar0, wpr2_start);

        let shadow_size = wpr2_size;
        let shadow_start = wpr2_start.saturating_sub(shadow_size);
        eprintln!("\n── A5: WPR Headers from Shadow Region (estimated) ──");
        eprintln!("  Shadow estimate: {shadow_start:#x} (WPR2 - {shadow_size:#x})");
        dump_wpr_headers_from_vram(&bar0, shadow_start);
    }

    eprintln!("\n── A6: WPR at our IOVA mirror addresses ──");
    dump_wpr_headers_from_vram(&bar0, 0x70000);
    dump_wpr_headers_from_vram(&bar0, 0x60000);

    let fw = AcrFirmwareSet::load("gv100").expect("firmware load");
    eprintln!(
        "\n  Firmware sizes: fecs_bl={}B fecs_inst={}B fecs_data={}B",
        fw.fecs_bl.code.len(),
        fw.fecs_inst.len(),
        fw.fecs_data.len()
    );
    eprintln!(
        "  Firmware sizes: gpccs_bl={}B gpccs_inst={}B gpccs_data={}B",
        fw.gpccs_bl.code.len(),
        fw.gpccs_inst.len(),
        fw.gpccs_data.len()
    );

    // ══════════════════════════════════════════════════════════════════════
    // VARIANT B: ACR boot blob_size=0 + VRAM mirror + BOOTSTRAP_FALCON
    // ══════════════════════════════════════════════════════════════════════
    eprintln!("\n{eq}");
    eprintln!("  VARIANT B: blob_size=0 + VRAM mirror + BOOTSTRAP_FALCON");
    eprintln!("  (This is the critical fix: match nouveau's blob_size=0 behavior)");
    eprintln!("{eq}");

    eprintln!("\n── B0: Fresh Nouveau Cycle ──");
    drop(bar0);
    drop(vfio_dev);
    nouveau_cycle(&bdf);
    let fds = ember_client::request_fds(&bdf).expect("ember fds");
    let vfio_dev = coral_driver::vfio::VfioDevice::from_received(&bdf, fds).expect("VfioDevice");
    let bar0 = vfio_dev.map_bar(0).expect("map_bar(0)");
    let container = vfio_dev.dma_backend();

    falcon_state(&bar0, "SEC2", freg116::SEC2_BASE);
    falcon_state(&bar0, "FECS", freg116::FECS_BASE);
    falcon_state(&bar0, "GPCCS", freg116::GPCCS_BASE);

    eprintln!("\n── B1: ACR boot (blob_size=0, correct PDEs, full PT chain) ──");
    let cfg_b = BootConfig {
        pde_upper: true,
        acr_vram_pte: false,
        blob_size_zero: true,
        bind_vram: false,
        imem_preload: false,
        tlb_invalidate: true,
    };
    eprintln!("  Config: {}", cfg_b.label());

    let result_b = attempt_sysmem_acr_boot_with_config(&bar0, &fw, container.clone(), &cfg_b);
    eprintln!("  Strategy: {}", result_b.strategy);
    eprintln!("  Success: {}", result_b.success);
    for note in &result_b.notes {
        eprintln!("  | {note}");
    }

    eprintln!("\n── B2: Post-ACR State ──");
    falcon_state(&bar0, "SEC2", freg116::SEC2_BASE);
    falcon_state(&bar0, "FECS", freg116::FECS_BASE);
    falcon_state(&bar0, "GPCCS", freg116::GPCCS_BASE);

    let sec2_pc = bar0.read_u32(freg116::SEC2_BASE + freg116::PC).unwrap_or(0);
    let sec2_cpuctl = bar0
        .read_u32(freg116::SEC2_BASE + freg116::CPUCTL)
        .unwrap_or(0);
    let sec2_alive = sec2_pc > 0x100
        && sec2_cpuctl & freg116::CPUCTL_HALTED == 0
        && sec2_cpuctl & freg116::CPUCTL_STOPPED == 0;
    eprintln!("  SEC2 alive (idle loop): {sec2_alive} (PC={sec2_pc:#x})");

    eprintln!("\n── B3: BOOTSTRAP_FALCON via mailbox ──");
    let bootvec = FalconBootvecOffsets {
        gpccs: fw.gpccs_bl.bl_imem_off(),
        fecs: fw.fecs_bl.bl_imem_off(),
    };
    eprintln!(
        "  BOOTVEC offsets: GPCCS={:#06x} FECS={:#06x}",
        bootvec.gpccs, bootvec.fecs
    );

    let mailbox_b = attempt_acr_mailbox_command(&bar0, &bootvec);
    eprintln!("  Mailbox strategy: {}", mailbox_b.strategy);
    eprintln!("  Mailbox success: {}", mailbox_b.success);
    for note in &mailbox_b.notes {
        eprintln!("  | {note}");
    }

    eprintln!("\n── B4: Post-BOOTSTRAP State ──");
    falcon_state(&bar0, "SEC2", freg116::SEC2_BASE);
    falcon_state(&bar0, "FECS", freg116::FECS_BASE);
    falcon_state(&bar0, "GPCCS", freg116::GPCCS_BASE);

    let fecs_b = bar0
        .read_u32(freg116::FECS_BASE + freg116::CPUCTL)
        .unwrap_or(0xDEAD);
    let gpccs_b = bar0
        .read_u32(freg116::GPCCS_BASE + freg116::CPUCTL)
        .unwrap_or(0xDEAD);
    let fecs_b_pc = bar0.read_u32(freg116::FECS_BASE + freg116::PC).unwrap_or(0);
    let gpccs_b_pc = bar0
        .read_u32(freg116::GPCCS_BASE + freg116::PC)
        .unwrap_or(0);
    let fecs_b_exci = bar0
        .read_u32(freg116::FECS_BASE + freg116::EXCI)
        .unwrap_or(0);
    let gpccs_b_exci = bar0
        .read_u32(freg116::GPCCS_BASE + freg116::EXCI)
        .unwrap_or(0);
    let fecs_b_running =
        fecs_b & (freg116::CPUCTL_HALTED | freg116::CPUCTL_STOPPED) == 0 && fecs_b != 0xDEAD;
    let gpccs_b_running =
        gpccs_b & (freg116::CPUCTL_HALTED | freg116::CPUCTL_STOPPED) == 0 && gpccs_b != 0xDEAD;

    // Check WPR status in VRAM after ACR
    eprintln!("\n── B5: WPR Header Status After ACR (VRAM) ──");
    dump_wpr_headers_from_vram(&bar0, 0x70000);
    dump_wpr_headers_from_vram(&bar0, 0x60000);
    let (wpr2_start_b, wpr2_end_b) = read_wpr2_boundaries(&bar0);
    if wpr2_start_b > 0 && wpr2_end_b > wpr2_start_b {
        dump_wpr_headers_from_vram(&bar0, wpr2_start_b);
    }

    // ══════════════════════════════════════════════════════════════════════
    // VARIANT C: ACR boot blob_size=0 + LEGACY PDEs (HS mode) + BOOTSTRAP
    // ══════════════════════════════════════════════════════════════════════
    eprintln!("\n{eq}");
    eprintln!("  VARIANT C: blob_size=0 + LEGACY PDEs (HS mode) + BOOTSTRAP");
    eprintln!("  (HS mode + skip blob DMA — does firmware process WPR in VRAM?)");
    eprintln!("{eq}");

    eprintln!("\n── C0: Fresh Nouveau Cycle ──");
    drop(bar0);
    drop(vfio_dev);
    nouveau_cycle(&bdf);
    let fds = ember_client::request_fds(&bdf).expect("ember fds");
    let vfio_dev = coral_driver::vfio::VfioDevice::from_received(&bdf, fds).expect("VfioDevice");
    let bar0 = vfio_dev.map_bar(0).expect("map_bar(0)");
    let container_c = vfio_dev.dma_backend();

    eprintln!("\n── C1: ACR boot (blob_size=0, LEGACY PDEs → HS mode) ──");
    let cfg_c = BootConfig {
        pde_upper: false,
        acr_vram_pte: false,
        blob_size_zero: true,
        bind_vram: false,
        imem_preload: false,
        tlb_invalidate: true,
    };
    eprintln!("  Config: {}", cfg_c.label());

    let result_c = attempt_sysmem_acr_boot_with_config(&bar0, &fw, container_c.clone(), &cfg_c);
    eprintln!("  Strategy: {}", result_c.strategy);
    eprintln!("  Success: {}", result_c.success);
    for note in &result_c.notes {
        eprintln!("  | {note}");
    }

    eprintln!("\n── C2: Post-ACR State (should be HS=0x3002) ──");
    falcon_state(&bar0, "SEC2", freg116::SEC2_BASE);
    falcon_state(&bar0, "FECS", freg116::FECS_BASE);
    falcon_state(&bar0, "GPCCS", freg116::GPCCS_BASE);

    let sec2_c_sctl = bar0
        .read_u32(freg116::SEC2_BASE + freg116::SCTL)
        .unwrap_or(0);
    let sec2_c_pc = bar0.read_u32(freg116::SEC2_BASE + freg116::PC).unwrap_or(0);
    let sec2_c_cpuctl = bar0
        .read_u32(freg116::SEC2_BASE + freg116::CPUCTL)
        .unwrap_or(0);
    let sec2_c_alive = sec2_c_pc > 0x100
        && sec2_c_cpuctl & freg116::CPUCTL_HALTED == 0
        && sec2_c_cpuctl & freg116::CPUCTL_STOPPED == 0;
    eprintln!("  SEC2: alive={sec2_c_alive} SCTL={sec2_c_sctl:#06x} PC={sec2_c_pc:#x}");

    eprintln!("\n── C3: WPR Header Status After HS ACR (VRAM) ──");
    dump_wpr_headers_from_vram(&bar0, 0x70000);
    let (wpr2_c_start, wpr2_c_end) = read_wpr2_boundaries(&bar0);
    if wpr2_c_start > 0 && wpr2_c_end > wpr2_c_start {
        eprintln!("  WPR2: {wpr2_c_start:#x}..{wpr2_c_end:#x}");
        dump_wpr_headers_from_vram(&bar0, wpr2_c_start);
    }

    // ══════════════════════════════════════════════════════════════════════
    // VARIANT D: blob_size>0 but with VRAM WPR address matching WPR2
    // (If WPR2 is valid, try putting our WPR blob at the actual WPR2 address)
    // ══════════════════════════════════════════════════════════════════════
    let do_variant_d = wpr2_valid;

    let (fecs_d_running, gpccs_d_running) = if do_variant_d {
        eprintln!("\n{eq}");
        eprintln!("  VARIANT D: Our WPR at actual WPR2 addresses + blob_size=0");
        eprintln!("{eq}");

        eprintln!("\n── D0: Fresh Nouveau Cycle ──");
        drop(bar0);
        drop(vfio_dev);
        nouveau_cycle(&bdf);
        let fds = ember_client::request_fds(&bdf).expect("ember fds");
        let vfio_dev =
            coral_driver::vfio::VfioDevice::from_received(&bdf, fds).expect("VfioDevice");
        let bar0 = vfio_dev.map_bar(0).expect("map_bar(0)");
        let container_d = vfio_dev.dma_backend();

        // Read fresh WPR2 boundaries
        let (w2s, w2e) = read_wpr2_boundaries(&bar0);
        let w2_size = w2e.saturating_sub(w2s);
        eprintln!("  WPR2: {w2s:#x}..{w2e:#x} ({} KiB)", w2_size / 1024);

        // Check what nouveau left in WPR2
        eprintln!("\n── D1: Nouveau's WPR2 Content ──");
        dump_wpr_headers_from_vram(&bar0, w2s);

        // Build our WPR blob with DMA addresses relative to WPR2 start
        eprintln!("\n── D2: Build WPR at WPR2 address + mirror to VRAM ──");
        let wpr_data = coral_driver::nv::vfio_compute::acr_boot::build_wpr(&fw, w2s);
        eprintln!("  WPR blob: {} bytes, base={w2s:#x}", wpr_data.len());

        // Mirror to VRAM at WPR2 address via PRAMIN
        let mut mirror_ok = true;
        let mut offset = 0usize;
        while offset < wpr_data.len() {
            let chunk_addr = w2s as u32 + offset as u32;
            let chunk_size = (wpr_data.len() - offset).min(0xC000);
            match PraminRegion::new(&bar0, chunk_addr, chunk_size) {
                Ok(mut rgn) => {
                    for wo in (0..chunk_size).step_by(4) {
                        let src = offset + wo;
                        if src >= wpr_data.len() {
                            break;
                        }
                        let end = (src + 4).min(wpr_data.len());
                        let mut bytes = [0u8; 4];
                        bytes[..end - src].copy_from_slice(&wpr_data[src..end]);
                        if rgn.write_u32(wo, u32::from_le_bytes(bytes)).is_err() {
                            mirror_ok = false;
                            break;
                        }
                    }
                    offset += chunk_size;
                }
                Err(e) => {
                    eprintln!("  PRAMIN write failed at {chunk_addr:#x}: {e}");
                    mirror_ok = false;
                    break;
                }
            }
        }
        eprintln!("  VRAM mirror to WPR2: {mirror_ok}");

        // Also write shadow copy just before WPR2
        let shadow_addr = w2s.saturating_sub(wpr_data.len() as u64);
        let shadow_addr = shadow_addr & !0xFF; // align
        let mut shadow_ok = true;
        let mut offset = 0usize;
        while offset < wpr_data.len() {
            let chunk_addr = shadow_addr as u32 + offset as u32;
            let chunk_size = (wpr_data.len() - offset).min(0xC000);
            match PraminRegion::new(&bar0, chunk_addr, chunk_size) {
                Ok(mut rgn) => {
                    for wo in (0..chunk_size).step_by(4) {
                        let src = offset + wo;
                        if src >= wpr_data.len() {
                            break;
                        }
                        let end = (src + 4).min(wpr_data.len());
                        let mut bytes = [0u8; 4];
                        bytes[..end - src].copy_from_slice(&wpr_data[src..end]);
                        if rgn.write_u32(wo, u32::from_le_bytes(bytes)).is_err() {
                            shadow_ok = false;
                            break;
                        }
                    }
                    offset += chunk_size;
                }
                Err(e) => {
                    eprintln!("  Shadow write failed at {chunk_addr:#x}: {e}");
                    shadow_ok = false;
                    break;
                }
            }
        }
        eprintln!("  VRAM shadow at {shadow_addr:#x}: {shadow_ok}");

        // Verify WPR headers are readable
        eprintln!("\n── D3: Verify WPR written to VRAM ──");
        dump_wpr_headers_from_vram(&bar0, w2s);

        // Boot ACR with blob_size=0 — this time the WPR is at the HW WPR2 addresses.
        // We need a custom ACR descriptor patch with WPR2 addresses.
        // For now, use the standard boot path which mirrors to 0x70000.
        // ALTERNATIVE: We patch the ACR descriptor manually.
        //
        // The standard `attempt_sysmem_acr_boot_with_config` uses our IOVA addresses
        // (0x70000 for WPR, 0x60000 for shadow). These are mirrored to VRAM at the
        // same addresses. But WPR2 is at a different address.
        //
        // For the best test, we'd need a custom descriptor patch. For now, test with
        // the standard path first — if blob_size=0 works with our addresses, the
        // firmware reads from VRAM 0x70000 (our mirror) instead of HW WPR2.

        eprintln!("\n── D4: ACR boot (blob_size=0, correct PDEs) ──");
        let cfg_d = BootConfig {
            pde_upper: true,
            acr_vram_pte: false,
            blob_size_zero: true,
            bind_vram: false,
            imem_preload: false,
            tlb_invalidate: true,
        };

        let result_d = attempt_sysmem_acr_boot_with_config(&bar0, &fw, container_d, &cfg_d);
        eprintln!("  Strategy: {}", result_d.strategy);
        eprintln!("  Success: {}", result_d.success);
        for note in &result_d.notes {
            eprintln!("  | {note}");
        }

        eprintln!("\n── D5: BOOTSTRAP_FALCON ──");
        let bootvec_d = FalconBootvecOffsets {
            gpccs: fw.gpccs_bl.bl_imem_off(),
            fecs: fw.fecs_bl.bl_imem_off(),
        };
        let mailbox_d = attempt_acr_mailbox_command(&bar0, &bootvec_d);
        eprintln!("  Mailbox: success={}", mailbox_d.success);
        for note in &mailbox_d.notes {
            eprintln!("  | {note}");
        }

        eprintln!("\n── D6: Final State ──");
        falcon_state(&bar0, "SEC2", freg116::SEC2_BASE);
        falcon_state(&bar0, "FECS", freg116::FECS_BASE);
        falcon_state(&bar0, "GPCCS", freg116::GPCCS_BASE);

        let fd = bar0
            .read_u32(freg116::FECS_BASE + freg116::CPUCTL)
            .unwrap_or(0xDEAD);
        let gd = bar0
            .read_u32(freg116::GPCCS_BASE + freg116::CPUCTL)
            .unwrap_or(0xDEAD);
        (
            fd & (freg116::CPUCTL_HALTED | freg116::CPUCTL_STOPPED) == 0 && fd != 0xDEAD,
            gd & (freg116::CPUCTL_HALTED | freg116::CPUCTL_STOPPED) == 0 && gd != 0xDEAD,
        )
    } else {
        eprintln!("\n  VARIANT D: SKIPPED (no valid WPR2 boundaries)");
        (false, false)
    };

    // ══════════════════════════════════════════════════════════════════════
    // SUMMARY
    // ══════════════════════════════════════════════════════════════════════
    let sep = "=".repeat(70);
    eprintln!("\n{sep}");
    eprintln!("  Exp 116 RESULTS");
    eprintln!("{sep}");
    eprintln!("  WPR2 boundaries: start={wpr2_start:#x} end={wpr2_end:#x} valid={wpr2_valid}");
    eprintln!(
        "  B (blob=0, correct PDEs): FECS running={fecs_b_running} pc={fecs_b_pc:#06x} \
         exci={fecs_b_exci:#010x} | GPCCS running={gpccs_b_running} pc={gpccs_b_pc:#06x} \
         exci={gpccs_b_exci:#010x}"
    );
    if do_variant_d {
        eprintln!("  D (blob=0, WPR2 mirror): FECS={fecs_d_running} GPCCS={gpccs_d_running}");
    }
    eprintln!("{sep}");

    if fecs_b_running && gpccs_b_running {
        eprintln!("\n  *** BOTH FECS AND GPCCS ALIVE via blob_size=0 FIX! ***");
        eprintln!("  *** L10 SOLVED — Next: L11 GR context init + shader dispatch ***");
    } else if fecs_b_running || gpccs_b_running {
        eprintln!("\n  *** PARTIAL SUCCESS: FECS={fecs_b_running} GPCCS={gpccs_b_running} ***");
    }

    if fecs_d_running && gpccs_d_running {
        eprintln!("\n  *** VARIANT D: BOTH ALIVE via WPR2 address fix! ***");
    }

    eprintln!("\n=== Exp 116 Complete ===");
}
