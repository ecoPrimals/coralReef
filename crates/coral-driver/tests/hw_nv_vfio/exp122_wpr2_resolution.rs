// SPDX-License-Identifier: AGPL-3.0-or-later

//! Exp 122: Systematic WPR2 Resolution — Three-Pronged Attack
//!
//! The persistent WPR copy stall (FECS=1, GPCCS=1) across Exp 114-121 proves
//! that ACR firmware starts but cannot write authenticated images to WPR2 VRAM
//! because WPR2 boundaries are INVALID in every scenario we can control.
//!
//! Three parallel approaches:
//!
//! **A: WPR2 Register Write Probe** — Can we directly set WPR2 boundaries
//!    from the host? Tests every known WPR2-related register for writability.
//!
//! **B: Parasitic Nouveau Mode** — While nouveau is active (WPR2 valid, FECS
//!    alive), use sysfs BAR0 to read WPR state and interact with running falcons.
//!
//! **C: FWSEC Binary Extraction** — Read the FWSEC firmware from VBIOS PROM,
//!    analyze its structure, identify the WPR2 carving mechanism.
//!
//! Run all:
//! ```sh
//! CORALREEF_VFIO_BDF=0000:03:00.0 cargo test -p coral-driver \
//!   --features vfio --test hw_nv_vfio exp122 -- --ignored --nocapture --test-threads=1
//! ```

use crate::helpers::init_tracing;
use coral_driver::vfio::device::{MappedBar, VfioDevice};

const _PRAMIN_MMIO_BASE: usize = 0x0070_0000;
const _BAR0_WINDOW_REG: usize = 0x0000_1700;

mod wpr_regs {
    pub const INDEXED_WPR: usize = 0x100CD4;

    pub const PFB_WPR2_BEG: usize = 0x100CEC;
    pub const PFB_WPR2_END: usize = 0x100CF0;

    pub const FBPA_WPR2_ADDR_LO: usize = 0x1FA824;
    pub const FBPA_WPR2_ADDR_HI: usize = 0x1FA828;

    pub const SEC2_BASE: usize = 0x087000;
    pub const FECS_BASE: usize = 0x409000;
    pub const GPCCS_BASE: usize = 0x41a000;
    pub const PMU_BASE: usize = 0x10a000;

    pub const CPUCTL: usize = 0x100;
    pub const SCTL: usize = 0x240;
    pub const PC: usize = 0x030;
    pub const EXCI: usize = 0x148;
    pub const MAILBOX0: usize = 0x040;
    pub const _MAILBOX1: usize = 0x044;
    pub const _BOOTVEC: usize = 0x104;

    pub const _PMC_ENABLE: usize = 0x000200;
    pub const PRIV_RING_INTR_STATUS: usize = 0x120058;
    pub const PRIV_RING_COMMAND: usize = 0x12004C;
    pub const PRIV_RING_CMD_ACK: u32 = 0x02;

    pub const _PROM_BASE: usize = 0x0030_0000;
    pub const _PROM_ENABLE_REG: usize = 0x0000_1854;

    // Additional WPR-adjacent registers to probe
    pub const _PFB_MMU_CTRL: usize = 0x100C80;
    pub const _PFB_MMU_INVALIDATE_PDB: usize = 0x100CB8;
    pub const PFB_WPR_CFG: usize = 0x100CD0;
}

fn falcon_state(bar0: &MappedBar, name: &str, base: usize) -> String {
    let cpuctl = bar0.read_u32(base + wpr_regs::CPUCTL).unwrap_or(0xDEAD);
    let sctl = bar0.read_u32(base + wpr_regs::SCTL).unwrap_or(0xDEAD);
    let pc = bar0.read_u32(base + wpr_regs::PC).unwrap_or(0xDEAD);
    let exci = bar0.read_u32(base + wpr_regs::EXCI).unwrap_or(0xDEAD);
    let mb0 = bar0.read_u32(base + wpr_regs::MAILBOX0).unwrap_or(0xDEAD);
    let hreset = cpuctl & 0x10 != 0;
    let halted = cpuctl & 0x20 != 0;
    let mode = match sctl {
        0x3000 => "LS",
        0x3002 => "HS",
        0x7021 => "FW",
        _ => "??",
    };
    let s = format!(
        "{name:6}: cpuctl={cpuctl:#010x} SCTL={sctl:#06x}({mode}) \
         PC={pc:#06x} EXCI={exci:#010x} MB0={mb0:#010x} hreset={hreset} halted={halted}"
    );
    eprintln!("  {s}");
    s
}

