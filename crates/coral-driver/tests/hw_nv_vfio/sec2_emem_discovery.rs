// SPDX-License-Identifier: AGPL-3.0-only

//! Exp 097: EMEM discovery + MSI wake after HS boot.
//!
//! After Exp 096 revealed that DMEM is locked in HS mode (returns 0xDEAD5EC2),
//! this test explores the EMEM back-channel and MSI interrupt path to establish
//! SEC2 conversation.
//!
//! Phase 1: Nouveau cycle → sysmem ACR boot (achieve HS)
//! Phase 2: Full EMEM scan (scan for init message pattern in EMEM)
//! Phase 3: ARM MSI IRQ + test STARTCPU on STOPPED SEC2
//! Phase 4: Write CMDQ tail + poke IRQSSET to test SEC2 responsiveness

use crate::ember_client;
use crate::helpers::{init_tracing, vfio_bdf};
use coral_driver::nv::vfio_compute::acr_boot::sec2_queue::{Sec2QueueInfo, Sec2Queues};

const SEC2_BASE: usize = 0x087000;

mod falcon_reg {
    pub const IRQSSET: usize = 0x000;
    pub const IRQSTAT: usize = 0x008;
    pub const IRQMASK: usize = 0x018;
    pub const IRQMSET: usize = 0x010;
    pub const CPUCTL: usize = 0x100;
    pub const CPUCTL_ALIAS: usize = 0x130;
    pub const _BOOTVEC: usize = 0x104;
    pub const HWCFG: usize = 0x108;
    pub const SCTL: usize = 0x240;
    pub const EXCI: usize = 0x148;
    pub const PC: usize = 0x030;
    pub const MAILBOX0: usize = 0x040;
    pub const MAILBOX1: usize = 0x044;
    pub const DMEMC: usize = 0x1C0;
    pub const DMEMD: usize = 0x1C4;
    pub const EMEMC0: usize = 0xAC0;
    pub const EMEMD0: usize = 0xAC4;

    pub const CPUCTL_STARTCPU: u32 = 1 << 1;
    pub const CPUCTL_HRESET: u32 = 1 << 4;
    pub const CPUCTL_HALTED: u32 = 1 << 5;

    pub fn dmem_size_bytes(hwcfg: u32) -> usize {
        (((hwcfg >> 9) & 0x1FF) as usize) << 8
    }
    pub fn imem_size_bytes(hwcfg: u32) -> usize {
        ((hwcfg & 0x1FF) as usize) << 8
    }
}
use falcon_reg as falcon;

