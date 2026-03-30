// SPDX-License-Identifier: AGPL-3.0-only

//! Exp 115: PMC GR Reset + Direct Falcon Upload (Bypass ACR Entirely)
//!
//! Exp 114 proved the ACR WPR copy stalls in both HS and LS modes. The copy-to-target
//! mechanism is broken regardless of security level. Instead of going through ACR at all,
//! we bypass it and upload firmware directly via PIO.
//!
//! Key insight from Exp 089b: PMC GR reset breaks the ACR CPUCTL lock, making
//! FECS/GPCCS writable by the host. Combined with the BOOTVEC fix (Exp 091) and
//! PIO accessibility in LS mode (Exp 091+), this should allow direct falcon start.
//!
//! Variants:
//!   A: BOOTVEC=0 (direct app entry, nouveau non-secure style) — no BL/DMA
//!   B: BOOTVEC=BL entry (BL at IMEM[0x3400/0x7E00]) — BL may need DMA
//!   C: Probed capabilities boot (runtime-discovered PIO format)
//!
//! Run:
//! ```sh
//! cargo test -p coral-driver --features vfio --test hw_nv_vfio \
//!   exp115 -- --ignored --nocapture --test-threads=1
//! ```

use crate::ember_client;
use crate::glowplug_client::GlowPlugClient;
use crate::helpers::init_tracing;
use coral_driver::nv::vfio_compute::acr_boot::{AcrFirmwareSet, attempt_direct_falcon_upload};
use coral_driver::nv::vfio_compute::fecs_boot;
use coral_driver::vfio::device::MappedBar;

mod freg115 {
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
    pub const ITFEN: usize = 0x048;
    pub const IRQMODE: usize = 0x00c;
    pub const IMEMC: usize = 0x180;
    pub const IMEMD: usize = 0x184;
    pub const DMEMC: usize = 0x1c0;
    pub const DMEMD: usize = 0x1c4;
    pub const MTHD_STATUS: usize = 0xC18;
    pub const HWCFG: usize = 0x108;

    pub const CPUCTL_IINVAL: u32 = 1 << 0;
    pub const CPUCTL_STARTCPU: u32 = 1 << 1;
    pub const CPUCTL_HALTED: u32 = 1 << 4;
    pub const CPUCTL_STOPPED: u32 = 1 << 5;

    pub const PMC_ENABLE: usize = 0x0000_0200;
    pub const PMC_UNK260: usize = 0x0000_0260;
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
    let cpuctl = r(freg115::CPUCTL);
    let sctl = r(freg115::SCTL);
    let pc = r(freg115::PC);
    let exci = r(freg115::EXCI);
    let mb0 = r(freg115::MAILBOX0);
    let bootvec = r(freg115::BOOTVEC);
    let halted = cpuctl & freg115::CPUCTL_HALTED != 0;
    let stopped = cpuctl & freg115::CPUCTL_STOPPED != 0;
    let running = !halted && !stopped && cpuctl != 0xDEAD;
    eprintln!(
        "  {name:6}: cpuctl={cpuctl:#010x} HALTED={halted:<5} STOPPED={stopped:<5} RUNNING={running:<5} \
         SCTL={sctl:#06x} PC={pc:#06x} EXCI={exci:#010x} MB0={mb0:#010x} BOOTVEC={bootvec:#06x}"
    );
}

fn pmc_gr_reset(bar0: &MappedBar) {
    let pmc = bar0.read_u32(freg115::PMC_ENABLE).unwrap_or(0);
    let gr_bit: u32 = 1 << 12;
    eprintln!("  PMC_ENABLE before: {pmc:#010x}");

    let _ = bar0.write_u32(freg115::PMC_ENABLE, pmc & !gr_bit);
    let _ = bar0.read_u32(freg115::PMC_ENABLE); // flush
    std::thread::sleep(std::time::Duration::from_micros(20));
    let _ = bar0.write_u32(freg115::PMC_ENABLE, pmc | gr_bit);
    let _ = bar0.read_u32(freg115::PMC_ENABLE); // flush
    std::thread::sleep(std::time::Duration::from_millis(5));

    let pmc_after = bar0.read_u32(freg115::PMC_ENABLE).unwrap_or(0);
    eprintln!("  PMC_ENABLE after:  {pmc_after:#010x}");
}