fn clear_pri_faults(bar0: &MappedBar) {
    let status = bar0.read_u32(wpr_regs::PRIV_RING_INTR_STATUS).unwrap_or(0);
    if status != 0 {
        eprintln!("  PRI faults pending: {status:#010x}, clearing...");
        for _ in 0..10 {
            let _ = bar0.write_u32(wpr_regs::PRIV_RING_COMMAND, wpr_regs::PRIV_RING_CMD_ACK);
            std::thread::sleep(std::time::Duration::from_millis(20));
            let s = bar0.read_u32(wpr_regs::PRIV_RING_INTR_STATUS).unwrap_or(0);
            if s == 0 {
                eprintln!("  PRI faults cleared");
                return;
            }
        }
        let s = bar0.read_u32(wpr_regs::PRIV_RING_INTR_STATUS).unwrap_or(0);
        eprintln!("  PRI faults remain: {s:#010x}");
    } else {
        eprintln!("  PRI ring clean");
    }
}

fn read_wpr2_indexed(bar0: &MappedBar) -> (u32, u32) {
    let _ = bar0.write_u32(wpr_regs::INDEXED_WPR, 2);
    std::thread::sleep(std::time::Duration::from_micros(10));
    let start_raw = bar0.read_u32(wpr_regs::INDEXED_WPR).unwrap_or(0xDEAD);
    let _ = bar0.write_u32(wpr_regs::INDEXED_WPR, 3);
    std::thread::sleep(std::time::Duration::from_micros(10));
    let end_raw = bar0.read_u32(wpr_regs::INDEXED_WPR).unwrap_or(0xDEAD);
    (start_raw, end_raw)
}

fn decode_indexed_wpr2(raw: u32) -> u64 {
    ((raw as u64) & 0xFFFF_FF00) << 8
}

fn wpr2_indexed_valid(start_raw: u32, end_raw: u32) -> bool {
    let s = decode_indexed_wpr2(start_raw);
    let e = decode_indexed_wpr2(end_raw) + 0x20000;
    s > 0 && e > s && (e - s) > 0x1000
}

