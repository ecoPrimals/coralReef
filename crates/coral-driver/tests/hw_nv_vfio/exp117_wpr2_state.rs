// SPDX-License-Identifier: AGPL-3.0-only

//! Exp 117: WPR2 State Tracking Across Driver Swap
//!
//! KEY QUESTION: Is WPR2 valid while nouveau is running, and when is it lost?
//!
//! Exp 116 showed WPR2 boundaries are INVALID (start > end) after nouveau → vfio
//! swap. But we never checked WPR2 *during* nouveau's active session. If WPR2 is
//! valid while nouveau is running, the ACR firmware has a working WPR2 to reference.
//! We need to understand when this state is lost and how to preserve it.
//!
//! This experiment opens BAR0 via sysfs `resource0` while nouveau is bound (before
//! the swap to vfio-pci), reads the critical GPU state, then compares after swap.
//!
//! Variants:
//!   A: Read WPR2 + falcon + FB state while nouveau is ACTIVE
//!   B: Read same state after vfio-pci swap (baseline comparison)
//!   C: If SEC2 is still alive post-swap, send BOOTSTRAP_FALCON directly
//!   D: ACR boot using WPR2 addresses from Phase A (if valid)
//!
//! Run:
//! ```sh
//! cargo test -p coral-driver --features vfio --test hw_nv_vfio \
//!   exp117 -- --ignored --nocapture --test-threads=1
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

mod regs {
    pub const SEC2_BASE: u32 = 0x087000;
    pub const FECS_BASE: u32 = 0x409000;
    pub const GPCCS_BASE: u32 = 0x41a000;

    pub const CPUCTL: u32 = 0x100;
    pub const SCTL: u32 = 0x240;
    pub const PC: u32 = 0x030;
    pub const EXCI: u32 = 0x148;
    pub const MAILBOX0: u32 = 0x040;
    pub const MAILBOX1: u32 = 0x044;
    pub const _BOOTVEC: u32 = 0x104;

    pub const CPUCTL_HRESET: u32 = 1 << 4;
    pub const CPUCTL_HALTED: u32 = 1 << 5;

    pub const PFB_WPR2_BEG: u32 = 0x100CEC;
    pub const PFB_WPR2_END: u32 = 0x100CF0;
    pub const PFB_WPR1_BEG: u32 = 0x100CE8;
    pub const PFB_WPR1_END: u32 = 0x100CEC;

    pub const INDEXED_WPR: u32 = 0x100CD4;
    pub const BAR0_WINDOW: u32 = 0x001700;
    pub const NV_PMC_BOOT_0: u32 = 0x000000;
    pub const NV_PMC_ENABLE: u32 = 0x000200;
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

/// Snapshot of critical GPU state at a point in time.
#[derive(Debug)]
struct GpuSnapshot {
    label: String,
    boot_id: u32,
    pmc_enable: u32,
    bar0_window: u32,

    sec2_cpuctl: u32,
    sec2_sctl: u32,
    sec2_pc: u32,
    sec2_exci: u32,
    sec2_mb0: u32,
    sec2_mb1: u32,

    fecs_cpuctl: u32,
    fecs_sctl: u32,
    fecs_pc: u32,
    fecs_exci: u32,

    gpccs_cpuctl: u32,
    gpccs_sctl: u32,
    gpccs_pc: u32,
    gpccs_exci: u32,

    wpr2_idx_start_raw: u32,
    wpr2_idx_end_raw: u32,
    wpr2_direct_beg: u32,
    wpr2_direct_end: u32,
    wpr1_direct_beg: u32,
    wpr1_direct_end: u32,
}

impl GpuSnapshot {
    fn wpr2_indexed_start(&self) -> u64 {
        ((self.wpr2_idx_start_raw as u64) & 0xFFFF_FF00) << 8
    }

    fn wpr2_indexed_end(&self) -> u64 {
        (((self.wpr2_idx_end_raw as u64) & 0xFFFF_FF00) << 8) + 0x20000
    }