fn imem_upload(bar0: &MappedBar, base: usize, addr: u32, data: &[u8], tag: u32) {
    let imemc_val: u32 = (1u32 << 24) | addr;
    let _ = bar0.write_u32(base + freg115::IMEMC, imemc_val);
    for (i, chunk) in data.chunks(4).enumerate() {
        let byte_off = (i * 4) as u32;
        if byte_off & 0xFF == 0 {
            let _ = bar0.write_u32(base + 0x188, tag + (byte_off >> 8)); // IMEMT
        }
        let word = match chunk.len() {
            4 => u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]),
            _ => {
                let mut b = [0u8; 4];
                b[..chunk.len()].copy_from_slice(chunk);
                u32::from_le_bytes(b)
            }
        };
        let _ = bar0.write_u32(base + freg115::IMEMD, word);
    }
}

fn dmem_upload(bar0: &MappedBar, base: usize, addr: u32, data: &[u8]) {
    let _ = bar0.write_u32(base + freg115::DMEMC, (1u32 << 24) | addr);
    for chunk in data.chunks(4) {
        let word = match chunk.len() {
            4 => u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]),
            _ => {
                let mut b = [0u8; 4];
                b[..chunk.len()].copy_from_slice(chunk);
                u32::from_le_bytes(b)
            }
        };
        let _ = bar0.write_u32(base + freg115::DMEMD, word);
    }
}

fn imem_read(bar0: &MappedBar, base: usize, addr: u32, count: usize) -> Vec<u32> {
    let _ = bar0.write_u32(base + freg115::IMEMC, 0x0200_0000 | addr);
    (0..count)
        .map(|_| bar0.read_u32(base + freg115::IMEMD).unwrap_or(0xDEAD_DEAD))
        .collect()
}

fn start_falcon(bar0: &MappedBar, base: usize) {
    let _ = bar0.write_u32(base + freg115::CPUCTL, freg115::CPUCTL_IINVAL);
    std::thread::sleep(std::time::Duration::from_millis(1));
    let _ = bar0.write_u32(base + freg115::CPUCTL, freg115::CPUCTL_STARTCPU);
}

fn poll_falcon(bar0: &MappedBar, name: &str, base: usize, timeout_ms: u64) -> (u32, u32, u32) {
    let start = std::time::Instant::now();
    let timeout = std::time::Duration::from_millis(timeout_ms);
    loop {
        std::thread::sleep(std::time::Duration::from_millis(10));
        let cpuctl = bar0.read_u32(base + freg115::CPUCTL).unwrap_or(0xDEAD);
        let pc = bar0.read_u32(base + freg115::PC).unwrap_or(0);
        let exci = bar0.read_u32(base + freg115::EXCI).unwrap_or(0);
        let mb0 = bar0.read_u32(base + freg115::MAILBOX0).unwrap_or(0);

        let halted = cpuctl & freg115::CPUCTL_HALTED != 0;
        let stopped = cpuctl & freg115::CPUCTL_STOPPED != 0;
        let running = !halted && !stopped;

        if mb0 != 0 || running || stopped || start.elapsed() > timeout {
            eprintln!(
                "  {name:6} poll: cpuctl={cpuctl:#010x} PC={pc:#06x} EXCI={exci:#010x} \
                 MB0={mb0:#010x} ({}ms)",
                start.elapsed().as_millis()
            );
            return (cpuctl, pc, exci);
        }
    }
}

