// SPDX-License-Identifier: AGPL-3.0-only

//! Exp 099: Post-Nouveau Falcon State — Path R
//!
//! Hypothesis: nouveau runs the full ACR chain during its 3-second bind,
//! loading authenticated FECS/GPCCS firmware. After swap back to vfio-pci,
//! that firmware may still be resident in IMEM. If so, we can skip our
//! entire ACR chain and directly boot FECS/GPCCS.
//!
//! This test:
//! 1. Nouveau cycle via GlowPlug (3s bind runs full ACR)
//! 2. Comprehensive FECS/GPCCS/SEC2 state dump
//! 3. IMEM readback (is there code?)
//! 4. DMEM readback (is there data?)
//! 5. Try STARTCPU on FECS if firmware is present
//! 6. If FECS responds, try GR init methods

use crate::ember_client;
use crate::helpers::{init_tracing, vfio_bdf};

const SEC2_BASE: usize = 0x087000;
const FECS_BASE: usize = 0x409000;
const GPCCS_BASE: usize = 0x41a000;

#[allow(dead_code, reason = "hardware register map — reference for bring-up")]
mod freg {
    pub const IRQSSET: usize = 0x000;
    pub const IRQSTAT: usize = 0x008;
    pub const IRQMASK: usize = 0x018;
    pub const PC: usize = 0x030;
    pub const MAILBOX0: usize = 0x040;
    pub const MAILBOX1: usize = 0x044;
    pub const CPUCTL: usize = 0x100;
    pub const BOOTVEC: usize = 0x104;
    pub const HWCFG: usize = 0x108;
    pub const SCTL: usize = 0x240;
    pub const EXCI: usize = 0x148;
    pub const DMEMC: usize = 0x1C0;
    pub const DMEMD: usize = 0x1C4;
    pub const IMEMC: usize = 0x180;
    pub const IMEMD: usize = 0x184;
    pub const ITFEN: usize = 0x048;

    pub const CPUCTL_STARTCPU: u32 = 1 << 1;
    pub const CPUCTL_HALTED: u32 = 1 << 4;
    pub const CPUCTL_STOPPED: u32 = 1 << 5;

    pub fn imem_size(hwcfg: u32) -> usize {
        ((hwcfg & 0x1FF) as usize) << 8
    }
    pub fn dmem_size(hwcfg: u32) -> usize {
        (((hwcfg >> 9) & 0x1FF) as usize) << 8
    }
}