    fn wpr2_indexed_valid(&self) -> bool {
        let s = self.wpr2_indexed_start();
        let e = self.wpr2_indexed_end();
        s > 0 && e > s && (e - s) > 0x1000
    }

    fn falcon_desc(&self, name: &str, cpuctl: u32, sctl: u32, pc: u32, exci: u32) -> String {
        let hreset = cpuctl & regs::CPUCTL_HRESET != 0;
        let halted = cpuctl & regs::CPUCTL_HALTED != 0;
        let running = !hreset && !halted && cpuctl != 0xDEAD;
        format!(
            "{name:6}: cpuctl={cpuctl:#010x} HRESET={hreset:<5} HALTED={halted:<5} \
             RUNNING={running:<5} SCTL={sctl:#06x} PC={pc:#06x} EXCI={exci:#010x}"
        )
    }

    fn print(&self) {
        let dash = "-".repeat(60);
        eprintln!("  {dash}");
        eprintln!("  Snapshot: {}", self.label);
        eprintln!("  {dash}");
        eprintln!(
            "  BOOT_0={:#010x}  PMC_ENABLE={:#010x}  BAR0_WIN={:#010x}",
            self.boot_id, self.pmc_enable, self.bar0_window
        );
        eprintln!();
        eprintln!("  Falcons:");
        eprintln!(
            "  {}",
            self.falcon_desc(
                "SEC2",
                self.sec2_cpuctl,
                self.sec2_sctl,
                self.sec2_pc,
                self.sec2_exci
            )
        );
        eprintln!(
            "    MB0={:#010x}  MB1={:#010x}",
            self.sec2_mb0, self.sec2_mb1
        );
        eprintln!(
            "  {}",
            self.falcon_desc(
                "FECS",
                self.fecs_cpuctl,
                self.fecs_sctl,
                self.fecs_pc,
                self.fecs_exci
            )
        );
        eprintln!(
            "  {}",
            self.falcon_desc(
                "GPCCS",
                self.gpccs_cpuctl,
                self.gpccs_sctl,
                self.gpccs_pc,
                self.gpccs_exci
            )
        );
        eprintln!();
        eprintln!("  WPR registers:");
        eprintln!(
            "    Indexed (0x100CD4): start_raw={:#010x} end_raw={:#010x}",
            self.wpr2_idx_start_raw, self.wpr2_idx_end_raw
        );
        eprintln!(
            "    Indexed decoded:    start={:#010x} end={:#010x} valid={}",
            self.wpr2_indexed_start(),
            self.wpr2_indexed_end(),
            self.wpr2_indexed_valid()
        );
        eprintln!(
            "    Direct  (CEC/CF0):  beg={:#010x} end={:#010x}",
            self.wpr2_direct_beg, self.wpr2_direct_end
        );
        eprintln!(
            "    WPR1    (CE8/CEC):  beg={:#010x} end={:#010x}",
            self.wpr1_direct_beg, self.wpr1_direct_end
        );
        eprintln!("  {dash}");
    }
}

/// Capture snapshot from Bar0Access (sysfs RW mmap — works while nouveau is bound).
fn snapshot_bar0access(bar0: &mut Bar0Access, label: &str) -> GpuSnapshot {
    let rd = |b: &Bar0Access, off: u32| b.read_u32(off).unwrap_or(0xDEAD_DEAD);

    let sec2 = regs::SEC2_BASE;
    let fecs = regs::FECS_BASE;
    let gpccs = regs::GPCCS_BASE;

    let boot_id = rd(bar0, regs::NV_PMC_BOOT_0);
    let pmc_enable = rd(bar0, regs::NV_PMC_ENABLE);
    let bar0_window = rd(bar0, regs::BAR0_WINDOW);

    let sec2_cpuctl = rd(bar0, sec2 + regs::CPUCTL);
    let sec2_sctl = rd(bar0, sec2 + regs::SCTL);
    let sec2_pc = rd(bar0, sec2 + regs::PC);
    let sec2_exci = rd(bar0, sec2 + regs::EXCI);
    let sec2_mb0 = rd(bar0, sec2 + regs::MAILBOX0);
    let sec2_mb1 = rd(bar0, sec2 + regs::MAILBOX1);

    let fecs_cpuctl = rd(bar0, fecs + regs::CPUCTL);
    let fecs_sctl = rd(bar0, fecs + regs::SCTL);
    let fecs_pc = rd(bar0, fecs + regs::PC);
    let fecs_exci = rd(bar0, fecs + regs::EXCI);

    let gpccs_cpuctl = rd(bar0, gpccs + regs::CPUCTL);
    let gpccs_sctl = rd(bar0, gpccs + regs::SCTL);
    let gpccs_pc = rd(bar0, gpccs + regs::PC);
    let gpccs_exci = rd(bar0, gpccs + regs::EXCI);

    let wpr2_direct_beg = rd(bar0, regs::PFB_WPR2_BEG);
    let wpr2_direct_end = rd(bar0, regs::PFB_WPR2_END);
    let wpr1_direct_beg = rd(bar0, regs::PFB_WPR1_BEG);
    let wpr1_direct_end = rd(bar0, regs::PFB_WPR1_END);

    // Indexed WPR2 read requires write then read on the same register
    let _ = bar0.write_u32(regs::INDEXED_WPR, 2);
    let wpr2_idx_start_raw = rd(bar0, regs::INDEXED_WPR);
    let _ = bar0.write_u32(regs::INDEXED_WPR, 3);
    let wpr2_idx_end_raw = rd(bar0, regs::INDEXED_WPR);

    GpuSnapshot {
        label: label.to_string(),
        boot_id,
        pmc_enable,
        bar0_window,
        sec2_cpuctl,
        sec2_sctl,
        sec2_pc,
        sec2_exci,
        sec2_mb0,
        sec2_mb1,
        fecs_cpuctl,
        fecs_sctl,
        fecs_pc,
        fecs_exci,
        gpccs_cpuctl,
        gpccs_sctl,
        gpccs_pc,
        gpccs_exci,
        wpr2_idx_start_raw,
        wpr2_idx_end_raw,
        wpr2_direct_beg,
        wpr2_direct_end,
        wpr1_direct_beg,
        wpr1_direct_end,
    }
}

/// Capture snapshot from VFIO MappedBar (after swap).
fn snapshot_vfio(bar0: &MappedBar, label: &str) -> GpuSnapshot {
    let r = |off: usize| bar0.read_u32(off).unwrap_or(0xDEAD_DEAD);

    let boot_id = r(regs::NV_PMC_BOOT_0 as usize);
    let pmc_enable = r(regs::NV_PMC_ENABLE as usize);
    let bar0_window = r(regs::BAR0_WINDOW as usize);

    let sec2 = regs::SEC2_BASE as usize;
    let fecs = regs::FECS_BASE as usize;
    let gpccs = regs::GPCCS_BASE as usize;

    let _ = bar0.write_u32(regs::INDEXED_WPR as usize, 2);
    let wpr2_idx_start_raw = r(regs::INDEXED_WPR as usize);
    let _ = bar0.write_u32(regs::INDEXED_WPR as usize, 3);
    let wpr2_idx_end_raw = r(regs::INDEXED_WPR as usize);

    GpuSnapshot {
        label: label.to_string(),
        boot_id,
        pmc_enable,
        bar0_window,
        sec2_cpuctl: r(sec2 + regs::CPUCTL as usize),
        sec2_sctl: r(sec2 + regs::SCTL as usize),
        sec2_pc: r(sec2 + regs::PC as usize),
        sec2_exci: r(sec2 + regs::EXCI as usize),
        sec2_mb0: r(sec2 + regs::MAILBOX0 as usize),
        sec2_mb1: r(sec2 + regs::MAILBOX1 as usize),
        fecs_cpuctl: r(fecs + regs::CPUCTL as usize),
        fecs_sctl: r(fecs + regs::SCTL as usize),
        fecs_pc: r(fecs + regs::PC as usize),
        fecs_exci: r(fecs + regs::EXCI as usize),
        gpccs_cpuctl: r(gpccs + regs::CPUCTL as usize),
        gpccs_sctl: r(gpccs + regs::SCTL as usize),
        gpccs_pc: r(gpccs + regs::PC as usize),
        gpccs_exci: r(gpccs + regs::EXCI as usize),
        wpr2_idx_start_raw,
        wpr2_idx_end_raw,
        wpr2_direct_beg: r(regs::PFB_WPR2_BEG as usize),
        wpr2_direct_end: r(regs::PFB_WPR2_END as usize),
        wpr1_direct_beg: r(regs::PFB_WPR1_BEG as usize),
        wpr1_direct_end: r(regs::PFB_WPR1_END as usize),
    }
}

fn compare_snapshots(a: &GpuSnapshot, b: &GpuSnapshot) {
    let eq = "=".repeat(60);
    eprintln!("\n  {eq}");
    eprintln!("  COMPARISON: '{}' vs '{}'", a.label, b.label);
    eprintln!("  {eq}");

    let mut diffs = 0u32;
    macro_rules! cmp {
        ($field:ident, $fmt:literal) => {
            if a.$field != b.$field {
                diffs += 1;
                eprintln!(
                    concat!("  DIFF {:20}: ", $fmt, " → ", $fmt),
                    stringify!($field),
                    a.$field,
                    b.$field
                );
            }
        };
    }

    cmp!(boot_id, "{:#010x}");
    cmp!(pmc_enable, "{:#010x}");
    cmp!(bar0_window, "{:#010x}");
    cmp!(sec2_cpuctl, "{:#010x}");
    cmp!(sec2_sctl, "{:#06x}");
    cmp!(sec2_pc, "{:#06x}");
    cmp!(sec2_exci, "{:#010x}");
    cmp!(sec2_mb0, "{:#010x}");
    cmp!(sec2_mb1, "{:#010x}");
    cmp!(fecs_cpuctl, "{:#010x}");
    cmp!(fecs_sctl, "{:#06x}");
    cmp!(fecs_pc, "{:#06x}");
    cmp!(gpccs_cpuctl, "{:#010x}");
    cmp!(gpccs_sctl, "{:#06x}");
    cmp!(gpccs_pc, "{:#06x}");
    cmp!(wpr2_idx_start_raw, "{:#010x}");
    cmp!(wpr2_idx_end_raw, "{:#010x}");
    cmp!(wpr2_direct_beg, "{:#010x}");
    cmp!(wpr2_direct_end, "{:#010x}");
    cmp!(wpr1_direct_beg, "{:#010x}");
    cmp!(wpr1_direct_end, "{:#010x}");

    if diffs == 0 {
        eprintln!("  NO DIFFERENCES — state preserved across swap");
    } else {
        eprintln!("  {diffs} registers changed across swap");
    }
    eprintln!("  {eq}");
}

/// Dump WPR headers via PRAMIN at given VRAM base using VFIO bar0.
fn dump_wpr_headers(bar0: &MappedBar, vram_base: u64, label: &str) {
    eprintln!("  WPR headers at {vram_base:#x} ({label}):");
    let base32 = vram_base as u32;
    match PraminRegion::new(bar0, base32, 264) {
        Ok(rgn) => {
            for i in 0..11 {
                let off = i * 24;
                let falcon_id = rgn.read_u32(off).unwrap_or(0xDEAD);
                if falcon_id == 0xFFFF_FFFF || falcon_id == 0xDEAD_DEAD {
                    break;
                }
                let lsb_off = rgn.read_u32(off + 4).unwrap_or(0);
                let status = rgn.read_u32(off + 20).unwrap_or(0);
                let status_str = match status {
                    0 => "NONE",
                    1 => "COPY",
                    2 => "VCODE_FAIL",
                    3 => "VDATA_FAIL",
                    4 => "VALID_DONE",
                    5 => "VALID_SKIP",
                    6 => "BOOT_READY",
                    7 => "REVOKE_FAIL",
                    _ => "???",
                };
                let fname = match falcon_id {
                    0 => "PMU",
                    2 => "FECS",
                    3 => "GPCCS",
                    7 => "SEC2",
                    _ => "???",
                };
                eprintln!(
                    "    [{i}] falcon={falcon_id}({fname}) lsb={lsb_off:#x} status={status}({status_str})"
                );
            }
        }
        Err(e) => eprintln!("    PRAMIN failed: {e}"),
    }
}

/// Read 64 bytes from PRAMIN and hex-dump them.
fn pramin_hexdump(bar0: &MappedBar, vram_addr: u32, label: &str) {
    eprintln!("  PRAMIN hexdump at {vram_addr:#010x} ({label}):");
    match PraminRegion::new(bar0, vram_addr, 64) {
        Ok(rgn) => {
            for row in 0..4u32 {
                let mut hex = String::new();
                for col in 0..4u32 {
                    let off = (row * 16 + col * 4) as usize;
                    let val = rgn.read_u32(off).unwrap_or(0xDEAD);
                    hex.push_str(&format!("{val:08x} "));
                }
                eprintln!("    {:#010x}: {hex}", vram_addr + row * 16);
            }
        }
        Err(e) => eprintln!("    PRAMIN failed: {e}"),
    }
}

#[test]
#[ignore = "requires VFIO-bound GPU hardware + GlowPlug + Ember"]
fn exp117_wpr2_state_tracking() {
    init_tracing();

    let banner = "#".repeat(70);
    eprintln!("\n{banner}");
    eprintln!("#  Exp 117: WPR2 State Tracking Across Driver Swap                 #");
    eprintln!("#  Question: Is WPR2 valid while nouveau is running?                #");
    eprintln!("{banner}\n");

    let bdf = discover_bdf();
    eprintln!("Target BDF: {bdf}");

    // ══════════════════════════════════════════════════════════════════════
    // PHASE A: Read GPU state while nouveau is ACTIVE
    // ══════════════════════════════════════════════════════════════════════
    let eq = "=".repeat(70);
    eprintln!("\n{eq}");
    eprintln!("  PHASE A: GPU state while NOUVEAU is active");
    eprintln!("{eq}");

    let mut gp = GlowPlugClient::connect().expect("GlowPlug connection");
    eprintln!("\n── A1: Swap to nouveau ──");
    gp.swap(&bdf, "nouveau").expect("swap→nouveau");
    std::thread::sleep(std::time::Duration::from_secs(4));

    eprintln!("\n── A2: Open BAR0 via sysfs while nouveau is bound ──");
    let sysfs_dev = format!("/sys/bus/pci/devices/{bdf}");
    let mut bar0_sysfs = match Bar0Access::from_sysfs_device(&sysfs_dev) {
        Ok(b) => {
            eprintln!("  BAR0 sysfs open: OK ({} MiB)", b.size() / (1024 * 1024));
            b
        }
        Err(e) => {
            eprintln!("  BAR0 sysfs open FAILED: {e}");
            eprintln!("  Cannot read state while nouveau is active — aborting Phase A");
            gp.swap(&bdf, "vfio-pci").expect("swap→vfio-pci");
            std::thread::sleep(std::time::Duration::from_millis(500));
            panic!("Phase A requires BAR0 access while nouveau is bound");
        }
    };

    eprintln!("\n── A3: Capture nouveau-active snapshot ──");
    let snap_nouveau = snapshot_bar0access(&mut bar0_sysfs, "nouveau-active");
    snap_nouveau.print();

    eprintln!("\n── A4: WPR2 Analysis ──");
    if snap_nouveau.wpr2_indexed_valid() {
        let start = snap_nouveau.wpr2_indexed_start();
        let end = snap_nouveau.wpr2_indexed_end();
        let size = end - start;
        eprintln!("  *** WPR2 IS VALID during nouveau session ***");
        eprintln!(
            "  WPR2 range: {start:#x}..{end:#x} ({size:#x} = {} KiB)",
            size / 1024
        );
    } else {
        eprintln!("  WPR2 is INVALID during nouveau session too");
        eprintln!("  This means FWSEC did not set up WPR2 (or different register layout)");
    }

    // Read PRAMIN base to understand where nouveau's instance memory is
    let pramin_base_raw = bar0_sysfs.read_u32(regs::BAR0_WINDOW).unwrap_or(0);
    let pramin_vram_base = (pramin_base_raw as u64) << 16;
    eprintln!("\n── A5: PRAMIN window info ──");
    eprintln!("  BAR0_WINDOW raw={pramin_base_raw:#010x} → VRAM base={pramin_vram_base:#010x}");

    // Drop sysfs BAR0 before driver swap
    drop(bar0_sysfs);

    // ══════════════════════════════════════════════════════════════════════
    // PHASE B: Read GPU state after vfio-pci swap (baseline)
    // ══════════════════════════════════════════════════════════════════════
    eprintln!("\n{eq}");
    eprintln!("  PHASE B: GPU state AFTER swap to vfio-pci");
    eprintln!("{eq}");

    eprintln!("\n── B1: Swap to vfio-pci ──");
    gp.swap(&bdf, "vfio-pci").expect("swap→vfio-pci");
    std::thread::sleep(std::time::Duration::from_millis(500));

    let fds = ember_client::request_fds(&bdf).expect("ember fds");
    let vfio_dev = coral_driver::vfio::VfioDevice::from_received(&bdf, fds).expect("VfioDevice");
    let bar0 = vfio_dev.map_bar(0).expect("map_bar(0)");

    eprintln!("\n── B2: Capture post-swap snapshot ──");
    let snap_vfio = snapshot_vfio(&bar0, "vfio-post-swap");
    snap_vfio.print();

    // ══════════════════════════════════════════════════════════════════════
    // COMPARISON
    // ══════════════════════════════════════════════════════════════════════
    compare_snapshots(&snap_nouveau, &snap_vfio);

    // ══════════════════════════════════════════════════════════════════════
    // PHASE B3: Dump VRAM content at key addresses
    // ══════════════════════════════════════════════════════════════════════
    eprintln!("\n── B3: VRAM content at key addresses ──");
    pramin_hexdump(&bar0, 0x0000_0000, "VRAM start");
    pramin_hexdump(&bar0, 0x0007_0000, "our WPR mirror (0x70000)");
    pramin_hexdump(&bar0, 0x0006_0000, "our shadow mirror (0x60000)");

    if snap_nouveau.wpr2_indexed_valid() {
        let w2s = snap_nouveau.wpr2_indexed_start();
        pramin_hexdump(&bar0, w2s as u32, "WPR2 start from nouveau");
        dump_wpr_headers(&bar0, w2s, "nouveau WPR2 start");
    }

    // Also read the PRAMIN area nouveau was using
    if pramin_vram_base > 0 && pramin_vram_base < 0x1_0000_0000 {
        pramin_hexdump(&bar0, pramin_vram_base as u32, "nouveau PRAMIN base");
    }

    // ══════════════════════════════════════════════════════════════════════
    // PHASE C: If SEC2 survived the swap, try BOOTSTRAP_FALCON directly
    // ══════════════════════════════════════════════════════════════════════
    let sec2_alive_post_swap = {
        let cpuctl = snap_vfio.sec2_cpuctl;
        cpuctl & regs::CPUCTL_HRESET == 0
            && cpuctl & regs::CPUCTL_HALTED == 0
            && cpuctl != 0xDEAD_DEAD
            && snap_vfio.sec2_pc > 0x100
    };

    eprintln!("\n{eq}");
    eprintln!("  PHASE C: SEC2 survival + direct BOOTSTRAP");
    eprintln!("{eq}");
    eprintln!("  SEC2 alive after swap: {sec2_alive_post_swap}");

    if sec2_alive_post_swap {
        eprintln!("\n  *** SEC2 SURVIVED the driver swap! ***");
        eprintln!("  Attempting BOOTSTRAP_FALCON on nouveau's running SEC2...\n");

        let fw = AcrFirmwareSet::load("gv100").expect("firmware load");
        let bootvec = FalconBootvecOffsets {
            gpccs: fw.gpccs_bl.bl_imem_off(),
            fecs: fw.fecs_bl.bl_imem_off(),
        };
        eprintln!(
            "  BOOTVEC offsets: GPCCS={:#06x} FECS={:#06x}",
            bootvec.gpccs, bootvec.fecs
        );

        let mailbox_result = attempt_acr_mailbox_command(&bar0, &bootvec);
        eprintln!("  Mailbox strategy: {}", mailbox_result.strategy);
        eprintln!("  Mailbox success: {}", mailbox_result.success);
        for note in &mailbox_result.notes {
            eprintln!("  | {note}");
        }

        eprintln!("\n── C2: Post-BOOTSTRAP state ──");
        let snap_c = snapshot_vfio(&bar0, "post-bootstrap-on-surviving-sec2");
        snap_c.print();

        let fecs_running = snap_c.fecs_cpuctl & (regs::CPUCTL_HRESET | regs::CPUCTL_HALTED) == 0
            && snap_c.fecs_cpuctl != 0xDEAD_DEAD;
        let gpccs_running = snap_c.gpccs_cpuctl & (regs::CPUCTL_HRESET | regs::CPUCTL_HALTED) == 0
            && snap_c.gpccs_cpuctl != 0xDEAD_DEAD;

        if fecs_running && gpccs_running {
            eprintln!("\n  *** FECS AND GPCCS ALIVE via surviving SEC2! ***");
            eprintln!("  *** THIS IS THE BREAKTHROUGH — WPR was already loaded by nouveau ***");
        } else if fecs_running || gpccs_running {
            eprintln!("\n  *** PARTIAL: FECS={fecs_running} GPCCS={gpccs_running} ***");
        } else {
            eprintln!("\n  SEC2 survived but BOOTSTRAP_FALCON did not activate FECS/GPCCS");
            eprintln!("  Possible: SEC2 in wrong state, queue not set up, or WPR already consumed");
        }
    } else {
        eprintln!("  SEC2 did NOT survive the swap — cannot attempt direct BOOTSTRAP");
    }

    // ══════════════════════════════════════════════════════════════════════
    // PHASE D: ACR boot using WPR2 addresses from nouveau session
    // ══════════════════════════════════════════════════════════════════════
    let do_phase_d = snap_nouveau.wpr2_indexed_valid();

    if do_phase_d {
        eprintln!("\n{eq}");
        eprintln!("  PHASE D: ACR boot with nouveau's WPR2 addresses");
        eprintln!("{eq}");

        let w2s = snap_nouveau.wpr2_indexed_start();
        let w2e = snap_nouveau.wpr2_indexed_end();
        eprintln!("  Using WPR2 from nouveau: {w2s:#x}..{w2e:#x}");

        // Fresh nouveau cycle to re-establish clean state
        eprintln!("\n── D0: Fresh nouveau cycle ──");
        drop(bar0);
        drop(vfio_dev);
        gp.swap(&bdf, "nouveau").expect("swap→nouveau");
        std::thread::sleep(std::time::Duration::from_secs(4));
        gp.swap(&bdf, "vfio-pci").expect("swap→vfio-pci");
        std::thread::sleep(std::time::Duration::from_millis(500));

        let fds = ember_client::request_fds(&bdf).expect("ember fds");
        let vfio_dev =
            coral_driver::vfio::VfioDevice::from_received(&bdf, fds).expect("VfioDevice");
        let bar0 = vfio_dev.map_bar(0).expect("map_bar(0)");
        let container = vfio_dev.dma_backend();

        // Build WPR at nouveau's WPR2 addresses
        let fw = AcrFirmwareSet::load("gv100").expect("firmware load");
        let wpr_data = coral_driver::nv::vfio_compute::acr_boot::build_wpr(&fw, w2s);
        eprintln!("  WPR blob: {} bytes at base {w2s:#x}", wpr_data.len());

        // Mirror to VRAM at WPR2 via PRAMIN
        eprintln!("\n── D1: Write WPR to VRAM at WPR2 address ──");
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
        eprintln!("  VRAM mirror at WPR2: {mirror_ok}");

        // Verify
        dump_wpr_headers(&bar0, w2s, "our WPR at WPR2 address");

        // ACR boot with blob_size=0
        eprintln!("\n── D2: ACR boot (blob_size=0, correct PDEs) ──");
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

        // BOOTSTRAP
        eprintln!("\n── D3: BOOTSTRAP_FALCON ──");
        let bootvec = FalconBootvecOffsets {
            gpccs: fw.gpccs_bl.bl_imem_off(),
            fecs: fw.fecs_bl.bl_imem_off(),
        };
        let mb = attempt_acr_mailbox_command(&bar0, &bootvec);
        eprintln!("  Mailbox: success={}", mb.success);
        for note in &mb.notes {
            eprintln!("  | {note}");
        }

        eprintln!("\n── D4: Final state ──");
        let snap_d = snapshot_vfio(&bar0, "phase-d-final");
        snap_d.print();

        dump_wpr_headers(&bar0, w2s, "WPR2 after ACR boot");
        dump_wpr_headers(&bar0, 0x70000, "0x70000 after ACR boot");
    } else {
        eprintln!("\n  PHASE D: SKIPPED — WPR2 was not valid during nouveau session");
        eprintln!("  This means FWSEC may not set WPR2 on GV100, or we're reading wrong registers");
    }

    // ══════════════════════════════════════════════════════════════════════
    // SUMMARY
    // ══════════════════════════════════════════════════════════════════════
    let sep = "=".repeat(70);
    eprintln!("\n{sep}");
    eprintln!("  Exp 117 RESULTS SUMMARY");
    eprintln!("{sep}");
    eprintln!(
        "  WPR2 valid during nouveau: {}",
        snap_nouveau.wpr2_indexed_valid()
    );
    if snap_nouveau.wpr2_indexed_valid() {
        eprintln!(
            "  WPR2 range: {:#x}..{:#x}",
            snap_nouveau.wpr2_indexed_start(),
            snap_nouveau.wpr2_indexed_end()
        );
    }
    eprintln!(
        "  WPR2 valid after swap:     {}",
        snap_vfio.wpr2_indexed_valid()
    );
    eprintln!("  SEC2 alive after swap:     {sec2_alive_post_swap}");
    eprintln!(
        "  SEC2 SCTL nouveau:         {:#06x}",
        snap_nouveau.sec2_sctl
    );
    eprintln!("  SEC2 SCTL post-swap:       {:#06x}", snap_vfio.sec2_sctl);
    eprintln!(
        "  FECS running (nouveau):    {}",
        snap_nouveau.fecs_cpuctl & (regs::CPUCTL_HRESET | regs::CPUCTL_HALTED) == 0
            && snap_nouveau.fecs_cpuctl != 0xDEAD_DEAD
    );
    eprintln!(
        "  GPCCS running (nouveau):   {}",
        snap_nouveau.gpccs_cpuctl & (regs::CPUCTL_HRESET | regs::CPUCTL_HALTED) == 0
            && snap_nouveau.gpccs_cpuctl != 0xDEAD_DEAD
    );
    eprintln!("{sep}");

    eprintln!("\n=== Exp 117 Complete ===");
}