#[test]
#[ignore = "requires VFIO-bound GPU hardware + GlowPlug + Ember"]
fn exp115_direct_boot() {
    init_tracing();

    let banner = "#".repeat(60);
    eprintln!("\n{banner}");
    eprintln!("#  Exp 115: PMC GR Reset + Direct Falcon Upload            #");
    eprintln!("#  Bypass ACR — upload firmware via PIO, start directly     #");
    eprintln!("{banner}\n");

    let bdf = discover_bdf();
    eprintln!("Target BDF: {bdf}");

    // ── Phase 1: Nouveau cycle ──
    eprintln!("\n── Phase 1: Nouveau Cycle ──");
    nouveau_cycle(&bdf);

    let fds = ember_client::request_fds(&bdf).expect("ember fds");
    let vfio_dev = coral_driver::vfio::VfioDevice::from_received(&bdf, fds).expect("VfioDevice");
    let bar0 = vfio_dev.map_bar(0).expect("map_bar(0)");

    eprintln!("\n── Phase 1b: Post-Nouveau State ──");
    falcon_state(&bar0, "SEC2", freg115::SEC2_BASE);
    falcon_state(&bar0, "FECS", freg115::FECS_BASE);
    falcon_state(&bar0, "GPCCS", freg115::GPCCS_BASE);

    let fw = AcrFirmwareSet::load("gv100").expect("firmware load");
    eprintln!(
        "  Firmware: gpccs_inst={}B gpccs_bl={}B(off={:#06x}) gpccs_data={}B",
        fw.gpccs_inst.len(),
        fw.gpccs_bl.code.len(),
        fw.gpccs_bl.bl_imem_off(),
        fw.gpccs_data.len()
    );
    eprintln!(
        "  Firmware: fecs_inst={}B fecs_bl={}B(off={:#06x}) fecs_data={}B",
        fw.fecs_inst.len(),
        fw.fecs_bl.code.len(),
        fw.fecs_bl.bl_imem_off(),
        fw.fecs_data.len()
    );

    // ══════════════════════════════════════════════════════════════
    // VARIANT A: BOOTVEC=0 (direct app entry, no BL/DMA)
    // ══════════════════════════════════════════════════════════════
    let eq60 = "=".repeat(60);
    eprintln!("\n{eq60}");
    eprintln!("  VARIANT A: BOOTVEC=0 (direct app entry, no BL/DMA)");
    eprintln!("{eq60}");

    eprintln!("\n── A1: PMC GR Reset ──");
    pmc_gr_reset(&bar0);
    falcon_state(&bar0, "GPCCS", freg115::GPCCS_BASE);
    falcon_state(&bar0, "FECS", freg115::FECS_BASE);

    // Verify CPUCTL is writable after PMC reset
    let gpccs_cpuctl = bar0
        .read_u32(freg115::GPCCS_BASE + freg115::CPUCTL)
        .unwrap_or(0xDEAD);
    eprintln!("  GPCCS CPUCTL after PMC reset: {gpccs_cpuctl:#010x}");
    let _ = bar0.write_u32(
        freg115::GPCCS_BASE + freg115::CPUCTL,
        freg115::CPUCTL_HALTED,
    );
    let gpccs_cpuctl_rb = bar0
        .read_u32(freg115::GPCCS_BASE + freg115::CPUCTL)
        .unwrap_or(0xDEAD);
    let writable =
        gpccs_cpuctl_rb == freg115::CPUCTL_HALTED || gpccs_cpuctl_rb & freg115::CPUCTL_HALTED != 0;
    eprintln!(
        "  GPCCS CPUCTL write test: wrote={:#010x} read={gpccs_cpuctl_rb:#010x} writable={writable}",
        freg115::CPUCTL_HALTED
    );

    eprintln!("\n── A2: Upload GPCCS firmware (inst+data, no BL) ──");
    imem_upload(&bar0, freg115::GPCCS_BASE, 0, &fw.gpccs_inst, 0);
    dmem_upload(&bar0, freg115::GPCCS_BASE, 0, &fw.gpccs_data);

    let imem_check = imem_read(&bar0, freg115::GPCCS_BASE, 0, 4);
    eprintln!(
        "  GPCCS IMEM[0x0000]: {:08x} {:08x} {:08x} {:08x}",
        imem_check[0], imem_check[1], imem_check[2], imem_check[3]
    );

    eprintln!("\n── A3: Configure + Start GPCCS (BOOTVEC=0) ──");
    let _ = bar0.write_u32(freg115::GPCCS_BASE + freg115::BOOTVEC, 0);
    let _ = bar0.write_u32(freg115::GPCCS_BASE + freg115::ITFEN, 0x04);
    let _ = bar0.write_u32(freg115::GPCCS_BASE + freg115::IRQMODE, 0xfc24);
    let _ = bar0.write_u32(freg115::GPCCS_BASE + freg115::MAILBOX0, 0);
    let _ = bar0.write_u32(freg115::GPCCS_BASE + freg115::MAILBOX1, 0);
    start_falcon(&bar0, freg115::GPCCS_BASE);
    let (gpccs_a_cpu, gpccs_a_pc, gpccs_a_exci) =
        poll_falcon(&bar0, "GPCCS", freg115::GPCCS_BASE, 200);

    let gpccs_a_ok = gpccs_a_exci == 0 && gpccs_a_pc != 0;
    eprintln!("  GPCCS-A result: alive={gpccs_a_ok}");

    // Upload + start FECS (BOOTVEC=0)
    eprintln!("\n── A4: Upload + Start FECS (BOOTVEC=0) ──");
    imem_upload(&bar0, freg115::FECS_BASE, 0, &fw.fecs_inst, 0);
    dmem_upload(&bar0, freg115::FECS_BASE, 0, &fw.fecs_data);
    let _ = bar0.write_u32(freg115::FECS_BASE + freg115::BOOTVEC, 0);
    let _ = bar0.write_u32(freg115::FECS_BASE + freg115::ITFEN, 0x04);
    let _ = bar0.write_u32(freg115::FECS_BASE + freg115::IRQMODE, 0xfc24);
    let _ = bar0.write_u32(freg115::FECS_BASE + freg115::MAILBOX0, 0);
    let _ = bar0.write_u32(freg115::FECS_BASE + freg115::MAILBOX1, 0);
    start_falcon(&bar0, freg115::FECS_BASE);
    let (fecs_a_cpu, fecs_a_pc, fecs_a_exci) = poll_falcon(&bar0, "FECS", freg115::FECS_BASE, 200);

    let fecs_a_ok = fecs_a_exci == 0 && fecs_a_pc != 0;
    eprintln!("  FECS-A result: alive={fecs_a_ok}");

    // ══════════════════════════════════════════════════════════════
    // VARIANT B: BOOTVEC=BL entry (inst+BL+data, BL needs DMA)
    // ══════════════════════════════════════════════════════════════
    eprintln!("\n{eq60}");
    eprintln!("  VARIANT B: BOOTVEC=BL entry (inst+BL+data)");
    eprintln!("{eq60}");

    // Fresh nouveau cycle for clean state
    eprintln!("\n── B0: Fresh Nouveau Cycle ──");
    drop(bar0);
    drop(vfio_dev);
    nouveau_cycle(&bdf);
    let fds = ember_client::request_fds(&bdf).expect("ember fds");
    let vfio_dev = coral_driver::vfio::VfioDevice::from_received(&bdf, fds).expect("VfioDevice");
    let bar0 = vfio_dev.map_bar(0).expect("map_bar(0)");

    eprintln!("\n── B1: PMC GR Reset ──");
    pmc_gr_reset(&bar0);
    falcon_state(&bar0, "GPCCS", freg115::GPCCS_BASE);

    eprintln!("\n── B2: Upload GPCCS firmware (inst+BL+data) ──");
    let gpccs_bl_off = fw.gpccs_bl.bl_imem_off();
    imem_upload(&bar0, freg115::GPCCS_BASE, 0, &fw.gpccs_inst, 0);
    imem_upload(
        &bar0,
        freg115::GPCCS_BASE,
        gpccs_bl_off,
        &fw.gpccs_bl.code,
        fw.gpccs_bl.start_tag,
    );
    dmem_upload(&bar0, freg115::GPCCS_BASE, 0, &fw.gpccs_data);

    let imem_bl = imem_read(&bar0, freg115::GPCCS_BASE, gpccs_bl_off, 4);
    eprintln!(
        "  GPCCS IMEM[{gpccs_bl_off:#06x}]: {:08x} {:08x} {:08x} {:08x}",
        imem_bl[0], imem_bl[1], imem_bl[2], imem_bl[3]
    );

    eprintln!("\n── B3: Configure + Start GPCCS (BOOTVEC={gpccs_bl_off:#06x}) ──");
    let _ = bar0.write_u32(freg115::GPCCS_BASE + freg115::BOOTVEC, gpccs_bl_off);
    let _ = bar0.write_u32(freg115::GPCCS_BASE + freg115::ITFEN, 0x04);
    let _ = bar0.write_u32(freg115::GPCCS_BASE + freg115::IRQMODE, 0xfc24);
    let _ = bar0.write_u32(freg115::GPCCS_BASE + freg115::MAILBOX0, 0);
    let _ = bar0.write_u32(freg115::GPCCS_BASE + freg115::MAILBOX1, 0);
    start_falcon(&bar0, freg115::GPCCS_BASE);
    let (gpccs_b_cpu, gpccs_b_pc, gpccs_b_exci) =
        poll_falcon(&bar0, "GPCCS", freg115::GPCCS_BASE, 500);

    let gpccs_b_ok = gpccs_b_exci == 0 && gpccs_b_pc != 0;
    eprintln!("  GPCCS-B result: alive={gpccs_b_ok}");

    // FECS with BL entry
    let fecs_bl_off = fw.fecs_bl.bl_imem_off();
    eprintln!("\n── B4: Upload + Start FECS (BOOTVEC={fecs_bl_off:#06x}) ──");
    imem_upload(&bar0, freg115::FECS_BASE, 0, &fw.fecs_inst, 0);
    imem_upload(
        &bar0,
        freg115::FECS_BASE,
        fecs_bl_off,
        &fw.fecs_bl.code,
        fw.fecs_bl.start_tag,
    );
    dmem_upload(&bar0, freg115::FECS_BASE, 0, &fw.fecs_data);
    let _ = bar0.write_u32(freg115::FECS_BASE + freg115::BOOTVEC, fecs_bl_off);
    let _ = bar0.write_u32(freg115::FECS_BASE + freg115::ITFEN, 0x04);
    let _ = bar0.write_u32(freg115::FECS_BASE + freg115::IRQMODE, 0xfc24);
    let _ = bar0.write_u32(freg115::FECS_BASE + freg115::MAILBOX0, 0);
    let _ = bar0.write_u32(freg115::FECS_BASE + freg115::MAILBOX1, 0);
    start_falcon(&bar0, freg115::FECS_BASE);
    let (fecs_b_cpu, fecs_b_pc, fecs_b_exci) = poll_falcon(&bar0, "FECS", freg115::FECS_BASE, 500);

    let fecs_b_ok = fecs_b_exci == 0 && fecs_b_pc != 0;
    eprintln!("  FECS-B result: alive={fecs_b_ok}");

    // ══════════════════════════════════════════════════════════════
    // VARIANT C: attempt_direct_falcon_upload (existing function)
    // ══════════════════════════════════════════════════════════════
    eprintln!("\n{eq60}");
    eprintln!("  VARIANT C: attempt_direct_falcon_upload (existing)");
    eprintln!("{eq60}");

    eprintln!("\n── C0: Fresh Nouveau Cycle ──");
    drop(bar0);
    drop(vfio_dev);
    nouveau_cycle(&bdf);
    let fds = ember_client::request_fds(&bdf).expect("ember fds");
    let vfio_dev = coral_driver::vfio::VfioDevice::from_received(&bdf, fds).expect("VfioDevice");
    let bar0 = vfio_dev.map_bar(0).expect("map_bar(0)");

    eprintln!("\n── C1: Run attempt_direct_falcon_upload ──");
    let result_c = attempt_direct_falcon_upload(&bar0, &fw);
    eprintln!("  Strategy: {}", result_c.strategy);
    eprintln!("  Success: {}", result_c.success);
    for note in &result_c.notes {
        eprintln!("  | {note}");
    }

    // ══════════════════════════════════════════════════════════════
    // VARIANT D: boot_gpccs + boot_fecs (fecs_boot module)
    // ══════════════════════════════════════════════════════════════
    eprintln!("\n{eq60}");
    eprintln!("  VARIANT D: fecs_boot::boot_gpccs + boot_fecs");
    eprintln!("{eq60}");

    eprintln!("\n── D0: Fresh Nouveau Cycle ──");
    drop(bar0);
    drop(vfio_dev);
    nouveau_cycle(&bdf);
    let fds = ember_client::request_fds(&bdf).expect("ember fds");
    let vfio_dev = coral_driver::vfio::VfioDevice::from_received(&bdf, fds).expect("VfioDevice");
    let bar0 = vfio_dev.map_bar(0).expect("map_bar(0)");

    // PMC GR reset first (needed to break CPUCTL lock)
    eprintln!("\n── D1: PMC GR Reset ──");
    pmc_gr_reset(&bar0);

    eprintln!("\n── D2: boot_gpccs ──");
    match fecs_boot::boot_gpccs(&bar0, "gv100") {
        Ok(result) => {
            eprintln!("  GPCCS: {result}");
        }
        Err(e) => {
            eprintln!("  GPCCS boot_gpccs failed: {e}");
        }
    }

    eprintln!("\n── D3: boot_fecs ──");
    match fecs_boot::boot_fecs(&bar0, "gv100") {
        Ok(result) => {
            eprintln!("  FECS: {result}");
        }
        Err(e) => {
            eprintln!("  FECS boot_fecs failed: {e}");
        }
    }

    // ══════════════════════════════════════════════════════════════
    // VARIANT E: ACR LS-mode with acr_vram_pte=FALSE (fix WPR stall)
    // ══════════════════════════════════════════════════════════════
    eprintln!("\n{eq60}");
    eprintln!("  VARIANT E: ACR LS-mode + acr_vram_pte=false");
    eprintln!("  (Hypothesis: VRAM PTEs caused WPR copy stall in Exp 114)");
    eprintln!("{eq60}");

    eprintln!("\n── E0: Fresh Nouveau Cycle ──");
    drop(bar0);
    drop(vfio_dev);
    nouveau_cycle(&bdf);
    let fds = ember_client::request_fds(&bdf).expect("ember fds");
    let vfio_dev = coral_driver::vfio::VfioDevice::from_received(&bdf, fds).expect("VfioDevice");
    let bar0 = vfio_dev.map_bar(0).expect("map_bar(0)");
    let container = vfio_dev.dma_backend();

    falcon_state(&bar0, "SEC2", freg115::SEC2_BASE);
    falcon_state(&bar0, "FECS", freg115::FECS_BASE);
    falcon_state(&bar0, "GPCCS", freg115::GPCCS_BASE);

    eprintln!("\n── E1: Sysmem ACR boot (correct PDEs, acr_vram_pte=FALSE, full init) ──");
    let cfg_e = coral_driver::nv::vfio_compute::acr_boot::BootConfig {
        pde_upper: true,
        acr_vram_pte: false,
        blob_size_zero: false,
        bind_vram: false,
        imem_preload: false,
        tlb_invalidate: true,
    };
    eprintln!("  Config: {}", cfg_e.label());

    let result_e = coral_driver::nv::vfio_compute::acr_boot::attempt_sysmem_acr_boot_with_config(
        &bar0, &fw, container, &cfg_e,
    );
    eprintln!("  Strategy: {}", result_e.strategy);
    eprintln!("  Success: {}", result_e.success);
    for note in &result_e.notes {
        eprintln!("  | {note}");
    }

    eprintln!("\n── E2: Post-ACR State ──");
    falcon_state(&bar0, "SEC2", freg115::SEC2_BASE);
    falcon_state(&bar0, "FECS", freg115::FECS_BASE);
    falcon_state(&bar0, "GPCCS", freg115::GPCCS_BASE);

    let sec2_pc = bar0.read_u32(freg115::SEC2_BASE + freg115::PC).unwrap_or(0);
    let sec2_cpuctl = bar0
        .read_u32(freg115::SEC2_BASE + freg115::CPUCTL)
        .unwrap_or(0);
    let sec2_alive = sec2_pc > 0x100
        && sec2_cpuctl & freg115::CPUCTL_HALTED == 0
        && sec2_cpuctl & freg115::CPUCTL_STOPPED == 0;

    if !sec2_alive {
        eprintln!("  SEC2 not in idle loop (PC={sec2_pc:#x}). Proceeding anyway...");
    }

    eprintln!("\n── E3: BOOTSTRAP_FALCON (FECS+GPCCS) via mailbox ──");
    let bootvec = coral_driver::nv::vfio_compute::acr_boot::FalconBootvecOffsets {
        gpccs: fw.gpccs_bl.bl_imem_off(),
        fecs: fw.fecs_bl.bl_imem_off(),
    };
    eprintln!(
        "  BOOTVEC: GPCCS={:#06x} FECS={:#06x}",
        bootvec.gpccs, bootvec.fecs
    );

    let mailbox_e =
        coral_driver::nv::vfio_compute::acr_boot::attempt_acr_mailbox_command(&bar0, &bootvec);
    eprintln!("  Mailbox strategy: {}", mailbox_e.strategy);
    eprintln!("  Mailbox success: {}", mailbox_e.success);
    for note in &mailbox_e.notes {
        eprintln!("  | {note}");
    }

    eprintln!("\n── E4: Final State ──");
    falcon_state(&bar0, "SEC2", freg115::SEC2_BASE);
    falcon_state(&bar0, "FECS", freg115::FECS_BASE);
    falcon_state(&bar0, "GPCCS", freg115::GPCCS_BASE);

    let fecs_e_cpuctl = bar0
        .read_u32(freg115::FECS_BASE + freg115::CPUCTL)
        .unwrap_or(0xDEAD);
    let fecs_e_pc = bar0.read_u32(freg115::FECS_BASE + freg115::PC).unwrap_or(0);
    let fecs_e_exci = bar0
        .read_u32(freg115::FECS_BASE + freg115::EXCI)
        .unwrap_or(0);
    let gpccs_e_cpuctl = bar0
        .read_u32(freg115::GPCCS_BASE + freg115::CPUCTL)
        .unwrap_or(0xDEAD);
    let gpccs_e_pc = bar0
        .read_u32(freg115::GPCCS_BASE + freg115::PC)
        .unwrap_or(0);
    let gpccs_e_exci = bar0
        .read_u32(freg115::GPCCS_BASE + freg115::EXCI)
        .unwrap_or(0);

    let fecs_e_running = fecs_e_cpuctl & (freg115::CPUCTL_HALTED | freg115::CPUCTL_STOPPED) == 0;
    let gpccs_e_running = gpccs_e_cpuctl & (freg115::CPUCTL_HALTED | freg115::CPUCTL_STOPPED) == 0;
    let fecs_e_mthd = bar0
        .read_u32(freg115::FECS_BASE + freg115::MTHD_STATUS)
        .unwrap_or(0);

    // ══════════════════════════════════════════════════════════════
    // SUMMARY
    // ══════════════════════════════════════════════════════════════
    let sep = "=".repeat(70);
    eprintln!("\n{sep}");
    eprintln!("  Exp 115 RESULTS");
    eprintln!("{sep}");
    eprintln!(
        "  A (BOOTVEC=0):   GPCCS alive={gpccs_a_ok} pc={gpccs_a_pc:#06x} exci={gpccs_a_exci:#010x} | \
         FECS alive={fecs_a_ok} pc={fecs_a_pc:#06x} exci={fecs_a_exci:#010x}"
    );
    eprintln!(
        "  B (BOOTVEC=BL):  GPCCS alive={gpccs_b_ok} pc={gpccs_b_pc:#06x} exci={gpccs_b_exci:#010x} | \
         FECS alive={fecs_b_ok} pc={fecs_b_pc:#06x} exci={fecs_b_exci:#010x}"
    );
    eprintln!("  C (existing fn): success={}", result_c.success);
    eprintln!(
        "  E (LS+no_vram_pte): GPCCS running={gpccs_e_running} pc={gpccs_e_pc:#06x} exci={gpccs_e_exci:#010x} | \
         FECS running={fecs_e_running} pc={fecs_e_pc:#06x} exci={fecs_e_exci:#010x}"
    );
    eprintln!("  E FECS MTHD_STATUS: {fecs_e_mthd:#010x}");
    eprintln!("{sep}");

    if gpccs_e_running && gpccs_e_exci == 0 {
        eprintln!("\n  *** GPCCS IS ALIVE via LS ACR! ***");
    }
    if fecs_e_running && fecs_e_exci == 0 {
        eprintln!("  *** FECS IS ALIVE via LS ACR! ***");
    }
    if fecs_e_running && gpccs_e_running {
        eprintln!("  *** BOTH FECS AND GPCCS ALIVE — L10 SOLVED! ***");
        eprintln!("  Next: L11 — GR context init + shader dispatch");
    }
    if fecs_e_mthd & 1 != 0 {
        eprintln!("  *** FECS MTHD_STATUS READY — GR engine accepts commands! ***");
    }

    eprintln!("\n=== Exp 115 Complete ===");
}
