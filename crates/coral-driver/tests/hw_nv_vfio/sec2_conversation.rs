// SPDX-License-Identifier: AGPL-3.0-only

//! SEC2 conversation stack integration test.
//!
//! Exercises the full SEC2 CMDQ/MSGQ conversation pipeline on live hardware:
//! boot solver → queue discovery → BOOTSTRAP_FALCON → MSGQ response.
//! Also validates VFIO IRQ probing.

use crate::ember_client;
use crate::helpers::{init_tracing, open_vfio, vfio_bdf};
use coral_driver::nv::vfio_compute::acr_boot::sec2_queue::{
    FalconId, Sec2QueueProbe, Sec2Queues,
};

/// Full conversation test: boot → discover queues → send commands → read responses.
#[test]
#[ignore = "requires VFIO-bound GPU hardware"]
fn sec2_conversation_full_cycle() {
    init_tracing();
    let mut dev = open_vfio();

    eprintln!("\n=== SEC2 Conversation: Full Cycle ===\n");

    // Phase 1: Run the boot solver (captures queue probes via probe_and_bootstrap)
    eprintln!("Phase 1: Running falcon boot solver...");
    let results = dev
        .falcon_boot_solver(None)
        .expect("falcon_boot_solver");
    eprintln!("Boot solver produced {} results\n", results.len());

    for (i, r) in results.iter().enumerate() {
        let queue_notes: Vec<_> = r
            .notes
            .iter()
            .filter(|n| {
                n.contains("Queue probe")
                    || n.contains("SEC2 queues discovered")
                    || n.contains("CMDQ:")
                    || n.contains("MSGQ:")
                    || n.contains("queue discovery")
            })
            .collect();

        if !queue_notes.is_empty() {
            eprintln!("Strategy {i}: {}", r.strategy);
            for note in &queue_notes {
                eprintln!("  {note}");
            }
            eprintln!();
        }
    }

    // Phase 2: Open raw BAR0 for direct queue probing
    eprintln!("Phase 2: Direct SEC2 queue probe...");
    let bdf = vfio_bdf();
    let fds = ember_client::request_fds(&bdf).expect("ember fds");
    let vfio_dev =
        coral_driver::vfio::VfioDevice::from_received(&bdf, fds).expect("VfioDevice");
    let bar0 = vfio_dev.map_bar(0).expect("map_bar(0)");

    let probe = Sec2Queues::probe(&bar0);
    eprintln!("Queue registers: {probe}");
    eprintln!(
        "  is_initialized: {}",
        probe.is_initialized()
    );

    // Phase 3: Attempt queue discovery on live DMEM
    eprintln!("\nPhase 3: Queue discovery...");
    match Sec2Queues::discover(&bar0) {
        Ok(mut queues) => {
            let info = queues.info();
            eprintln!(
                "  CMDQ: offset={:#x} size={}",
                info.cmdq_offset, info.cmdq_size
            );
            eprintln!(
                "  MSGQ: offset={:#x} size={}",
                info.msgq_offset, info.msgq_size
            );
            eprintln!("  os_debug_entry: {:#06x}", info.os_debug_entry);

            // Phase 4: Try BOOTSTRAP_FALCON
            eprintln!("\nPhase 4: Sending BOOTSTRAP_FALCON commands...");
            for (name, fid) in [("GPCCS", FalconId::Gpccs), ("FECS", FalconId::Fecs)] {
                match queues.cmd_bootstrap_falcon(&bar0, fid) {
                    Ok(seq) => {
                        eprintln!("  Sent BOOTSTRAP_FALCON({name}) seq={seq}");
                        match queues.recv_wait(&bar0, seq, 2000) {
                            Ok(msg) => {
                                eprintln!(
                                    "  Response for {name}: unit={:#04x} size={} seq={}",
                                    msg.unit_id, msg.size, msg.seq_id
                                );
                                let hex_words: Vec<_> = msg
                                    .words
                                    .iter()
                                    .map(|w| format!("{w:#010x}"))
                                    .collect();
                                eprintln!("    words: [{}]", hex_words.join(", "));
                            }
                            Err(e) => {
                                eprintln!("  No response for {name}: {e}");
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("  {name} send failed: {e}");
                    }
                }
            }

            // Post-command queue state
            let probe_after = Sec2Queues::probe(&bar0);
            eprintln!("\nPost-command queue registers: {probe_after}");
        }
        Err(e) => {
            eprintln!("  Queue discovery failed: {e}");
            eprintln!("  (SEC2 may not have reached HS mode or no init message in DMEM)");
        }
    }

    // Phase 5: VFIO IRQ probe
    eprintln!("\nPhase 5: VFIO IRQ info...");
    use coral_driver::vfio::irq::{get_irq_info, VfioIrqIndex};

    for (name, idx) in [
        ("INTX", VfioIrqIndex::Intx),
        ("MSI", VfioIrqIndex::Msi),
        ("MSI-X", VfioIrqIndex::Msix),
    ] {
        match get_irq_info(vfio_dev.device_as_fd(), idx) {
            Ok(info) => {
                eprintln!(
                    "  {name}: count={} flags={:#010x}",
                    info.count, info.flags
                );
            }
            Err(e) => {
                eprintln!("  {name}: query failed: {e}");
            }
        }
    }

    eprintln!("\n=== SEC2 Conversation test complete ===");
}

/// Probe-only test: just read queue registers without sending commands.
#[test]
#[ignore = "requires VFIO-bound GPU hardware"]
fn sec2_queue_probe_only() {
    init_tracing();

    let bdf = vfio_bdf();
    let fds = ember_client::request_fds(&bdf).expect("ember fds");
    let vfio_dev =
        coral_driver::vfio::VfioDevice::from_received(&bdf, fds).expect("VfioDevice");
    let bar0 = vfio_dev.map_bar(0).expect("map_bar(0)");

    let probe = Sec2Queues::probe(&bar0);
    eprintln!("SEC2 Queue probe: {probe}");

    let _: Sec2QueueProbe = probe.clone();
    let _debug = format!("{probe:?}");
    let _display = probe.to_string();
}