/// Probe a register: read, write test value, read back, restore.
fn probe_register_writability(bar0: &MappedBar, name: &str, addr: usize, test_val: u32) {
    let before = bar0.read_u32(addr).unwrap_or(0xDEAD_DEAD);
    let _ = bar0.write_u32(addr, test_val);
    std::thread::sleep(std::time::Duration::from_micros(50));
    let after = bar0.read_u32(addr).unwrap_or(0xDEAD_DEAD);
    let _ = bar0.write_u32(addr, before);
    std::thread::sleep(std::time::Duration::from_micros(50));
    let restored = bar0.read_u32(addr).unwrap_or(0xDEAD_DEAD);

    let writable = after == test_val;
    let restored_ok = restored == before;
    let pri_fault = after == 0xBADF_1100 || after == 0xDEAD_DEAD;

    eprintln!(
        "  {name:30} ({addr:#010x}): before={before:#010x} wrote={test_val:#010x} \
         after={after:#010x} writable={writable} restore={restored_ok} pri_fault={pri_fault}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════
// 122A: WPR2 Register Write Probe
// ═══════════════════════════════════════════════════════════════════════════

#[test]
#[ignore = "requires VFIO-bound GPU hardware + GlowPlug + Ember"]
fn exp122a_wpr2_register_write_probe() {
    init_tracing();
    let eq = "=".repeat(70);
    let bdf = crate::helpers::vfio_bdf();

    eprintln!("{eq}");
    eprintln!("#  Exp 122A: WPR2 Register Write Probe");
    eprintln!("#  Can we SET WPR2 boundaries from the host?");
    eprintln!("{eq}");

    let fds = crate::ember_client::request_fds(&bdf).expect("ember fds");
    let device = VfioDevice::from_received(&bdf, fds).expect("VfioDevice");
    let bar0 = device.map_bar(0).expect("BAR0");

    let boot0 = bar0.read_u32(0).unwrap_or(0);
    eprintln!("  BOOT0={boot0:#010x} BDF={bdf}");

    // Phase 1: Clear PRI faults for clean register access
    eprintln!("\n  ── Phase 1: Clear PRI faults ──");
    clear_pri_faults(&bar0);

    // Phase 2: Read current WPR2 state from all register locations
    eprintln!("\n  ── Phase 2: Current WPR2 state ──");
    let (idx_start, idx_end) = read_wpr2_indexed(&bar0);
    let valid = wpr2_indexed_valid(idx_start, idx_end);
    eprintln!(
        "  Indexed (0x100CD4): start_raw={idx_start:#010x} end_raw={idx_end:#010x} \
         decoded: {:#x}..{:#x} valid={valid}",
        decode_indexed_wpr2(idx_start),
        decode_indexed_wpr2(idx_end) + 0x20000
    );

    let direct_beg = bar0.read_u32(wpr_regs::PFB_WPR2_BEG).unwrap_or(0xDEAD);
    let direct_end = bar0.read_u32(wpr_regs::PFB_WPR2_END).unwrap_or(0xDEAD);
    eprintln!("  Direct (CEC/CF0): beg={direct_beg:#010x} end={direct_end:#010x}");

    let fbpa_lo = bar0.read_u32(wpr_regs::FBPA_WPR2_ADDR_LO).unwrap_or(0xDEAD);
    let fbpa_hi = bar0.read_u32(wpr_regs::FBPA_WPR2_ADDR_HI).unwrap_or(0xDEAD);
    eprintln!("  FBPA (1FA824/828): lo={fbpa_lo:#010x} hi={fbpa_hi:#010x}");

    // Phase 3: Falcon state
    eprintln!("\n  ── Phase 3: Falcon state ──");
    falcon_state(&bar0, "PMU", wpr_regs::PMU_BASE);
    falcon_state(&bar0, "SEC2", wpr_regs::SEC2_BASE);
    falcon_state(&bar0, "FECS", wpr_regs::FECS_BASE);
    falcon_state(&bar0, "GPCCS", wpr_regs::GPCCS_BASE);

    // Phase 4: Systematic register write probes
    eprintln!("\n  ── Phase 4: Register write probes ──");
    eprintln!("  Testing each WPR2-related register for host writability...\n");

    // Use a realistic WPR2 start value (256KB-aligned address in VRAM)
    let test_wpr_start: u32 = 0x002F_FE02; // encoded: (0x2FFE00000 >> 8) | idx_2
    let test_wpr_end: u32 = 0x002F_FF03; // encoded: (0x2FFF00000 >> 8) | idx_3

    // Indexed WPR register — try writing with data + index encoding
    let _ = bar0.write_u32(wpr_regs::INDEXED_WPR, 2);
    let pre_start = bar0.read_u32(wpr_regs::INDEXED_WPR).unwrap_or(0xDEAD);
    let _ = bar0.write_u32(wpr_regs::INDEXED_WPR, test_wpr_start);
    std::thread::sleep(std::time::Duration::from_micros(50));
    let _ = bar0.write_u32(wpr_regs::INDEXED_WPR, 2);
    let post_start = bar0.read_u32(wpr_regs::INDEXED_WPR).unwrap_or(0xDEAD);
    let start_changed = post_start != pre_start;
    eprintln!(
        "  Indexed WPR start: pre={pre_start:#010x} test={test_wpr_start:#010x} \
         post={post_start:#010x} CHANGED={start_changed}"
    );

    let _ = bar0.write_u32(wpr_regs::INDEXED_WPR, 3);
    let pre_end = bar0.read_u32(wpr_regs::INDEXED_WPR).unwrap_or(0xDEAD);
    let _ = bar0.write_u32(wpr_regs::INDEXED_WPR, test_wpr_end);
    std::thread::sleep(std::time::Duration::from_micros(50));
    let _ = bar0.write_u32(wpr_regs::INDEXED_WPR, 3);
    let post_end = bar0.read_u32(wpr_regs::INDEXED_WPR).unwrap_or(0xDEAD);
    let end_changed = post_end != pre_end;
    eprintln!(
        "  Indexed WPR end:   pre={pre_end:#010x} test={test_wpr_end:#010x} \
         post={post_end:#010x} CHANGED={end_changed}"
    );

    // Direct flat register probes
    probe_register_writability(
        &bar0,
        "PFB_WPR2_BEG (0x100CEC)",
        wpr_regs::PFB_WPR2_BEG,
        0x2FFE_0000,
    );
    probe_register_writability(
        &bar0,
        "PFB_WPR2_END (0x100CF0)",
        wpr_regs::PFB_WPR2_END,
        0x3000_0000,
    );
    probe_register_writability(
        &bar0,
        "FBPA_WPR2_LO (0x1FA824)",
        wpr_regs::FBPA_WPR2_ADDR_LO,
        0x0002_FFE0,
    );
    probe_register_writability(
        &bar0,
        "FBPA_WPR2_HI (0x1FA828)",
        wpr_regs::FBPA_WPR2_ADDR_HI,
        0x0003_0000,
    );

    // Probe WPR configuration register (might control WPR enable/mode)
    probe_register_writability(
        &bar0,
        "PFB_WPR_CFG (0x100CD0)",
        wpr_regs::PFB_WPR_CFG,
        0x0000_0001,
    );

    // Probe additional indexed values (0-7) for any accessible WPR sub-regs
    eprintln!("\n  ── Indexed register scan (indices 0-15) ──");
    for idx in 0u32..16 {
        let _ = bar0.write_u32(wpr_regs::INDEXED_WPR, idx);
        std::thread::sleep(std::time::Duration::from_micros(10));
        let val = bar0.read_u32(wpr_regs::INDEXED_WPR).unwrap_or(0xDEAD_DEAD);
        if val != 0 && val != idx && val != 0xDEAD_DEAD {
            eprintln!("    idx={idx:2}: {val:#010x} (non-trivial)");
        } else {
            eprintln!("    idx={idx:2}: {val:#010x}");
        }
    }

    // Phase 5: Scan FBPA partition registers (per-partition WPR)
    eprintln!("\n  ── Phase 5: FBPA partition WPR scan ──");
    for part in 0..4u32 {
        let base = 0x1F0000 + part * 0x4000;
        let wpr_lo = bar0.read_u32((base + 0x824) as usize).unwrap_or(0xBADF);
        let wpr_hi = bar0.read_u32((base + 0x828) as usize).unwrap_or(0xBADF);
        let ctrl = bar0.read_u32(base as usize).unwrap_or(0xBADF);
        if wpr_lo != 0xBADF_1100 || wpr_hi != 0xBADF_1100 {
            eprintln!(
                "  FBPA[{part}] @ {base:#08x}: ctrl={ctrl:#010x} wpr_lo={wpr_lo:#010x} wpr_hi={wpr_hi:#010x}"
            );
        } else {
            eprintln!("  FBPA[{part}] @ {base:#08x}: PRI FAULT (partition offline)");
        }
    }

    // Phase 6: Probe NV_PFB_PRI_MMU register space for hidden WPR controls
    eprintln!("\n  ── Phase 6: PFB_PRI_MMU register scan (0x100C80..0x100D10) ──");
    for addr in (0x100C80..=0x100D10).step_by(4) {
        let val = bar0.read_u32(addr).unwrap_or(0xDEAD_DEAD);
        if val != 0 && val != 0xDEAD_DEAD && val != 0xBADF_1100 {
            eprintln!("    {addr:#010x}: {val:#010x}");
        }
    }

    // Phase 7: Check if PRI ring state changed
    eprintln!("\n  ── Phase 7: Post-probe PRI state ──");
    let pri = bar0
        .read_u32(wpr_regs::PRIV_RING_INTR_STATUS)
        .unwrap_or(0xDEAD);
    eprintln!("  PRI_RING_INTR_STATUS: {pri:#010x}");
    if pri != 0 {
        clear_pri_faults(&bar0);
    }

    // Phase 8: Re-read WPR2 to confirm no accidental corruption
    eprintln!("\n  ── Phase 8: Final WPR2 check ──");
    let (final_start, final_end) = read_wpr2_indexed(&bar0);
    eprintln!(
        "  Indexed: start={final_start:#010x} end={final_end:#010x} valid={}",
        wpr2_indexed_valid(final_start, final_end)
    );

    eprintln!("\n{eq}");
    eprintln!("#  Exp 122A COMPLETE");
    eprintln!("{eq}");
}

// ═══════════════════════════════════════════════════════════════════════════
// 122B: Parasitic Nouveau Mode
// ═══════════════════════════════════════════════════════════════════════════

#[test]
#[ignore = "requires VFIO-bound GPU hardware + GlowPlug + Ember"]
fn exp122b_parasitic_nouveau() {
    init_tracing();
    let eq = "=".repeat(70);

    eprintln!("{eq}");
    eprintln!("#  Exp 122B: Parasitic Nouveau Mode");
    eprintln!("#  Read WPR state + interact with live falcons via sysfs BAR0");
    eprintln!("{eq}");

    // Find vfio BDF (Titan #1 = 0000:03:00.0 or Titan #2 = 0000:4a:00.0)
    let bdf = crate::helpers::vfio_bdf();
    let sysfs_dev = format!("/sys/bus/pci/devices/{bdf}");

    // Phase 1: Swap to nouveau
    eprintln!("\n  ── Phase 1: Swap to nouveau ──");
    let mut gp = crate::glowplug_client::GlowPlugClient::connect().expect("GlowPlug connection");
    gp.swap(&bdf, "nouveau").expect("swap→nouveau");
    std::thread::sleep(std::time::Duration::from_secs(4));
    eprintln!("  nouveau bound, waiting for initialization...");
    std::thread::sleep(std::time::Duration::from_secs(2));

    // Phase 2: Open sysfs BAR0
    eprintln!("\n  ── Phase 2: Open sysfs BAR0 ──");
    let mut bar0 =
        coral_driver::nv::bar0::Bar0Access::from_sysfs_device(&sysfs_dev).expect("sysfs BAR0");
    use coral_driver::gsp::RegisterAccess;
    eprintln!("  BAR0 sysfs: {} MiB", bar0.size() / (1024 * 1024));

    let boot0 = bar0.read_u32(0).unwrap_or(0xDEAD);
    eprintln!("  BOOT0={boot0:#010x}");

    // Phase 3: Read ALL WPR2 registers while nouveau is active
    eprintln!("\n  ── Phase 3: WPR2 registers (nouveau active) ──");

    let r =
        |b: &coral_driver::nv::bar0::Bar0Access, off: u32| b.read_u32(off).unwrap_or(0xDEAD_DEAD);

    // Indexed WPR2
    let _ = bar0.write_u32(0x100CD4, 2);
    let idx_start = r(&bar0, 0x100CD4);
    let _ = bar0.write_u32(0x100CD4, 3);
    let idx_end = r(&bar0, 0x100CD4);
    let s = decode_indexed_wpr2(idx_start);
    let e = decode_indexed_wpr2(idx_end) + 0x20000;
    eprintln!("  Indexed WPR2: start_raw={idx_start:#010x} end_raw={idx_end:#010x}");
    eprintln!(
        "  Decoded: {s:#x}..{e:#x} ({} KiB) valid={}",
        (e - s) / 1024,
        s > 0 && e > s
    );

    // Direct registers
    let cec = r(&bar0, 0x100CEC);
    let cf0 = r(&bar0, 0x100CF0);
    eprintln!("  Direct (CEC/CF0): {cec:#010x} / {cf0:#010x}");

    // FBPA registers
    let fbpa_lo = r(&bar0, 0x1FA824);
    let fbpa_hi = r(&bar0, 0x1FA828);
    eprintln!("  FBPA (1FA824/828): lo={fbpa_lo:#010x} hi={fbpa_hi:#010x}");

    // WPR config register
    let wpr_cfg = r(&bar0, 0x100CD0);
    eprintln!("  WPR_CFG (100CD0): {wpr_cfg:#010x}");

    // Full indexed scan
    eprintln!("\n  ── Indexed register scan (nouveau) ──");
    for idx in 0u32..16 {
        let _ = bar0.write_u32(0x100CD4, idx);
        let val = r(&bar0, 0x100CD4);
        if val != idx && val != 0 {
            eprintln!("    idx={idx:2}: {val:#010x}");
        }
    }

    // Phase 4: Falcon state under nouveau
    eprintln!("\n  ── Phase 4: Falcon state (nouveau active) ──");
    let sysfs_falcon = |name: &str, base: u32| {
        let cpuctl = r(&bar0, base + 0x100);
        let sctl = r(&bar0, base + 0x240);
        let pc = r(&bar0, base + 0x030);
        let exci = r(&bar0, base + 0x148);
        let mb0 = r(&bar0, base + 0x040);
        let hreset = cpuctl & 0x10 != 0;
        let halted = cpuctl & 0x20 != 0;
        let mode = match sctl {
            0x3000 => "LS",
            0x3002 => "HS",
            0x7021 => "FW",
            _ => "??",
        };
        eprintln!(
            "  {name:6}: cpuctl={cpuctl:#010x} SCTL={sctl:#06x}({mode}) \
             PC={pc:#06x} EXCI={exci:#010x} MB0={mb0:#010x} hreset={hreset} halted={halted}"
        );
        (cpuctl, sctl, pc)
    };

    let (_, _, _) = sysfs_falcon("PMU", 0x10a000);
    let (_sec2_cpu, _sec2_sctl, _) = sysfs_falcon("SEC2", 0x087000);
    let (fecs_cpu, fecs_sctl, _) = sysfs_falcon("FECS", 0x409000);
    let (_gpccs_cpu, _gpccs_sctl, _) = sysfs_falcon("GPCCS", 0x41a000);

    // Phase 5: Read WPR headers from VRAM via PRAMIN (sysfs BAR0)
    eprintln!("\n  ── Phase 5: WPR headers from VRAM ──");
    if s > 0 && e > s {
        let wpr2_vram = s as u32;
        eprintln!("  Reading WPR headers at VRAM {wpr2_vram:#010x}...");

        // PRAMIN via sysfs: set BAR0_WINDOW, then read from 0x700000
        let window_base = wpr2_vram & !0xFFFF;
        let offset_in_window = (wpr2_vram & 0xFFFF) as u32;
        let saved_window = r(&bar0, 0x1700);
        let _ = bar0.write_u32(0x1700, window_base >> 16);
        std::thread::sleep(std::time::Duration::from_micros(50));

        for i in 0..11u32 {
            let hdr_off = 0x700000 + offset_in_window + i * 24;
            let falcon_id = r(&bar0, hdr_off);
            if falcon_id == 0xFFFF_FFFF || falcon_id == 0xDEAD_DEAD {
                eprintln!("    [{i}] sentinel/end");
                break;
            }
            let lsb_off = r(&bar0, hdr_off + 4);
            let bootstrap_owner = r(&bar0, hdr_off + 8);
            let lazy = r(&bar0, hdr_off + 12);
            let bin_version = r(&bar0, hdr_off + 16);
            let status = r(&bar0, hdr_off + 20);
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
                "    [{i}] falcon={falcon_id}({fname}) lsb={lsb_off:#x} owner={bootstrap_owner} \
                 lazy={lazy} ver={bin_version:#x} status={status}({status_str})"
            );
        }
        let _ = bar0.write_u32(0x1700, saved_window);
    } else {
        eprintln!("  WPR2 invalid — cannot read WPR headers");
    }

    // Phase 6: FECS method interface probe (FECS is alive under nouveau)
    eprintln!("\n  ── Phase 6: FECS method interface ──");
    let fecs_alive = fecs_cpu & 0x30 == 0 && fecs_cpu != 0xDEAD_DEAD;
    eprintln!("  FECS alive: {fecs_alive} (SCTL={fecs_sctl:#06x})");

    if fecs_alive {
        // Read FECS mailbox registers for current state
        let fecs_mb0 = r(&bar0, 0x409040);
        let fecs_mb1 = r(&bar0, 0x409044);
        let fecs_os = r(&bar0, 0x409094); // OS register
        eprintln!("  FECS MB0={fecs_mb0:#010x} MB1={fecs_mb1:#010x} OS={fecs_os:#010x}");

        // Try reading FECS method interface
        let fecs_method_data = r(&bar0, 0x409500); // FECS_METHOD_DATA
        let fecs_method_push = r(&bar0, 0x409504); // FECS_METHOD_PUSH
        eprintln!("  FECS METHOD: data={fecs_method_data:#010x} push={fecs_method_push:#010x}");

        // Read GR engine status
        let gr_status = r(&bar0, 0x400700);
        let gr_intr = r(&bar0, 0x400100);
        eprintln!("  GR: status={gr_status:#010x} intr={gr_intr:#010x}");

        // Attempt to read context image size via FECS method
        eprintln!("\n  Attempting FECS method: GET_IMAGE_SIZE (0x108)...");
        let _ = bar0.write_u32(0x409500, 0); // data = 0
        let _ = bar0.write_u32(0x409504, 0x108); // method = GET_IMAGE_SIZE
        std::thread::sleep(std::time::Duration::from_millis(100));
        let method_result = r(&bar0, 0x409500);
        let method_status = r(&bar0, 0x409504);
        eprintln!("  FECS response: data={method_result:#010x} status={method_status:#010x}");
        if method_result > 0 && method_result < 0x100_0000 {
            eprintln!("  *** FECS responded! Context image size = {method_result:#x} ***");
        }
    }

    // Phase 7: PRAMIN hexdump at WPR2 region (first 256 bytes)
    if s > 0 && e > s {
        eprintln!("\n  ── Phase 7: VRAM hexdump at WPR2 start ──");
        let wpr2_vram = s as u32;
        for row in 0..8u32 {
            let row_addr = wpr2_vram + row * 16;
            let window_base = row_addr & !0xFFFF;
            let off_in_win = (row_addr & 0xFFFF) as u32;
            let _ = bar0.write_u32(0x1700, window_base >> 16);
            std::thread::sleep(std::time::Duration::from_micros(10));
            let mut hex = String::new();
            for col in 0..4u32 {
                let mmio_off = 0x700000 + off_in_win + col * 4;
                let val = r(&bar0, mmio_off);
                hex.push_str(&format!("{val:08x} "));
            }
            eprintln!("    {row_addr:#010x}: {hex}");
        }
    }

    // Drop sysfs BAR0 before driver swap
    drop(bar0);

    // Phase 8: Swap back to vfio-pci
    eprintln!("\n  ── Phase 8: Swap back to vfio-pci ──");
    gp.swap(&bdf, "vfio-pci").expect("swap→vfio-pci");
    std::thread::sleep(std::time::Duration::from_millis(500));

    eprintln!("\n{eq}");
    eprintln!("#  Exp 122B COMPLETE");
    eprintln!("{eq}");
}

// ═══════════════════════════════════════════════════════════════════════════
// 122C: FWSEC Binary Extraction
// ═══════════════════════════════════════════════════════════════════════════

#[test]
#[ignore = "requires VFIO-bound GPU hardware + GlowPlug + Ember"]
fn exp122c_fwsec_extraction() {
    init_tracing();
    let eq = "=".repeat(70);
    let bdf = crate::helpers::vfio_bdf();

    eprintln!("{eq}");
    eprintln!("#  Exp 122C: FWSEC Binary Extraction from VBIOS");
    eprintln!("#  Understand how FWSEC carves WPR2 at boot");
    eprintln!("{eq}");

    let fds = crate::ember_client::request_fds(&bdf).expect("ember fds");
    let device = VfioDevice::from_received(&bdf, fds).expect("VfioDevice");
    let bar0 = device.map_bar(0).expect("BAR0");

    let boot0 = bar0.read_u32(0).unwrap_or(0);
    eprintln!("  BOOT0={boot0:#010x} BDF={bdf}");

    // Phase 1: Read VBIOS from PROM
    eprintln!("\n  ── Phase 1: Read VBIOS from PROM ──");
    use coral_driver::vfio::channel::devinit::{BitTable, parse_pmu_table, read_vbios_prom};

    let rom = read_vbios_prom(&bar0).expect("VBIOS PROM read");
    eprintln!(
        "  VBIOS size: {} bytes ({} KiB)",
        rom.len(),
        rom.len() / 1024
    );

    // Phase 2: Parse BIT table
    eprintln!("\n  ── Phase 2: BIT table entries ──");
    let bit = BitTable::parse(&rom).expect("BIT parse");
    for entry in &bit.entries {
        let id_char = if entry.id.is_ascii_graphic() {
            entry.id as char
        } else {
            '?'
        };
        eprintln!(
            "    BIT '{id_char}' (0x{:02x}): ver={} data_off={:#06x} data_size={:#x}",
            entry.id, entry.version, entry.data_offset, entry.data_size
        );
    }

    // Phase 3: Parse PMU firmware table
    eprintln!("\n  ── Phase 3: PMU firmware table ──");
    match parse_pmu_table(&rom, &bit) {
        Ok(pmu_fws) => {
            for (i, fw) in pmu_fws.iter().enumerate() {
                eprintln!(
                    "    PMU[{i}]: type={} boot_addr={:#x} boot_size={:#x} \
                     code_addr={:#x} code_size={:#x} data_addr={:#x} data_size={:#x}",
                    fw.app_type,
                    fw.boot_addr,
                    fw.boot_size,
                    fw.code_addr,
                    fw.code_size,
                    fw.data_addr,
                    fw.data_size
                );
            }
        }
        Err(e) => eprintln!("    PMU table parse error: {e}"),
    }

    // Phase 4: Scan for FWSEC-related BIT entries
    eprintln!("\n  ── Phase 4: FWSEC-related BIT entries ──");

    // 'B' = Boot scripts, 'S' = security/FWSEC, 'F' = falcon
    for &entry_id in b"BSFfUuiI" {
        if let Some(entry) = bit.find(entry_id) {
            let id_char = entry_id as char;
            eprintln!(
                "  BIT '{id_char}': offset={:#06x} size={:#x}",
                entry.data_offset, entry.data_size
            );
            let off = entry.data_offset as usize;
            let end = (off + entry.data_size as usize).min(rom.len());
            if end > off && end - off >= 4 {
                let data = &rom[off..end];
                let mut hex = String::new();
                for (j, chunk) in data.chunks(4).enumerate().take(8) {
                    if chunk.len() == 4 {
                        let w = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
                        hex.push_str(&format!("[{:02}]={w:#010x} ", j));
                    }
                }
                eprintln!("    Data: {hex}");
            }
        }
    }

    // Phase 5: Scan VBIOS for SEC2/FWSEC falcon image signatures
    eprintln!("\n  ── Phase 5: Falcon image scan in VBIOS ──");

    // Falcon images in VBIOS have a known header: 2-byte version + offset structure
    // NVKM's sec2 FWSEC is referenced from BIT 'B' (boot) table
    // Scan for "HSF_FW" style headers (used by falcon firmware)
    let scan_patterns: &[(&str, &[u8])] = &[
        ("NVIDIA VBIOS", b"NVIDIA"),
        ("PCI ROM", &[0x55, 0xAA]),
        ("NPDE (PCI Data)", b"PCIR"),
        ("Falcon IMEM tag", &[0x00, 0xF0, 0x0F, 0x00]),
    ];

    for (label, pattern) in scan_patterns {
        let count = rom
            .windows(pattern.len())
            .filter(|w| *w == *pattern)
            .count();
        if count > 0 {
            let first = rom
                .windows(pattern.len())
                .position(|w| w == *pattern)
                .unwrap();
            eprintln!("    {label}: {count} occurrences, first at offset {first:#06x}");
        }
    }

    // Phase 6: Scan for boot descriptor table structures
    eprintln!("\n  ── Phase 6: Boot descriptor structures ──");
    // On GV100, the FWSEC boot is described in the BIT 'B' table.
    // 'B' table has a pointer to the boot script table.
    if let Some(b_entry) = bit.find(b'B') {
        let off = b_entry.data_offset as usize;
        if off + 8 <= rom.len() {
            let boot_script_ptr = u16::from_le_bytes([rom[off], rom[off + 1]]) as usize;
            eprintln!("  BIT 'B' boot_script_ptr: {boot_script_ptr:#06x}");
            if boot_script_ptr > 0 && boot_script_ptr + 4 < rom.len() {
                let ver = rom[boot_script_ptr];
                let hdr_size = rom[boot_script_ptr + 1];
                let entry_count = rom[boot_script_ptr + 2];
                let entry_size = rom[boot_script_ptr + 3];
                eprintln!(
                    "    Boot script table: ver={ver} hdr={hdr_size} entries={entry_count} entry_size={entry_size}"
                );
            }
        }
    }

    // Phase 7: Extract SEC2 EMEM data (FWSEC leaves metadata there)
    eprintln!("\n  ── Phase 7: SEC2 EMEM content ──");
    use coral_driver::nv::vfio_compute::acr_boot::{sec2_emem_read, sec2_tracepc_dump};

    let emem_offsets = [0u32, 0x100, 0x200, 0x400, 0x800];
    for &off in &emem_offsets {
        let data = sec2_emem_read(&bar0, off, 8);
        let non_zero = data.iter().any(|&v| v != 0);
        if non_zero {
            let hex: Vec<String> = data.iter().map(|v| format!("{v:#010x}")).collect();
            eprintln!("    EMEM[{off:#06x}]: {}", hex.join(" "));
        }
    }

    // Phase 8: Scan for known FWSEC opcodes in VBIOS
    eprintln!("\n  ── Phase 8: FWSEC opcode patterns in VBIOS ──");
    // FWSEC typically writes to NV_PFB_PRI_MMU_WPR* registers.
    // These register addresses would appear as 32-bit LE constants in the firmware.
    let target_constants: &[(u32, &str)] = &[
        (0x100CD4, "INDEXED_WPR (0x100CD4)"),
        (0x100CD0, "WPR_CFG (0x100CD0)"),
        (0x100CEC, "PFB_reg_CEC (0x100CEC)"),
        (0x100CF0, "PFB_reg_CF0 (0x100CF0)"),
        (0x1FA824, "FBPA_WPR2_LO (0x1FA824)"),
        (0x1FA828, "FBPA_WPR2_HI (0x1FA828)"),
    ];

    for &(val, label) in target_constants {
        let bytes = val.to_le_bytes();
        let positions: Vec<usize> = rom
            .windows(4)
            .enumerate()
            .filter(|(_, w)| *w == bytes)
            .map(|(pos, _)| pos)
            .collect();
        if !positions.is_empty() {
            let pos_str: Vec<String> = positions
                .iter()
                .take(5)
                .map(|p| format!("{p:#06x}"))
                .collect();
            let suffix = if positions.len() > 5 {
                format!(" (+{} more)", positions.len() - 5)
            } else {
                String::new()
            };
            eprintln!("    {label}: found at {}{suffix}", pos_str.join(", "));
        } else {
            eprintln!("    {label}: NOT FOUND in VBIOS");
        }
    }

    // Phase 9: Dump PCIR structures for multi-image VBIOS
    eprintln!("\n  ── Phase 9: VBIOS image structure ──");
    let mut img_start = 0usize;
    let mut img_idx = 0;
    while img_start < rom.len() - 2 {
        if rom[img_start] != 0x55 || rom[img_start + 1] != 0xAA {
            break;
        }
        let pcir_ptr = if img_start + 0x18 + 2 <= rom.len() {
            u16::from_le_bytes([rom[img_start + 0x18], rom[img_start + 0x19]]) as usize
        } else {
            break;
        };
        let pcir_off = img_start + pcir_ptr;
        if pcir_off + 0x18 > rom.len() {
            break;
        }
        if &rom[pcir_off..pcir_off + 4] != b"PCIR" {
            eprintln!("    Image {img_idx} at {img_start:#06x}: PCIR signature mismatch");
            break;
        }

        let vendor = u16::from_le_bytes([rom[pcir_off + 4], rom[pcir_off + 5]]);
        let device_id = u16::from_le_bytes([rom[pcir_off + 6], rom[pcir_off + 7]]);
        let img_len_blocks =
            u16::from_le_bytes([rom[pcir_off + 0x10], rom[pcir_off + 0x11]]) as usize;
        let img_len = img_len_blocks * 512;
        let code_type = rom[pcir_off + 0x14];
        let indicator = rom[pcir_off + 0x15];
        let last = indicator & 0x80 != 0;

        let type_str = match code_type {
            0 => "x86/BIOS",
            1 => "OpenFirmware",
            3 => "EFI",
            0xFF => "NVIDIA-extended",
            _ => "???",
        };

        eprintln!(
            "    Image {img_idx}: offset={img_start:#06x} size={img_len:#x} ({} KiB) \
             vendor={vendor:#06x} dev={device_id:#06x} type={code_type}({type_str}) last={last}",
            img_len / 1024
        );

        if last || img_len == 0 {
            break;
        }
        img_start += img_len;
        img_idx += 1;
    }

    // Phase 10: SEC2 TRACEPC (firmware execution trace)
    eprintln!("\n  ── Phase 10: SEC2 execution trace ──");
    let (tracepc_idx, tracepc_vals) = sec2_tracepc_dump(&bar0);
    eprintln!("  TRACEPC index: {tracepc_idx:#010x}");
    let non_zero: Vec<String> = tracepc_vals
        .iter()
        .enumerate()
        .filter(|(_, v)| **v != 0)
        .map(|(i, v)| format!("[{i}]={v:#010x}"))
        .collect();
    if !non_zero.is_empty() {
        eprintln!("  Non-zero trace entries: {}", non_zero.join(" "));
    } else {
        eprintln!("  All trace entries zero");
    }

    eprintln!("\n{eq}");
    eprintln!("#  Exp 122C COMPLETE");
    eprintln!("{eq}");
}