/// Exp 098 Path O: nouveau cycle → direct full sysmem ACR (no prior boot solver).
///
/// Critical: runs full-init on a CLEAN device, not after the boot solver.
/// The boot solver's HS state prevents re-binding.
#[test]
#[ignore = "requires VFIO-bound GPU hardware"]
fn sec2_full_init_conversation() {
    init_tracing();

    eprintln!("\n=== Exp 098: Full Init Conversation (Path O) ===\n");

    let bdf = vfio_bdf();

    // Phase 1: Nouveau cycle via GlowPlug — fresh DEVINIT + signed firmware
    eprintln!("Phase 1: Nouveau cycle (GlowPlug swap)...");
    {
        let mut gp =
            crate::glowplug_client::GlowPlugClient::connect().expect("GlowPlug connection");

        match gp.swap(&bdf, "nouveau") {
            Ok(r) => eprintln!("  swap→nouveau: {r}"),
            Err(e) => {
                eprintln!("  swap→nouveau FAILED: {e} — continuing without cycle");
            }
        }
        std::thread::sleep(std::time::Duration::from_secs(3));

        match gp.swap(&bdf, "vfio-pci") {
            Ok(r) => eprintln!("  swap→vfio-pci: {r}"),
            Err(e) => eprintln!("  swap→vfio-pci FAILED: {e}"),
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }

    // Phase 1b: Open fresh VFIO device
    let fds = ember_client::request_fds(&bdf).expect("ember fds");
    let vfio_dev = coral_driver::vfio::VfioDevice::from_received(&bdf, fds).expect("VfioDevice");
    let bar0 = vfio_dev.map_bar(0).expect("map_bar(0)");

    let r = |off: usize| bar0.read_u32(SEC2_BASE + off).unwrap_or(0xDEAD_DEAD);

    // Check state after nouveau cycle (should be HS from nouveau, then HRESET after unbind)
    let sctl_pre = r(falcon::SCTL);
    let cpuctl_pre = r(falcon::CPUCTL);
    let exci_pre = r(falcon::EXCI);
    let pc_pre = r(falcon::PC);
    eprintln!("\nPhase 1c: Post-nouveau SEC2 state...");
    eprintln!("  sctl={sctl_pre:#010x} cpuctl={cpuctl_pre:#010x}");
    eprintln!("  PC={pc_pre:#06x} EXCI={exci_pre:#010x}");
    eprintln!(
        "  HS={} HRESET={}",
        sctl_pre & 0x02 != 0,
        cpuctl_pre & falcon::CPUCTL_HRESET != 0
    );

    // Phase 2: Run the FULL sysmem boot (blob_size preserved)
    eprintln!("\nPhase 2: Attempting sysmem ACR boot with full init...");
    let fw = coral_driver::nv::vfio_compute::acr_boot::AcrFirmwareSet::load("gv100")
        .expect("firmware load");
    let container = vfio_dev.dma_backend();

    let full_result = coral_driver::nv::vfio_compute::acr_boot::attempt_sysmem_acr_boot_full(
        &bar0, &fw, container,
    );

    eprintln!("Full boot result: success={}", full_result.success);
    eprintln!("Strategy: {}", full_result.strategy);
    for note in &full_result.notes {
        eprintln!("  {note}");
    }

    // Phase 3: Immediately check if SEC2 is RUNNING
    eprintln!("\nPhase 3: Post-full-boot state...");
    let cpuctl_post = r(falcon::CPUCTL);
    let sctl_post = r(falcon::SCTL);
    let pc_post = r(falcon::PC);
    let exci_post = r(falcon::EXCI);

    let running = cpuctl_post & (falcon::CPUCTL_HALTED | falcon::CPUCTL_HRESET) == 0;
    let hs = sctl_post & 0x02 != 0;

    eprintln!("  cpuctl={cpuctl_post:#010x} sctl={sctl_post:#010x} PC={pc_post:#06x}");
    eprintln!("  EXCI={exci_post:#010x}");
    eprintln!("  RUNNING={running} HS={hs}");

    // Phase 4: Try DMEM queue discovery (might work if RUNNING!)
    eprintln!("\nPhase 4: DMEM queue discovery...");

    // First check: is DMEM readable?
    let w = |off: usize, val: u32| {
        let _ = bar0.write_u32(SEC2_BASE + off, val);
    };
    w(falcon::DMEMC, 1u32 << 25);
    let dmem_test = r(falcon::DMEMD);
    let dmem_locked = dmem_test == 0xDEAD_5EC2;
    eprintln!("  DMEM[0]={dmem_test:#010x} locked={dmem_locked}");

    if !dmem_locked {
        eprintln!("  *** DMEM IS READABLE! Attempting queue discovery...");
        match Sec2Queues::discover(&bar0) {
            Ok(mut queues) => {
                let info = queues.info();
                eprintln!(
                    "  *** QUEUES FOUND: CMDQ@{:#x}/{}B MSGQ@{:#x}/{}B",
                    info.cmdq_offset, info.cmdq_size, info.msgq_offset, info.msgq_size
                );
                eprintln!("  os_debug_entry: {:#06x}", info.os_debug_entry);

                // Phase 5: Send BOOTSTRAP_FALCON!
                eprintln!("\nPhase 5: Sending BOOTSTRAP_FALCON commands...");
                use coral_driver::nv::vfio_compute::acr_boot::sec2_queue::FalconId;
                for (name, fid) in [("GPCCS", FalconId::Gpccs), ("FECS", FalconId::Fecs)] {
                    match queues.cmd_bootstrap_falcon(&bar0, fid) {
                        Ok(seq) => {
                            eprintln!("  Sent BOOTSTRAP_FALCON({name}) seq={seq}");
                            match queues.recv_wait(&bar0, seq, 5000) {
                                Ok(msg) => {
                                    eprintln!(
                                        "  *** RESPONSE for {name}: unit={:#04x} size={} seq={}",
                                        msg.unit_id, msg.size, msg.seq_id
                                    );
                                    let hex_words: Vec<_> =
                                        msg.words.iter().map(|w| format!("{w:#010x}")).collect();
                                    eprintln!("    words: [{}]", hex_words.join(", "));
                                }
                                Err(e) => eprintln!("  No response for {name}: {e}"),
                            }
                        }
                        Err(e) => eprintln!("  {name} send failed: {e}"),
                    }
                }
            }
            Err(e) => {
                eprintln!("  Queue discovery failed: {e}");
                eprintln!("  Trying known offsets from EMEM init message...");

                // Use known queue offsets from Exp 097: CMDQ@0x0000/128, MSGQ@0x0080/128
                {
                    let mut queues = Sec2Queues::from_known(Sec2QueueInfo {
                        cmdq_offset: 0x0000,
                        cmdq_size: 128,
                        msgq_offset: 0x0080,
                        msgq_size: 128,
                        os_debug_entry: 0x026c,
                    });
                    eprintln!("  Built queues from known offsets (EMEM Exp 097).");
                    use coral_driver::nv::vfio_compute::acr_boot::sec2_queue::FalconId;
                    for (name, fid) in [("GPCCS", FalconId::Gpccs), ("FECS", FalconId::Fecs)] {
                        match queues.cmd_bootstrap_falcon(&bar0, fid) {
                            Ok(seq) => {
                                eprintln!("  Sent BOOTSTRAP_FALCON({name}) seq={seq}");
                                match queues.recv_wait(&bar0, seq, 5000) {
                                    Ok(msg) => {
                                        eprintln!(
                                            "  *** RESPONSE for {name}: unit={:#04x} size={} seq={}",
                                            msg.unit_id, msg.size, msg.seq_id
                                        );
                                    }
                                    Err(e) => eprintln!("  No response for {name}: {e}"),
                                }
                            }
                            Err(e) => eprintln!("  {name} send failed: {e}"),
                        }
                    }
                }
            }
        }
    } else {
        eprintln!("  DMEM still locked. Checking EMEM for updated state...");

        // Read EMEM init message region
        w(falcon::EMEMC0, (1u32 << 25) | 0x80);
        let emem_init: Vec<u32> = (0..8).map(|_| r(falcon::EMEMD0)).collect();
        let words: Vec<String> = emem_init.iter().map(|w| format!("{w:#010x}")).collect();
        eprintln!("  EMEM[0x80..0xA0]: {}", words.join(" "));
    }

    // Phase 6: Queue register state
    eprintln!("\nPhase 6: Final queue registers...");
    let final_probe = Sec2Queues::probe(&bar0);
    eprintln!("  {final_probe}");
    let final_cpuctl = r(falcon::CPUCTL);
    let final_sctl = r(falcon::SCTL);
    let final_pc = r(falcon::PC);
    eprintln!("  cpuctl={final_cpuctl:#010x} sctl={final_sctl:#010x} PC={final_pc:#06x}");

    // EMEM first 256 bytes for comparison
    eprintln!("\n  EMEM[0x00..0x40] (first 16 words):");
    w(falcon::EMEMC0, 1u32 << 25);
    for i in 0..4 {
        let ws: Vec<u32> = (0..4).map(|_| r(falcon::EMEMD0)).collect();
        let off = i * 16;
        eprintln!(
            "    {off:04x}: {:08x} {:08x} {:08x} {:08x}",
            ws[0], ws[1], ws[2], ws[3]
        );
    }

    eprintln!("\n=== Exp 098 Complete ===");
}

/// Full EMEM discovery after HS boot: scan all EMEM for queue structures.
#[test]
#[ignore = "requires VFIO-bound GPU hardware"]
fn sec2_emem_discovery_after_hs() {
    init_tracing();

    eprintln!("\n=== Exp 097: EMEM Discovery + MSI Wake ===\n");

    // ── Phase 0: Get a raw VFIO device (SEC2 should be in HS from prior test) ──
    let bdf = vfio_bdf();
    let fds = ember_client::request_fds(&bdf).expect("ember fds");
    let vfio_dev = coral_driver::vfio::VfioDevice::from_received(&bdf, fds).expect("VfioDevice");
    let bar0 = vfio_dev.map_bar(0).expect("map_bar(0)");

    let r = |off: usize| bar0.read_u32(SEC2_BASE + off).unwrap_or(0xDEAD_DEAD);
    let w = |off: usize, val: u32| {
        let _ = bar0.write_u32(SEC2_BASE + off, val);
    };

    // ── Phase 1: Characterize SEC2 state ──
    let cpuctl = r(falcon::CPUCTL);
    let sctl = r(falcon::SCTL);
    let exci = r(falcon::EXCI);
    let pc = r(falcon::PC);
    let hwcfg = r(falcon::HWCFG);
    let mb0 = r(falcon::MAILBOX0);
    let mb1 = r(falcon::MAILBOX1);
    let irqstat = r(falcon::IRQSTAT);
    let irqmask = r(falcon::IRQMASK);

    let hs = sctl & 0x02 != 0;
    let stopped = cpuctl & falcon::CPUCTL_HALTED != 0;
    let hreset = cpuctl & falcon::CPUCTL_HRESET != 0;

    eprintln!("── Phase 1: SEC2 State ──");
    eprintln!("  cpuctl={cpuctl:#010x} sctl={sctl:#010x} HS={hs}");
    eprintln!("  pc={pc:#06x} exci={exci:#010x} stopped={stopped} hreset={hreset}");
    eprintln!("  mb0={mb0:#010x} mb1={mb1:#010x}");
    eprintln!("  irqstat={irqstat:#010x} irqmask={irqmask:#010x}");
    eprintln!("  hwcfg={hwcfg:#010x}");

    let dmem_size = falcon::dmem_size_bytes(hwcfg);
    let imem_size = falcon::imem_size_bytes(hwcfg);
    eprintln!("  IMEM={imem_size}B DMEM={dmem_size}B");

    // ── Phase 2: Queue register snapshot ──
    let probe = Sec2Queues::probe(&bar0);
    eprintln!("\n── Phase 2: Queue Registers ──");
    eprintln!("  {probe}");

    // ── Phase 3: DMEM sample (verify lockdown) ──
    eprintln!("\n── Phase 3: DMEM Lockdown Check ──");
    let dmem_locked = {
        w(falcon::DMEMC, 1u32 << 25);
        let w0 = r(falcon::DMEMD);
        let w1 = r(falcon::DMEMD);
        let w2 = r(falcon::DMEMD);
        let w3 = r(falcon::DMEMD);
        eprintln!("  DMEM[0..16]: {w0:#010x} {w1:#010x} {w2:#010x} {w3:#010x}");
        w0 == 0xDEAD_5EC2 || w1 == 0xDEAD_5EC2
    };
    eprintln!("  locked={dmem_locked}");

    // Also try a few known offsets where the init message might live
    for off in [0x0F00u32, 0x1000, 0x2000, 0x4000, 0x8000] {
        w(falcon::DMEMC, 1u32 << 25 | off);
        let w0 = r(falcon::DMEMD);
        let unit_id = w0 & 0xFF;
        let size = (w0 >> 8) & 0xFF;
        if unit_id == 0x01 && (24..=48).contains(&size) {
            eprintln!("  *** INIT MSG CANDIDATE at DMEM[{off:#06x}]: w0={w0:#010x}");
        }
    }

    // ── Phase 4: Full EMEM scan ──
    eprintln!("\n── Phase 4: Full EMEM Scan ──");

    // Read EMEM in chunks. EMEM size on GP102+ is typically 1KB-16KB.
    // We'll read up to 16KB and stop when we hit all-dead reads.
    let max_emem = 16384u32;
    let chunk_words = 64usize;
    let chunk_bytes = (chunk_words * 4) as u32;

    let mut total_nonzero = 0u32;
    let mut emem_regions: Vec<(u32, Vec<u32>)> = Vec::new();
    let mut init_msg_candidates: Vec<(u32, u32)> = Vec::new();
    let mut all_dead_count = 0u32;

    for offset in (0..max_emem).step_by(chunk_bytes as usize) {
        w(falcon::EMEMC0, (1u32 << 25) | offset);
        let words: Vec<u32> = (0..chunk_words).map(|_| r(falcon::EMEMD0)).collect();

        let all_dead = words.iter().all(|&w| w == 0xDEAD_DEAD);
        if all_dead {
            all_dead_count += 1;
            if all_dead_count >= 4 {
                eprintln!("  EMEM ends at ~{offset:#06x} (4 consecutive dead chunks)");
                break;
            }
            continue;
        }
        all_dead_count = 0;

        let nz: Vec<(usize, u32)> = words
            .iter()
            .enumerate()
            .filter(|&(_, &w)| w != 0 && w != 0xDEAD_DEAD)
            .map(|(i, &w)| (i, w))
            .collect();

        if !nz.is_empty() {
            total_nonzero += nz.len() as u32;

            let region_str: Vec<String> = nz
                .iter()
                .take(16)
                .map(|(i, w)| format!("[{:#05x}]={w:#010x}", offset as usize + i * 4))
                .collect();
            let suffix = if nz.len() > 16 {
                format!(" (+{} more)", nz.len() - 16)
            } else {
                String::new()
            };
            eprintln!("  EMEM@{offset:#06x}: {}{suffix}", region_str.join(" "));

            emem_regions.push((offset, words.clone()));
        }

        // Check for init-message-like pattern in EMEM
        for (i, window) in words.windows(6).enumerate() {
            let w0 = window[0];
            let unit_id = w0 & 0xFF;
            let size = (w0 >> 8) & 0xFF;
            let w1 = window[1];
            let msg_type = w1 & 0xFF;
            let num_queues = (w1 >> 8) & 0xFF;

            if unit_id == 0x01 && (24..=48).contains(&size) && msg_type == 0x00 && num_queues == 2 {
                let emem_off = offset + (i as u32) * 4;
                eprintln!("  *** INIT MSG in EMEM at {emem_off:#06x}!");
                eprintln!("    w0={w0:#010x} (unit=0x01 size={size})");
                eprintln!(
                    "    w1={w1:#010x} (msg=0x00 queues=2 os_debug={:#06x})",
                    (w1 >> 16) & 0xFFFF
                );
                let q0_off = window[2];
                let q0_meta = window[3];
                let q1_off = window[4];
                let q1_meta = window[5];
                eprintln!(
                    "    queue0: offset={q0_off:#010x} size={} id={}",
                    q0_meta & 0xFFFF,
                    (q0_meta >> 24) & 0xFF
                );
                eprintln!(
                    "    queue1: offset={q1_off:#010x} size={} id={}",
                    q1_meta & 0xFFFF,
                    (q1_meta >> 24) & 0xFF
                );
                init_msg_candidates.push((emem_off, w0));
            }
        }

        // Also scan for queue-like pointers (DMEM addresses in typical range 0x100-0xFFFF)
        for (i, &word) in words.iter().enumerate() {
            if (0x0100..=0xFFFF).contains(&word) && word & 0x03 == 0 {
                let next_idx = i + 1;
                if next_idx < words.len() {
                    let next = words[next_idx];
                    let size_field = next & 0xFFFF;
                    if (64..=4096).contains(&size_field) {
                        let emem_off = offset + (i as u32) * 4;
                        eprintln!(
                            "  [queue-like] EMEM[{emem_off:#05x}]: ptr={word:#06x} next={next:#010x} (size_field={size_field})"
                        );
                    }
                }
            }
        }
    }

    eprintln!("\n  Total non-zero EMEM words: {total_nonzero}");
    eprintln!("  Init message candidates: {}", init_msg_candidates.len());
    eprintln!("  Regions with data: {}", emem_regions.len());

    // ── Phase 5: MSI IRQ wiring ──
    eprintln!("\n── Phase 5: MSI IRQ Wiring ──");
    use coral_driver::vfio::irq::{VfioIrq, VfioIrqIndex};

    match VfioIrq::arm(vfio_dev.device_as_fd(), VfioIrqIndex::Msi, 0) {
        Ok(mut msi) => {
            eprintln!("  MSI armed on vector 0");

            // Check for any pending IRQs
            let pending = msi.poll_irq();
            eprintln!(
                "  Pending IRQs after arm: {pending} (fires={})",
                msi.fire_count()
            );

            // Wait briefly for any spontaneous IRQ
            let spontaneous = msi.wait(100);
            eprintln!("  Spontaneous IRQ (100ms): {spontaneous}");

            // ── Phase 6: Poke IRQSSET and watch for MSI ──
            eprintln!("\n── Phase 6: IRQSSET Poke + MSI Watch ──");

            let irqstat_before = r(falcon::IRQSTAT);
            eprintln!("  IRQSTAT before poke: {irqstat_before:#010x}");

            // Poke the SEC2 IRQ (0x40 = SWGEN0, the standard CMDQ doorbell)
            w(falcon::IRQSSET, 0x40);
            std::thread::sleep(std::time::Duration::from_millis(10));

            let irqstat_after = r(falcon::IRQSTAT);
            let pc_after_poke = r(falcon::PC);
            let cpuctl_after_poke = r(falcon::CPUCTL);
            eprintln!("  IRQSTAT after IRQSSET=0x40: {irqstat_after:#010x}");
            eprintln!("  PC after poke: {pc_after_poke:#06x} (was {pc:#06x})");
            eprintln!("  cpuctl after poke: {cpuctl_after_poke:#010x}");

            let msi_fired = msi.poll_irq();
            eprintln!(
                "  MSI fired after IRQSSET poke: {msi_fired} (fires={})",
                msi.fire_count()
            );

            // Also try IRQMSET to enable the IRQ mask, then poke again
            eprintln!("\n  Trying IRQMSET=0x40 (enable SWGEN0 in mask)...");
            w(falcon::IRQMSET, 0x40);
            let irqmask_after = r(falcon::IRQMASK);
            eprintln!("  IRQMASK after IRQMSET: {irqmask_after:#010x}");

            w(falcon::IRQSSET, 0x40);
            std::thread::sleep(std::time::Duration::from_millis(10));
            let msi_fired2 = msi.poll_irq();
            eprintln!(
                "  MSI fired after mask+poke: {msi_fired2} (fires={})",
                msi.fire_count()
            );

            // ── Phase 7: Try STARTCPU on STOPPED SEC2 ──
            eprintln!("\n── Phase 7: STARTCPU on STOPPED SEC2 ──");
            let cpuctl_pre = r(falcon::CPUCTL);
            eprintln!("  cpuctl before: {cpuctl_pre:#010x}");

            // Try writing STARTCPU (bit 1) to CPUCTL
            w(falcon::CPUCTL, falcon::CPUCTL_STARTCPU);
            std::thread::sleep(std::time::Duration::from_millis(50));
            let cpuctl_after = r(falcon::CPUCTL);
            let pc_after = r(falcon::PC);
            let sctl_after = r(falcon::SCTL);
            eprintln!("  cpuctl after STARTCPU: {cpuctl_after:#010x}");
            eprintln!("  PC after STARTCPU: {pc_after:#06x}");
            eprintln!("  SCTL after STARTCPU: {sctl_after:#010x}");

            let msi_after_start = msi.poll_irq();
            eprintln!(
                "  MSI fired after STARTCPU: {msi_after_start} (fires={})",
                msi.fire_count()
            );

            // Try CPUCTL_ALIAS too
            eprintln!("\n  Trying CPUCTL_ALIAS...");
            w(falcon::CPUCTL_ALIAS, falcon::CPUCTL_STARTCPU);
            std::thread::sleep(std::time::Duration::from_millis(50));
            let cpuctl_alias = r(falcon::CPUCTL);
            let pc_alias = r(falcon::PC);
            eprintln!("  cpuctl after ALIAS STARTCPU: {cpuctl_alias:#010x}");
            eprintln!("  PC after ALIAS: {pc_alias:#06x}");

            // ── Phase 8: Try writing CMDQ tail to non-zero and poking ──
            eprintln!("\n── Phase 8: CMDQ Tail Write + Poke ──");
            let cmdq_head = r(0xA00);
            let cmdq_tail = r(0xA04);
            eprintln!("  CMDQ before: head={cmdq_head:#x} tail={cmdq_tail:#x}");

            // Write a non-zero head (pretend we have a command at offset 0x10)
            w(0xA00, 0x10);
            let cmdq_head_after = r(0xA00);
            eprintln!("  CMDQ head after write=0x10: readback={cmdq_head_after:#x}");

            // Poke IRQ
            w(falcon::IRQSSET, 0x40);
            std::thread::sleep(std::time::Duration::from_millis(50));
            let cmdq_head_post = r(0xA00);
            let cmdq_tail_post = r(0xA04);
            let pc_post = r(falcon::PC);
            eprintln!(
                "  After poke: CMDQ head={cmdq_head_post:#x} tail={cmdq_tail_post:#x} PC={pc_post:#06x}"
            );

            let msi_cmdq = msi.poll_irq();
            eprintln!(
                "  MSI fired after CMDQ+poke: {msi_cmdq} (fires={})",
                msi.fire_count()
            );

            // Restore CMDQ head to 0
            w(0xA00, 0x0);

            // ── Phase 9: Full EMEM structured dump (first 512 bytes) ──
            eprintln!("\n── Phase 9: EMEM Structured Dump (first 512B) ──");
            w(falcon::EMEMC0, 1u32 << 25);
            let emem_first: Vec<u32> = (0..128).map(|_| r(falcon::EMEMD0)).collect();
            for row in 0..16 {
                let base = row * 8;
                let offset = row * 32;
                let vals: Vec<String> = emem_first[base..base + 8]
                    .iter()
                    .map(|w| format!("{w:08x}"))
                    .collect();
                let ascii: String = emem_first[base..base + 8]
                    .iter()
                    .flat_map(|w| w.to_le_bytes())
                    .map(|b| {
                        if (0x20..0x7F).contains(&b) {
                            b as char
                        } else {
                            '.'
                        }
                    })
                    .collect();
                eprintln!("  {offset:04x}: {} |{ascii}|", vals.join(" "));
            }

            // Total MSI fires
            eprintln!("\n── Summary ──");
            eprintln!("  Total MSI fires: {}", msi.fire_count());
            eprintln!("  DMEM locked: {dmem_locked}");
            eprintln!("  HS mode: {hs}");
            eprintln!(
                "  Init msg candidates in EMEM: {}",
                init_msg_candidates.len()
            );
        }
        Err(e) => {
            eprintln!("  MSI arm failed: {e}");
            eprintln!("  (continuing without MSI)");
        }
    }

    // ── Phase 10: Post-test queue state ──
    eprintln!("\n── Phase 10: Final Queue State ──");
    let final_probe = Sec2Queues::probe(&bar0);
    eprintln!("  {final_probe}");

    let final_cpuctl = r(falcon::CPUCTL);
    let final_sctl = r(falcon::SCTL);
    let final_pc = r(falcon::PC);
    eprintln!("  cpuctl={final_cpuctl:#010x} sctl={final_sctl:#010x} PC={final_pc:#06x}");

    eprintln!("\n=== Exp 097 Complete ===");
}