#[test]
#[ignore = "requires VFIO-bound GPU hardware"]
fn post_nouveau_falcon_state() {
    init_tracing();

    eprintln!("\n=== Exp 099: Post-Nouveau Falcon State (Path R) ===\n");

    let bdf = vfio_bdf();

    // ── Phase 1: Nouveau cycle ──
    eprintln!("── Phase 1: Nouveau Cycle ──");
    {
        let mut gp =
            crate::glowplug_client::GlowPlugClient::connect().expect("GlowPlug connection");

        match gp.swap(&bdf, "nouveau") {
            Ok(_) => eprintln!("  swap→nouveau: OK"),
            Err(e) => {
                eprintln!("  swap→nouveau FAILED: {e}");
                return;
            }
        }
        std::thread::sleep(std::time::Duration::from_secs(3));

        match gp.swap(&bdf, "vfio-pci") {
            Ok(_) => eprintln!("  swap→vfio-pci: OK"),
            Err(e) => {
                eprintln!("  swap→vfio-pci FAILED: {e}");
                return;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }

    let fds = ember_client::request_fds(&bdf).expect("ember fds");
    let vfio_dev = coral_driver::vfio::VfioDevice::from_received(&bdf, fds).expect("VfioDevice");
    let bar0 = vfio_dev.map_bar(0).expect("map_bar(0)");

    // Helper closures for each falcon
    let dump_falcon = |name: &str, base: usize| {
        let r = |off: usize| bar0.read_u32(base + off).unwrap_or(0xDEAD_DEAD);

        let cpuctl = r(freg::CPUCTL);
        let hwcfg = r(freg::HWCFG);
        let sctl = r(freg::SCTL);
        let pc = r(freg::PC);
        let exci = r(freg::EXCI);
        let mb0 = r(freg::MAILBOX0);
        let mb1 = r(freg::MAILBOX1);
        let bootvec = r(freg::BOOTVEC);
        let irqstat = r(freg::IRQSTAT);
        let irqmask = r(freg::IRQMASK);
        let itfen = r(freg::ITFEN);

        let imem_sz = freg::imem_size(hwcfg);
        let dmem_sz = freg::dmem_size(hwcfg);
        let hs = sctl & 0x02 != 0;
        let halted = cpuctl & freg::CPUCTL_HALTED != 0;
        let stopped = cpuctl & freg::CPUCTL_STOPPED != 0;

        eprintln!("  {name}:");
        eprintln!(
            "    cpuctl={cpuctl:#010x} sctl={sctl:#010x} HS={hs} HALTED={halted} STOPPED={stopped}"
        );
        eprintln!("    PC={pc:#06x} EXCI={exci:#010x} BOOTVEC={bootvec:#06x}");
        eprintln!("    mb0={mb0:#010x} mb1={mb1:#010x}");
        eprintln!("    IMEM={imem_sz}B DMEM={dmem_sz}B hwcfg={hwcfg:#010x}");
        eprintln!("    irqstat={irqstat:#010x} irqmask={irqmask:#010x} itfen={itfen:#010x}");

        // IMEM probe: read first 64 words to check for code
        let w = |off: usize, val: u32| {
            let _ = bar0.write_u32(base + off, val);
        };
        // IMEMC: BIT(25) = read, auto-increment
        w(freg::IMEMC, 1u32 << 25);
        let imem_first: Vec<u32> = (0..64).map(|_| r(freg::IMEMD)).collect();
        let imem_nonzero = imem_first
            .iter()
            .filter(|&&w| w != 0 && w != 0xDEAD_DEAD)
            .count();

        eprintln!("    IMEM[0..256]: {imem_nonzero}/64 non-zero words");
        if imem_nonzero > 0 {
            let first_nz: Vec<String> = imem_first
                .iter()
                .enumerate()
                .filter(|&(_, &w)| w != 0 && w != 0xDEAD_DEAD)
                .take(8)
                .map(|(i, w)| format!("[{:#05x}]={w:#010x}", i * 4))
                .collect();
            eprintln!("    First: {}", first_nz.join(" "));
        }

        // Also check end of IMEM (where BL lives)
        if imem_sz > 256 {
            let end_off = (imem_sz - 256) as u32;
            w(freg::IMEMC, 1u32 << 25 | end_off);
            let imem_end: Vec<u32> = (0..64).map(|_| r(freg::IMEMD)).collect();
            let end_nz = imem_end
                .iter()
                .filter(|&&w| w != 0 && w != 0xDEAD_DEAD)
                .count();
            eprintln!(
                "    IMEM[{end_off:#x}..{:#x}]: {end_nz}/64 non-zero words",
                end_off + 256
            );
            if end_nz > 0 {
                let end_str: Vec<String> = imem_end
                    .iter()
                    .enumerate()
                    .filter(|&(_, &w)| w != 0 && w != 0xDEAD_DEAD)
                    .take(4)
                    .map(|(i, w)| format!("[{:#07x}]={w:#010x}", end_off as usize + i * 4))
                    .collect();
                eprintln!("    End: {}", end_str.join(" "));
            }
        }

        // DMEM probe: first 64 words
        w(freg::DMEMC, 1u32 << 25);
        let dmem_first: Vec<u32> = (0..64).map(|_| r(freg::DMEMD)).collect();
        let dmem_nonzero = dmem_first
            .iter()
            .filter(|&&w| w != 0 && w != 0xDEAD_DEAD && w != 0xDEAD_5EC2)
            .count();
        let dmem_locked = dmem_first.contains(&0xDEAD_5EC2);

        eprintln!("    DMEM[0..256]: {dmem_nonzero}/64 non-zero locked={dmem_locked}");
        if dmem_nonzero > 0 {
            let first_dm: Vec<String> = dmem_first
                .iter()
                .enumerate()
                .filter(|&(_, &w)| w != 0 && w != 0xDEAD_DEAD && w != 0xDEAD_5EC2)
                .take(8)
                .map(|(i, w)| format!("[{:#05x}]={w:#010x}", i * 4))
                .collect();
            eprintln!("    First: {}", first_dm.join(" "));
        }

        (
            cpuctl,
            sctl,
            pc,
            exci,
            hwcfg,
            imem_nonzero,
            dmem_nonzero,
            hs,
            halted,
        )
    };

    // ── Phase 2: Full state dump ──
    eprintln!("\n── Phase 2: Post-Nouveau Falcon State ──");
    let (_sec2_cpuctl, _sec2_sctl, _sec2_pc, _sec2_exci, _, _, _, sec2_hs, _) =
        dump_falcon("SEC2", SEC2_BASE);
    let (
        fecs_cpuctl,
        _fecs_sctl,
        _fecs_pc,
        _fecs_exci,
        _fecs_hwcfg,
        fecs_imem_nz,
        _fecs_dmem_nz,
        fecs_hs,
        fecs_halted,
    ) = dump_falcon("FECS", FECS_BASE);
    let (
        _gpccs_cpuctl,
        _gpccs_sctl,
        _gpccs_pc,
        _gpccs_exci,
        _gpccs_hwcfg,
        gpccs_imem_nz,
        _gpccs_dmem_nz,
        gpccs_hs,
        gpccs_halted,
    ) = dump_falcon("GPCCS", GPCCS_BASE);

    // ── Phase 3: Summary and decision ──
    eprintln!("\n── Phase 3: Decision ──");
    let fecs_has_code = fecs_imem_nz > 4;
    let gpccs_has_code = gpccs_imem_nz > 4;
    eprintln!(
        "  FECS has firmware: {} ({fecs_imem_nz} IMEM words)",
        fecs_has_code
    );
    eprintln!(
        "  GPCCS has firmware: {} ({gpccs_imem_nz} IMEM words)",
        gpccs_has_code
    );
    eprintln!("  SEC2 HS: {sec2_hs} FECS HS: {fecs_hs} GPCCS HS: {gpccs_hs}");

    // ── Phase 4: Try STARTCPU on FECS ──
    if fecs_has_code && fecs_halted {
        eprintln!("\n── Phase 4: FECS STARTCPU ──");
        let w = |off: usize, val: u32| {
            let _ = bar0.write_u32(FECS_BASE + off, val);
        };
        let r = |off: usize| bar0.read_u32(FECS_BASE + off).unwrap_or(0xDEAD_DEAD);

        // Set BOOTVEC to 0 (standard entry point)
        w(freg::BOOTVEC, 0);
        w(freg::MAILBOX0, 0);
        w(freg::MAILBOX1, 0);

        eprintln!("  BOOTVEC=0, issuing STARTCPU...");
        w(freg::CPUCTL, freg::CPUCTL_STARTCPU);

        // Poll FECS for response
        let start = std::time::Instant::now();
        let mut last_pc = 0u32;
        let mut pc_trace = Vec::new();

        loop {
            std::thread::sleep(std::time::Duration::from_millis(5));
            let cpuctl = r(freg::CPUCTL);
            let pc = r(freg::PC);
            let mb0 = r(freg::MAILBOX0);
            let exci = r(freg::EXCI);

            if pc != last_pc {
                pc_trace.push(format!("{pc:#06x}@{}ms", start.elapsed().as_millis()));
                last_pc = pc;
            }

            let stopped = cpuctl & freg::CPUCTL_STOPPED != 0;
            let halted = cpuctl & freg::CPUCTL_HALTED != 0;

            if stopped || halted || mb0 != 0 || start.elapsed() > std::time::Duration::from_secs(2)
            {
                eprintln!("  cpuctl={cpuctl:#010x} PC={pc:#06x} EXCI={exci:#010x} mb0={mb0:#010x}");
                eprintln!(
                    "  STOPPED={stopped} HALTED={halted} ({}ms)",
                    start.elapsed().as_millis()
                );
                if !pc_trace.is_empty() {
                    eprintln!("  PC trace: [{}]", pc_trace.join(", "));
                }
                break;
            }
        }

        // Post-start state
        let post_sctl = r(freg::SCTL);
        let post_pc = r(freg::PC);
        let post_cpuctl = r(freg::CPUCTL);
        let post_mb0 = r(freg::MAILBOX0);
        let post_mb1 = r(freg::MAILBOX1);
        eprintln!("  Post-start: sctl={post_sctl:#010x} cpuctl={post_cpuctl:#010x}");
        eprintln!("  PC={post_pc:#06x} mb0={post_mb0:#010x} mb1={post_mb1:#010x}");

        let fecs_running = post_cpuctl & (freg::CPUCTL_STOPPED | freg::CPUCTL_HALTED) == 0;
        eprintln!("  FECS RUNNING: {fecs_running}");

        // If FECS is running (HALT and STOP bits clear), check GPCCS too
        if post_cpuctl & freg::CPUCTL_HALTED == 0 && post_cpuctl & freg::CPUCTL_STOPPED == 0 {
            eprintln!("\n  *** FECS STARTED! Checking GPCCS...");

            let gr = |off: usize| bar0.read_u32(GPCCS_BASE + off).unwrap_or(0xDEAD_DEAD);
            let gpccs_post = gr(freg::CPUCTL);
            let gpccs_pc_post = gr(freg::PC);
            let gpccs_exci_post = gr(freg::EXCI);
            eprintln!(
                "  GPCCS: cpuctl={gpccs_post:#010x} PC={gpccs_pc_post:#06x} EXCI={gpccs_exci_post:#010x}"
            );

            // Try FECS method: read GR engine status
            let pgraph = bar0.read_u32(0x400700).unwrap_or(0xDEAD);
            eprintln!("  PGRAPH_STATUS={pgraph:#010x}");

            // Try reading GR class
            let gr_class = bar0.read_u32(0x410004).unwrap_or(0xDEAD);
            eprintln!("  GR_CLASS={gr_class:#010x}");
        }
    } else if !fecs_has_code {
        eprintln!("\n── Phase 4: SKIPPED — FECS has no firmware in IMEM ──");
    } else {
        eprintln!("\n── Phase 4: SKIPPED — FECS HALT bit clear (cpuctl={fecs_cpuctl:#010x}) ──");
    }

    // ── Phase 5: Try STARTCPU on GPCCS ──
    if gpccs_has_code && gpccs_halted {
        eprintln!("\n── Phase 5: GPCCS STARTCPU ──");
        let w = |off: usize, val: u32| {
            let _ = bar0.write_u32(GPCCS_BASE + off, val);
        };
        let r = |off: usize| bar0.read_u32(GPCCS_BASE + off).unwrap_or(0xDEAD_DEAD);

        w(freg::BOOTVEC, 0);
        w(freg::MAILBOX0, 0);
        w(freg::MAILBOX1, 0);

        eprintln!("  BOOTVEC=0, issuing STARTCPU...");
        w(freg::CPUCTL, freg::CPUCTL_STARTCPU);
        std::thread::sleep(std::time::Duration::from_millis(100));

        let cpuctl = r(freg::CPUCTL);
        let pc = r(freg::PC);
        let exci = r(freg::EXCI);
        let sctl = r(freg::SCTL);
        eprintln!("  cpuctl={cpuctl:#010x} PC={pc:#06x} EXCI={exci:#010x} SCTL={sctl:#010x}");
    }

    // ── Phase 6: Final state summary ──
    eprintln!("\n── Phase 6: Final State ──");
    for (name, base) in [
        ("SEC2", SEC2_BASE),
        ("FECS", FECS_BASE),
        ("GPCCS", GPCCS_BASE),
    ] {
        let r = |off: usize| bar0.read_u32(base + off).unwrap_or(0xDEAD_DEAD);
        let cpuctl = r(freg::CPUCTL);
        let pc = r(freg::PC);
        let sctl = r(freg::SCTL);
        let mb0 = r(freg::MAILBOX0);
        eprintln!("  {name}: cpuctl={cpuctl:#010x} PC={pc:#06x} SCTL={sctl:#010x} mb0={mb0:#010x}");
    }

    eprintln!("\n=== Exp 099 Complete ===");
}
