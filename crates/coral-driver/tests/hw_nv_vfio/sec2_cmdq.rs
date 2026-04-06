// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::ember_client;
use crate::helpers::{init_tracing, open_vfio, vfio_bdf};

#[test]
#[ignore = "requires VFIO-bound GPU hardware"]
fn vfio_sec2_cmdq_probe() {
    init_tracing();

    eprintln!("\n=== Exp 089b: SEC2 CMDQ/MSGQ Ring Probe ===\n");

    // Phase -1: RAW BAR0 state before ANY GR init
    // Open VFIO device directly (no NvVfioComputeDevice) to get raw BAR0
    let bdf = vfio_bdf();
    let raw_bar0 = {
        let fds = match ember_client::request_fds(&bdf) {
            Ok(f) => {
                eprintln!("ember: received raw VFIO fds for {bdf}");
                f
            }
            Err(e) => {
                panic!("ember unavailable: {e}");
            }
        };
        let vfio_dev = coral_driver::vfio::VfioDevice::from_received(&bdf, fds)
            .expect("VfioDevice::from_received");
        let bar0 = vfio_dev.map_bar(0).expect("map_bar(0)");

        // Read GPCCS/FECS/SEC2 state BEFORE any GR init writes
        let r = |addr: usize| bar0.read_u32(addr).unwrap_or(0xBAD0_0000);

        const FECS: usize = 0x409000;
        const GPCCS: usize = 0x41A000;
        const SEC2: usize = 0x087000;

        eprintln!("Phase -1: RAW state (no GR init):");
        eprintln!(
            "  FECS:  cpuctl={:#010x} pc={:#010x} sctl={:#010x}",
            r(FECS + 0x100),
            r(FECS + 0x030),
            r(FECS + 0x240)
        );
        eprintln!(
            "  GPCCS: cpuctl={:#010x} pc={:#010x} sctl={:#010x} exci={:#010x}",
            r(GPCCS + 0x100),
            r(GPCCS + 0x030),
            r(GPCCS + 0x240),
            r(GPCCS + 0x148)
        );
        eprintln!(
            "  SEC2:  cpuctl={:#010x} pc={:#010x} mb0={:#010x} mb1={:#010x}",
            r(SEC2 + 0x100),
            r(SEC2 + 0x030),
            r(SEC2 + 0x040),
            r(SEC2 + 0x044)
        );

        // GPCCS CPUCTL write test
        let _ = bar0.write_u32(GPCCS + 0x100, 0x20);
        let gpccs_cpuctl_wb = r(GPCCS + 0x100);
        eprintln!(
            "  GPCCS CPUCTL write test: wrote 0x20, read {gpccs_cpuctl_wb:#010x} — {}",
            if gpccs_cpuctl_wb & 0x20 != 0 {
                "WRITABLE"
            } else {
                "LOCKED"
            }
        );

        (r(GPCCS + 0x030), r(GPCCS + 0x100), r(GPCCS + 0x148))
    };
    let (_raw_gpccs_pc, _raw_gpccs_cpuctl, _raw_gpccs_exci) = raw_bar0;

    // Phase 0: Now open with full GR init
    eprintln!("\nPhase 0: Opening NvVfioComputeDevice (runs apply_gr_bar0_init)...");
    let dev = open_vfio();

    let bar0 = dev.bar0_ref();
    let r = |addr: usize| bar0.read_u32(addr).unwrap_or(0xBAD0_0000);

    const SEC2: usize = 0x087000;
    const FECS: usize = 0x409000;
    const GPCCS: usize = 0x41A000;

    // SEC2 basic state — HWCFG is at falcon+0x108, not +0x008
    let cpuctl = r(SEC2 + 0x100);
    let pc = r(SEC2 + 0x030);
    let mb0 = r(SEC2 + 0x040);
    let mb1 = r(SEC2 + 0x044);
    let hwcfg = r(SEC2 + 0x108);
    let sctl = r(SEC2 + 0x240);
    let dmem_sz = ((hwcfg >> 9) & 0x1FF) << 8;
    eprintln!("SEC2: cpuctl={cpuctl:#010x} pc={pc:#010x} sctl={sctl:#010x}");
    eprintln!("  mb0={mb0:#010x} mb1={mb1:#010x} hwcfg={hwcfg:#010x} dmem={dmem_sz}B");
    eprintln!(
        "  imem={}B sec_mode={}",
        (hwcfg & 0x1FF) << 8,
        (hwcfg >> 8) & 1
    );

    // CMDQ/MSGQ registers (from nouveau gp102_sec2_flcn)
    // .cmdq = { 0xa00, 0xa04, 8 }, .msgq = { 0xa30, 0xa34, 8 }
    eprintln!("\nCMDQ/MSGQ registers:");
    for idx in 0..2u32 {
        let ch = r(SEC2 + 0xa00 + (idx as usize) * 8);
        let ct = r(SEC2 + 0xa04 + (idx as usize) * 8);
        let mh = r(SEC2 + 0xa30 + (idx as usize) * 8);
        let mt = r(SEC2 + 0xa34 + (idx as usize) * 8);
        eprintln!(
            "  CMDQ[{idx}]: head={ch:#010x} tail={ct:#010x} {}",
            if ch == ct { "EMPTY" } else { "HAS DATA" }
        );
        eprintln!(
            "  MSGQ[{idx}]: head={mh:#010x} tail={mt:#010x} {}",
            if mh == mt { "EMPTY" } else { "HAS DATA" }
        );
    }

    let cmdq_head = r(SEC2 + 0xa00);
    let cmdq_tail = r(SEC2 + 0xa04);
    let msgq_head = r(SEC2 + 0xa30);
    let msgq_tail = r(SEC2 + 0xa34);

    // Read DMEM around queue positions via PIO
    let dmem_read = |addr: usize| -> u32 {
        let ctrl = (1u32 << 25) | ((addr as u32) & 0xFFFC);
        let _ = bar0.write_u32(SEC2 + 0x1c0, ctrl); // DMEMC port 0
        r(SEC2 + 0x1c4) // DMEMD port 0
    };

    let dmem_read_block = |start: usize, count: usize| -> Vec<u32> {
        let ctrl = (1u32 << 25) | ((start as u32) & 0xFFFC);
        let _ = bar0.write_u32(SEC2 + 0x1c0, ctrl);
        (0..count).map(|_| r(SEC2 + 0x1c4)).collect()
    };

    // Scan DMEM for non-zero regions (full DMEM)
    eprintln!("\nDMEM non-zero scan (full {dmem_sz}B):");
    let scan_end = dmem_sz as usize;
    let mut ranges: Vec<(usize, usize)> = Vec::new();
    let mut in_range = false;
    let mut rstart = 0;
    for off in (0..scan_end).step_by(4) {
        let val = dmem_read(off);
        if val != 0 && val != 0xDEAD_DEAD {
            if !in_range {
                rstart = off;
                in_range = true;
            }
        } else if in_range {
            ranges.push((rstart, off));
            in_range = false;
        }
    }
    if in_range {
        ranges.push((rstart, scan_end));
    }

    for &(s, e) in &ranges {
        let words = dmem_read_block(s, ((e - s) / 4).min(16));
        eprintln!("  [{s:#06x}..{e:#06x}] ({} bytes):", e - s);
        for (i, &w) in words.iter().enumerate() {
            let addr = s + i * 4;
            let mut markers = String::new();
            if addr == cmdq_head as usize {
                markers += " <-- CMDQ_HEAD";
            }
            if addr == cmdq_tail as usize {
                markers += " <-- CMDQ_TAIL";
            }
            if addr == msgq_head as usize {
                markers += " <-- MSGQ_HEAD";
            }
            if addr == msgq_tail as usize {
                markers += " <-- MSGQ_TAIL";
            }
            eprintln!("    [{addr:#06x}] = {w:#010x}{markers}");
        }
    }

    // Try to read init message structure at DMEM start
    // nv_sec2_init_msg: hdr(4) + msg_type(1) + num_queues(1) + os_debug(2)
    //                   + queue_info[2]{ offset(4)+size(2)+index(1)+id(1) } = 24 bytes
    eprintln!("\nLooking for init message remnants in DMEM[0..64]:");
    let init_words = dmem_read_block(0, 16);
    for (i, &w) in init_words.iter().enumerate() {
        if w != 0 {
            eprintln!("  [{:#06x}] = {w:#010x}", i * 4);
        }
    }

    // FECS/GPCCS state — try multiple HWCFG offsets
    eprintln!("\nFECS:");
    eprintln!(
        "  cpuctl={:#010x} pc={:#010x}",
        r(FECS + 0x100),
        r(FECS + 0x030)
    );
    eprintln!(
        "  hwcfg candidates: +0x008={:#010x} +0x108={:#010x} +0x908={:#010x}",
        r(FECS + 0x008),
        r(FECS + 0x108),
        r(FECS + 0x908)
    );
    eprintln!("GPCCS:");
    eprintln!(
        "  cpuctl={:#010x} pc={:#010x} sctl={:#010x}",
        r(GPCCS + 0x100),
        r(GPCCS + 0x030),
        r(GPCCS + 0x240)
    );
    eprintln!(
        "  hwcfg candidates: +0x008={:#010x} +0x108={:#010x} +0x908={:#010x}",
        r(GPCCS + 0x008),
        r(GPCCS + 0x108),
        r(GPCCS + 0x908)
    );
    let _gpccs_hwcfg = r(GPCCS + 0x108);

    // PMC and GR power/enable state
    let pmc_enable = r(0x000200);
    let pmc_device_enable = r(0x000600);
    let gr_enable = r(0x400500);
    let gr_status = r(0x400700);
    let pgraph_intr = r(0x400100);
    eprintln!("\nPMC/GR enable state:");
    eprintln!("  PMC_ENABLE (0x200) = {pmc_enable:#010x}");
    eprintln!("  PMC_DEVICE (0x600) = {pmc_device_enable:#010x}");
    eprintln!("  GR_ENABLE (0x400500) = {gr_enable:#010x}");
    eprintln!("  GR_STATUS (0x400700) = {gr_status:#010x}");
    eprintln!("  PGRAPH_INTR (0x400100) = {pgraph_intr:#010x}");

    // GPC configuration
    eprintln!("\nGPC configuration:");
    for gpc in 0..6u32 {
        let base = 0x502000 + gpc as usize * 0x8000;
        let ctrl = r(base);
        if ctrl != 0xBAD0_0000 && ctrl != 0xBADF_5040 {
            eprintln!("  GPC[{gpc}] @ {base:#08x}: ctrl={ctrl:#010x}");
        }
    }

    // Also check GPC TPC count registers
    eprintln!("\nGPC-GPCCS unit registers (0x41B000 range):");
    for off in (0x000..0x100).step_by(4) {
        let addr = 0x41B000 + off;
        let val = r(addr);
        if val != 0 && val != 0xBAD0_0000 && val != 0xBADF_5040 {
            eprintln!("  [{addr:#08x}] = {val:#010x}");
        }
    }

    // Deep GPCCS security/boot analysis
    {
        eprintln!("\nGPCCS deep boot analysis:");
        let hwcfg = r(GPCCS + 0x108);
        let imem_sz = (hwcfg & 0x1FF) << 8;
        let dmem_sz = ((hwcfg >> 9) & 0x1FF) << 8;
        let sec_mode = (hwcfg >> 8) & 1;
        eprintln!("  hwcfg={hwcfg:#010x} imem={imem_sz}B dmem={dmem_sz}B sec_mode={sec_mode}");
        eprintln!(
            "  cpuctl={:#010x} bootvec={:#010x}",
            r(GPCCS + 0x100),
            r(GPCCS + 0x104)
        );
        eprintln!(
            "  sctl={:#010x} dmactl={:#010x}",
            r(GPCCS + 0x240),
            r(GPCCS + 0x10C)
        );
        eprintln!(
            "  exci={:#010x} tracepc={:#010x}",
            r(GPCCS + 0x148),
            r(GPCCS + 0x030)
        );
        eprintln!(
            "  intr={:#010x} intr_en={:#010x}",
            r(GPCCS),
            r(GPCCS + 0x00c)
        );

        // Mailboxes
        eprintln!(
            "  mb0={:#010x} mb1={:#010x}",
            r(GPCCS + 0x040),
            r(GPCCS + 0x044)
        );

        // Check if GPCCS firmware is valid by reading IMEM via PIO
        let _ = bar0.write_u32(GPCCS + 0x180, 0x0200_0000); // IMEMC: read, addr=0
        let mut imem_words = Vec::new();
        for _ in 0..8 {
            imem_words.push(r(GPCCS + 0x184));
        }
        let all_zero = imem_words.iter().all(|&w| w == 0);
        let all_bad = imem_words
            .iter()
            .all(|&w| w == 0xBAD0_0000 || w == 0xBADF_5040);
        eprintln!(
            "  IMEM[0x0000..0x0020]: {:?}",
            imem_words
                .iter()
                .map(|w| format!("{w:#010x}"))
                .collect::<Vec<_>>()
        );
        if all_zero {
            eprintln!("  ** GPCCS IMEM[0] IS ALL ZEROS — firmware not loaded at addr 0 **");
        }
        if all_bad {
            eprintln!("  ** GPCCS IMEM reads return PRI error — HS mode blocks reads **");
        }

        // Exp 091: Read IMEM at BOOTVEC target (0x3400) — this is where
        // ACR places the GPCCS bootloader (start_tag=0x34 → 0x3400).
        // If BOOTVEC=0 but firmware is at 0x3400, that's the L10 root cause.
        let bl_imem_off = 0x3400u32;
        let imemc_val = 0x0200_0000 | (bl_imem_off & 0xFFFC);
        let _ = bar0.write_u32(GPCCS + 0x180, imemc_val);
        let mut imem_bl_words = Vec::new();
        for _ in 0..8 {
            imem_bl_words.push(r(GPCCS + 0x184));
        }
        let bl_all_zero = imem_bl_words.iter().all(|&w| w == 0);
        eprintln!(
            "  IMEM[0x3400..0x3420]: {:?}",
            imem_bl_words
                .iter()
                .map(|w| format!("{w:#010x}"))
                .collect::<Vec<_>>()
        );
        if !bl_all_zero && all_zero {
            eprintln!("  ** FIRMWARE AT 0x3400, NOTHING AT 0x0000 — BOOTVEC MUST BE 0x3400! **");
        } else if bl_all_zero {
            eprintln!("  ** IMEM[0x3400] also empty — BL may not have been loaded by ACR **");
        }

        // Read DMEM too
        let ctrl = 1u32 << 25;
        let _ = bar0.write_u32(GPCCS + 0x1c0, ctrl);
        let mut dmem_words = Vec::new();
        for _ in 0..8 {
            dmem_words.push(r(GPCCS + 0x1c4));
        }
        eprintln!(
            "  DMEM[0..32]: {:?}",
            dmem_words
                .iter()
                .map(|w| format!("{w:#010x}"))
                .collect::<Vec<_>>()
        );

        // Compare with FECS for reference
        eprintln!("\n  FECS reference:");
        eprintln!(
            "  hwcfg={:#010x} sctl={:#010x}",
            r(FECS + 0x108),
            r(FECS + 0x240)
        );
        let _ = bar0.write_u32(FECS + 0x180, 0x0200_0000);
        let mut fecs_imem = Vec::new();
        for _ in 0..4 {
            fecs_imem.push(r(FECS + 0x184));
        }
        eprintln!(
            "  IMEM[0..16]: {:?}",
            fecs_imem
                .iter()
                .map(|w| format!("{w:#010x}"))
                .collect::<Vec<_>>()
        );
    }

    // === Phase 2: Try to unstick GPCCS ===
    {
        eprintln!("\n=== Phase 2: GPCCS restart experiments ===");
        let gpccs_pc = r(GPCCS + 0x030);
        let gpccs_exci = r(GPCCS + 0x148);
        eprintln!("  Before: pc={gpccs_pc:#010x} exci={gpccs_exci:#010x}");

        // Experiment A: LS mode escape + CPUCTL unlock attempts
        eprintln!("\n  Exp A: Security register probing");

        // Read all security-related registers for GPCCS and FECS
        for (name, base) in [("FECS", FECS), ("GPCCS", GPCCS)] {
            let sctl = r(base + 0x240);
            let cpuctl = r(base + 0x100);
            let cpuctl_alias = r(base + 0x130); // CPUCTL_ALIAS on falcon v5+
            eprintln!(
                "    {name}: sctl={sctl:#010x} cpuctl={cpuctl:#010x} cpuctl_alias={cpuctl_alias:#010x}"
            );

            // Extended security registers
            for (rname, off) in [
                ("DEBUG", 0x090usize),
                ("SCTL_H", 0x244),
                ("HWCFG2", 0x10C),
                ("DMACTL", 0x10C),
                ("OS", 0x080),
            ] {
                let val = r(base + off);
                eprintln!("      {rname}({off:#05x}) = {val:#010x}");
            }
        }

        // Try SCTL write to clear LS mode on GPCCS
        eprintln!("\n    Attempting to clear GPCCS LS mode...");
        let gpccs_sctl_before = r(GPCCS + 0x240);
        let _ = bar0.write_u32(GPCCS + 0x240, 0); // try clearing SCTL
        let gpccs_sctl_after = r(GPCCS + 0x240);
        eprintln!(
            "      SCTL: {gpccs_sctl_before:#010x} -> {gpccs_sctl_after:#010x} {}",
            if gpccs_sctl_after != gpccs_sctl_before {
                "CHANGED!"
            } else {
                "(locked)"
            }
        );

        // Try CPUCTL_ALIAS (0x130) for STARTCPU
        eprintln!("\n    Attempting GPCCS start via CPUCTL_ALIAS (0x130)...");
        let _ = bar0.write_u32(GPCCS + 0x130, 0x02); // STARTCPU via alias
        std::thread::sleep(std::time::Duration::from_millis(50));
        let gpccs_pc = r(GPCCS + 0x030);
        let gpccs_cpuctl = r(GPCCS + 0x100);
        eprintln!("      cpuctl={gpccs_cpuctl:#010x} pc={gpccs_pc:#010x}");

        // Try PMC GR engine reset to break LS mode
        eprintln!("\n    Attempting PMC GR reset to break LS mode...");
        let pmc = r(0x200);
        let _ = bar0.write_u32(0x200, pmc & !(1u32 << 12)); // disable GR
        std::thread::sleep(std::time::Duration::from_millis(20));
        let _ = bar0.write_u32(0x200, pmc | (1u32 << 12)); // re-enable GR
        std::thread::sleep(std::time::Duration::from_millis(50));
        let gpccs_sctl_post = r(GPCCS + 0x240);
        let gpccs_cpuctl_post = r(GPCCS + 0x100);
        let gpccs_pc_post = r(GPCCS + 0x030);
        let fecs_sctl_post = r(FECS + 0x240);
        let fecs_cpuctl_post = r(FECS + 0x100);
        let fecs_pc_post = r(FECS + 0x030);
        eprintln!("      After GR reset:");
        eprintln!(
            "      GPCCS: sctl={gpccs_sctl_post:#010x} cpuctl={gpccs_cpuctl_post:#010x} pc={gpccs_pc_post:#010x}"
        );
        eprintln!(
            "      FECS:  sctl={fecs_sctl_post:#010x} cpuctl={fecs_cpuctl_post:#010x} pc={fecs_pc_post:#010x}"
        );

        // If GR reset cleared LS mode, try STARTCPU
        if gpccs_sctl_post != 0x3000 || gpccs_cpuctl_post != 0 {
            eprintln!("      ** LS mode changed! Trying STARTCPU... **");
            let _ = bar0.write_u32(GPCCS + 0x104, 0); // BOOTVEC
            let _ = bar0.write_u32(GPCCS + 0x100, 0x02); // STARTCPU
            std::thread::sleep(std::time::Duration::from_millis(100));
            let pc = r(GPCCS + 0x030);
            let cpuctl = r(GPCCS + 0x100);
            let sctl = r(GPCCS + 0x240);
            eprintln!(
                "      After STARTCPU: cpuctl={cpuctl:#010x} pc={pc:#010x} sctl={sctl:#010x}"
            );
        }

        // Experiment B: Try with BOOTVEC from GPCCS firmware header
        // GPCCS BL start_tag=0x34 → boot at IMEM 0x3400
        // But the actual app code might start at a different offset
        eprintln!("\n  Exp B: HRESET + BOOTVEC=0x3400 + STARTCPU (gpccs_bl start_tag)");
        let _ = bar0.write_u32(GPCCS + 0x100, 0x20); // HRESET
        std::thread::sleep(std::time::Duration::from_millis(10));
        let _ = bar0.write_u32(GPCCS + 0x104, 0x3400); // BOOTVEC
        let _ = bar0.write_u32(GPCCS + 0x100, 0x02); // STARTCPU
        std::thread::sleep(std::time::Duration::from_millis(50));
        eprintln!(
            "    cpuctl={:#010x} pc={:#010x} exci={:#010x}",
            r(GPCCS + 0x100),
            r(GPCCS + 0x030),
            r(GPCCS + 0x148)
        );

        // Experiment C: Try app_code entry point (typically small offset)
        // nouveau sets BOOTVEC to code_entry_point from BLD
        eprintln!("\n  Exp C: HRESET + BOOTVEC=0x0000 (standard) + STARTCPU");
        let _ = bar0.write_u32(GPCCS + 0x100, 0x20);
        std::thread::sleep(std::time::Duration::from_millis(10));
        // Read DMEM for BLD hints (code_entry_point at offset 24 in flcn_bl_dmem_desc)
        let ctrl = 1u32 << 25;
        let _ = bar0.write_u32(GPCCS + 0x1c0, ctrl);
        let mut dmem0 = Vec::new();
        for _ in 0..32 {
            dmem0.push(r(GPCCS + 0x1c4));
        }
        eprintln!("    GPCCS DMEM[0..128]:");
        for (i, &w) in dmem0.iter().enumerate() {
            if w != 0 {
                eprintln!("      [{:#06x}] = {w:#010x}", i * 4);
            }
        }

        // Try STARTCPU with standard BOOTVEC=0
        let _ = bar0.write_u32(GPCCS + 0x104, 0);
        let _ = bar0.write_u32(GPCCS + 0x100, 0x02);
        std::thread::sleep(std::time::Duration::from_millis(100));
        let gpccs_pc = r(GPCCS + 0x030);
        let gpccs_cpuctl = r(GPCCS + 0x100);
        let gpccs_exci = r(GPCCS + 0x148);
        eprintln!("    cpuctl={gpccs_cpuctl:#010x} pc={gpccs_pc:#010x} exci={gpccs_exci:#010x}");

        // PC sampling
        let mut pcs = Vec::new();
        for _ in 0..5 {
            std::thread::sleep(std::time::Duration::from_millis(20));
            pcs.push(r(GPCCS + 0x030));
        }
        eprintln!(
            "    PCs: {:?}",
            pcs.iter().map(|p| format!("{p:#06x}")).collect::<Vec<_>>()
        );

        // Also check FECS PC — did our GPCCS restarts affect FECS?
        let fecs_pc = r(FECS + 0x030);
        eprintln!("    FECS pc={fecs_pc:#010x}");
    }

    // === Phase 3: Try to read SEC2 init message from DMEM ===
    // The init message structure (nv_sec2_init_msg):
    //   nvfw_falcon_msg hdr: { u8 unit_id, u8 size, u8 ctrl_flags, u8 seq_id } = 4B
    //   u8 msg_type (0x00 = INIT)
    //   u8 num_queues
    //   u16 os_debug_entry_point
    //   queue_info[2] { u32 offset, u16 size, u8 index, u8 id } = 8B each
    //   u32 sw_managed_area_offset
    //   u16 sw_managed_area_size
    //
    // Total: 4 + 4 + 16 + 6 = 30 bytes
    //
    // The init msg is written to MSGQ in DMEM. MSGQ tail_reg points to where
    // the host should read. Since nobody read it, the message should still be there.
    {
        eprintln!("\n=== Phase 3: SEC2 Init Message Recovery ===");

        // The MSGQ head/tail registers are at SEC2 + 0xa30/0xa34
        // But they read as 0. Try reading from alternate offsets.
        eprintln!("  MSGQ head/tail register scan:");
        for off in (0xa00..0xa60).step_by(4) {
            let val = r(SEC2 + off);
            if val != 0 && val != 0xBADF_5040 && val != 0xBAD0_0000 {
                eprintln!("    SEC2+{off:#05x} = {val:#010x}");
            }
        }

        // The init message is the FIRST thing SEC2 writes after booting.
        // On nouveau, it's consumed by the IRQ handler via gp102_sec2_initmsg().
        // Since we never ran the IRQ handler, the message should be in DMEM.
        //
        // SEC2 firmware writes the init msg at the MSGQ start offset.
        // The MSGQ start offset is typically at the end of the used DMEM area.
        //
        // From our DMEM scan: last non-zero region before stack is at 0x0f20-0x0f30.
        // The init message might be nearby.
        //
        // Let's scan for the init message signature:
        // First word: unit_id=0x01 (NV_SEC2_UNIT_INIT), size=~30, ctrl_flags=0, seq_id=0
        // In little-endian: 0x00001E01 (size=30) or similar
        //
        // Actually: let me search for msg_type=0x00 + num_queues=0x02 at bytes 4-5

        eprintln!("  Scanning FULL SEC2 DMEM for init message (unit_id=0x01)...");
        let scan_end = dmem_sz as usize; // full 64KB

        let dmem_read = |addr: usize| -> u32 {
            let ctrl = (1u32 << 25) | ((addr as u32) & 0xFFFC);
            let _ = bar0.write_u32(SEC2 + 0x1c0, ctrl);
            r(SEC2 + 0x1c4)
        };

        let dmem_read_block = |start: usize, count: usize| -> Vec<u32> {
            let ctrl = (1u32 << 25) | ((start as u32) & 0xFFFC);
            let _ = bar0.write_u32(SEC2 + 0x1c0, ctrl);
            (0..count).map(|_| r(SEC2 + 0x1c4)).collect()
        };

        let mut found_init = false;
        for off in (0..scan_end).step_by(4) {
            let w = dmem_read(off);
            let unit_id = w & 0xFF;
            let size = (w >> 8) & 0xFF;
            // Init message: unit_id=1, size=28-40 (depends on version)
            if unit_id == 0x01 && (24..=48).contains(&size) {
                let w1 = dmem_read(off + 4);
                let msg_type = w1 & 0xFF;
                let num_queues = (w1 >> 8) & 0xFF;
                if msg_type == 0x00 && num_queues == 2 {
                    eprintln!("  ** FOUND INIT MESSAGE at DMEM[{off:#06x}]! **");
                    let block = dmem_read_block(off, 12);
                    for (i, &bw) in block.iter().enumerate() {
                        eprintln!("    [{:#06x}] = {bw:#010x}", off + i * 4);
                    }

                    // Parse queue_info[2] starting at offset 8 from msg start
                    // queue_info[0]: { u32 offset, u16 size, u8 index, u8 id }
                    let q0_offset = block[2]; // offset 8
                    let q0_size_idx_id = block[3]; // offset 12
                    let q0_size = q0_size_idx_id & 0xFFFF;
                    let q0_index = (q0_size_idx_id >> 16) & 0xFF;
                    let q0_id = (q0_size_idx_id >> 24) & 0xFF;

                    let q1_offset = block[4]; // offset 16
                    let q1_size_idx_id = block[5]; // offset 20
                    let q1_size = q1_size_idx_id & 0xFFFF;
                    let q1_index = (q1_size_idx_id >> 16) & 0xFF;
                    let q1_id = (q1_size_idx_id >> 24) & 0xFF;

                    let id_name = |id: u32| match id {
                        0 => "CMDQ",
                        1 => "MSGQ",
                        _ => "???",
                    };
                    eprintln!(
                        "\n    Queue[0]: id={} offset={q0_offset:#010x} size={q0_size} index={q0_index}",
                        id_name(q0_id)
                    );
                    eprintln!(
                        "    Queue[1]: id={} offset={q1_offset:#010x} size={q1_size} index={q1_index}",
                        id_name(q1_id)
                    );

                    found_init = true;
                    break;
                }
            }
        }

        if !found_init {
            eprintln!("  Init message NOT found in DMEM[0..{scan_end:#x}]");
            eprintln!("  SEC2 may not have completed init (no init msg written)");
        }
    }

    // === Phase 4: Attempt CMDQ bootstrap if queue looks valid ===
    if cmdq_head != 0 || cmdq_tail != 0 || msgq_head != 0 || msgq_tail != 0 {
        eprintln!("\n=== Phase 2: CMDQ BOOTSTRAP_FALCON(GPCCS) ===");

        let write_pos = cmdq_head as usize;
        // Build nv_sec2_acr_bootstrap_falcon_cmd (16 bytes / 4 words)
        // Word 0: unit_id=0x08, size=0x10, ctrl_flags=0x03, seq_id=0x00
        // Word 1: cmd_type=0x00, pad, pad, pad
        // Word 2: flags=0x00000000 (RESET_YES)
        // Word 3: falcon_id=0x00000003 (GPCCS)
        let cmd_words: [u32; 4] = [
            0x0003_1008, // seq=0, ctrl=0x03(STATUS|INTR), size=0x10, unit=0x08(ACR)
            0x0000_0000, // cmd_type=0x00(BOOTSTRAP_FALCON) + padding
            0x0000_0000, // flags=RESET_YES
            0x0000_0003, // falcon_id=GPCCS
        ];

        eprintln!("  Writing cmd to DMEM[{write_pos:#06x}]:");
        for (i, &w) in cmd_words.iter().enumerate() {
            eprintln!("    [{:#06x}] = {w:#010x}", write_pos + i * 4);
        }

        // Write command to DMEM
        let ctrl = (1u32 << 24) | ((write_pos as u32) & 0xFFFC);
        let _ = bar0.write_u32(SEC2 + 0x1c0, ctrl);
        for &w in &cmd_words {
            let _ = bar0.write_u32(SEC2 + 0x1c4, w);
        }

        // Verify
        let verify = dmem_read_block(write_pos, 4);
        let ok = verify == cmd_words.to_vec();
        eprintln!("  Verify: {ok}");

        // Advance CMDQ head
        let new_head = (write_pos + 16) as u32;
        eprintln!("  Advancing CMDQ head: {cmdq_head:#010x} -> {new_head:#010x}");
        let _ = bar0.write_u32(SEC2 + 0xa00, new_head);
        let readback = r(SEC2 + 0xa00);
        eprintln!("  CMDQ head readback: {readback:#010x}");

        // Poke SEC2 interrupt (IRQSSET bit 6)
        eprintln!("  Poking SEC2 IRQ...");
        let _ = bar0.write_u32(SEC2, 0x40);

        // Wait for processing
        eprintln!("  Waiting 500ms...");
        std::thread::sleep(std::time::Duration::from_millis(500));

        // Check MSGQ for response
        let new_mh = r(SEC2 + 0xa30);
        let new_mt = r(SEC2 + 0xa34);
        eprintln!("  MSGQ after: head={new_mh:#010x} tail={new_mt:#010x}");
        if new_mh != new_mt {
            eprintln!("  ** MSGQ has response! Reading... **");
            let resp = dmem_read_block(new_mt as usize, 8);
            for (i, &w) in resp.iter().enumerate() {
                eprintln!("    [{:#06x}] = {w:#010x}", new_mt as usize + i * 4);
            }
        }

        // Check CMDQ tail (did SEC2 consume our command?)
        let new_ct = r(SEC2 + 0xa04);
        eprintln!("  CMDQ tail after: {new_ct:#010x} (was {cmdq_tail:#010x})");
        if new_ct != cmdq_tail {
            eprintln!("  ** SEC2 consumed the command! **");
        }

        // Check GPCCS state
        let gpccs_cpuctl = r(GPCCS + 0x100);
        let gpccs_pc = r(GPCCS + 0x030);
        let gpccs_sctl = r(GPCCS + 0x240);
        eprintln!("\n  GPCCS after CMDQ bootstrap:");
        eprintln!("    cpuctl={gpccs_cpuctl:#010x} pc={gpccs_pc:#010x} sctl={gpccs_sctl:#010x}");

        if gpccs_pc != 0 {
            eprintln!("  ****************************************************");
            eprintln!("  *  GPCCS IS RUNNING! PC advanced from 0x0000!       *");
            eprintln!("  ****************************************************");
        }

        // Also check FECS — it might advance if GPCCS came alive
        let fecs_pc = r(FECS + 0x030);
        eprintln!("  FECS pc={fecs_pc:#010x}");

        // PC sampling
        let mut gpccs_pcs = Vec::new();
        let mut fecs_pcs = Vec::new();
        for _ in 0..5 {
            std::thread::sleep(std::time::Duration::from_millis(20));
            gpccs_pcs.push(r(GPCCS + 0x030));
            fecs_pcs.push(r(FECS + 0x030));
        }
        eprintln!(
            "  GPCCS PCs: {:?}",
            gpccs_pcs
                .iter()
                .map(|p| format!("{p:#06x}"))
                .collect::<Vec<_>>()
        );
        eprintln!(
            "  FECS  PCs: {:?}",
            fecs_pcs
                .iter()
                .map(|p| format!("{p:#06x}"))
                .collect::<Vec<_>>()
        );
    } else {
        eprintln!("\nCMDQ/MSGQ registers all zero — queue not initialized.");
        eprintln!("SEC2 may need its init message replayed first.");
    }

    // SEC2 final state
    let sec2_pc = r(SEC2 + 0x030);
    let sec2_mb0 = r(SEC2 + 0x040);
    eprintln!("\nSEC2 final: pc={sec2_pc:#010x} mb0={sec2_mb0:#010x}");

    eprintln!("\n=== End Exp 089b ===");
}
