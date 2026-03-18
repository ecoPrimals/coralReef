// SPDX-License-Identifier: AGPL-3.0-only
//! NVIDIA VFIO — channel creation, PFIFO, runlist tests.
//!
//! Tests for PFIFO diagnostic matrix, ProbeInterpreter, and PRI bus backpressure.
//!
//! Run: `CORALREEF_VFIO_BDF=0000:01:00.0 cargo test --test hw_nv_vfio_channel --features vfio -- --ignored`

#[cfg(feature = "vfio")]
#[path = "glowplug_client.rs"]
mod glowplug_client;

#[cfg(feature = "vfio")]
mod tests {
    use super::glowplug_client::VfioLease;

    fn vfio_bdf() -> String {
        std::env::var("CORALREEF_VFIO_BDF")
            .expect("set CORALREEF_VFIO_BDF=0000:XX:XX.X to run VFIO tests")
    }

    fn try_lease(bdf: &str) -> Option<VfioLease> {
        match VfioLease::acquire(bdf) {
            Ok(lease) => Some(lease),
            Err(e) => {
                eprintln!("glowplug not available ({e}), opening VFIO directly");
                None
            }
        }
    }

    fn open_vfio() -> (Option<VfioLease>, coral_driver::nv::RawVfioDevice) {
        let bdf = vfio_bdf();
        let lease = try_lease(&bdf);
        let raw = coral_driver::nv::RawVfioDevice::open(&bdf)
            .expect("RawVfioDevice::open() — is GPU bound to vfio-pci?");
        (lease, raw)
    }

    #[test]
    #[ignore = "requires VFIO-bound GPU hardware"]
    fn vfio_pfifo_diagnostic_matrix() {
        use coral_driver::nv::RawVfioDevice;
        use coral_driver::vfio::channel::{build_experiment_matrix, diagnostic_matrix};

        let bdf = vfio_bdf();
        let (_lease, mut raw) = open_vfio();

        // Verify PCIe bus mastering via sysfs (critical for DMA)
        let config_path = format!("/sys/bus/pci/devices/{bdf}/config");
        if let Ok(cfg) = std::fs::read(&config_path)
            && cfg.len() >= 6
        {
            let cmd = u16::from_le_bytes([cfg[4], cfg[5]]);
            let bm = cmd & 0x0004 != 0;
            eprintln!("PCI_COMMAND={cmd:#06x} BusMaster={bm}");
            assert!(bm, "PCIe bus mastering MUST be enabled for DMA");
        }

        let configs = build_experiment_matrix();
        eprintln!(
            "\n=== PFIFO DIAGNOSTIC MATRIX: {} configurations ===\n",
            configs.len()
        );

        let results = diagnostic_matrix(
            raw.container_fd,
            &raw.bar0,
            RawVfioDevice::gpfifo_iova(),
            RawVfioDevice::gpfifo_entries(),
            RawVfioDevice::userd_iova(),
            0, // channel ID
            &configs,
            raw.gpfifo_ring.as_mut_slice(),
            raw.userd.as_mut_slice(),
        )
        .expect("diagnostic_matrix failed");

        let total = results.len();
        let faulted: Vec<_> = results.iter().filter(|r| r.faulted).collect();
        let scheduled: Vec<_> = results.iter().filter(|r| r.scheduled).collect();
        let clean: Vec<_> = results
            .iter()
            .filter(|r| !r.faulted && r.scheduled)
            .collect();
        let pbdma_ours: Vec<_> = results.iter().filter(|r| r.pbdma_ours).collect();

        eprintln!("\n=== SUMMARY ===");
        eprintln!("Total:        {total}");
        eprintln!("Faulted:      {}", faulted.len());
        eprintln!("Scheduled:    {}", scheduled.len());
        eprintln!("Clean:        {} (no fault + scheduled)", clean.len());
        eprintln!(
            "PBDMA ours:   {} (registers changed from residual)",
            pbdma_ours.len()
        );

        if !clean.is_empty() {
            eprintln!("\n=== WINNING CONFIGURATIONS ===");
            for r in &clean {
                eprintln!("  {}", r.name);
            }
        }

        if !pbdma_ours.is_empty() {
            eprintln!("\n=== PBDMA REGISTERS CHANGED (direct programming worked) ===");
            for r in &pbdma_ours {
                eprintln!(
                    "  {} | USERD@D0={:08x} @08={:08x} GP_BASE={:08x}_{:08x} SIG={:08x} GP_PUT={} GP_FETCH={}",
                    r.name,
                    r.pbdma_userd_lo,
                    r.pbdma_ramfc_userd_lo,
                    r.pbdma_gp_base_hi,
                    r.pbdma_gp_base_lo,
                    r.pbdma_signature,
                    r.pbdma_gp_put,
                    r.pbdma_gp_fetch
                );
            }
        }

        if !scheduled.is_empty() {
            eprintln!("\n=== SCHEDULED (may have faults) ===");
            for r in &scheduled {
                eprintln!("  {} (faulted={})", r.name, r.faulted);
            }
        }

        eprintln!("\nDiagnostic matrix complete. Analyze the table above.");
    }

    #[test]
    #[ignore = "requires VFIO-bound GPU hardware"]
    fn vfio_interpreter_probe() {
        use coral_driver::vfio::channel::ProbeInterpreter;

        let (_lease, raw) = open_vfio();

        let interpreter = ProbeInterpreter::new(&raw.bar0, raw.container_fd);
        let report = interpreter.run();
        report.print_summary();

        eprintln!("\nProbe reached layer {}/7", report.depth());
        assert!(
            report.depth() >= 3,
            "Interpreter should reach at least Layer 3 (engines)"
        );
    }

    #[test]
    #[ignore = "requires VFIO-bound GPU hardware"]
    fn vfio_pri_backpressure_probe() {
        use coral_driver::vfio::channel::pri_monitor::{DomainHealth, PriBusMonitor};

        let (_lease, raw) = open_vfio();

        eprintln!("╔══════════════════════════════════════════════════════════════╗");
        eprintln!("║ PRI BUS BACKPRESSURE PROBE — Domain Health Map             ║");
        eprintln!("╠══════════════════════════════════════════════════════════════╣");

        let mut monitor = PriBusMonitor::new(&raw.bar0);

        // Phase 1: Full diagnostic with decoded PRI errors
        let diagnostic = monitor.full_diagnostic();
        for line in &diagnostic {
            eprintln!("║ {line}");
        }

        let health = monitor.probe_all_domains();
        let alive = health
            .iter()
            .filter(|(_, _, h)| matches!(h, DomainHealth::Alive))
            .count();
        let faulted = health
            .iter()
            .filter(|(_, _, h)| matches!(h, DomainHealth::Faulted { .. }))
            .count();
        eprintln!("║");
        eprintln!("║ Summary: {alive} alive, {faulted} faulted");

        // Phase 2: If faulted, try recovery
        if faulted > 0 {
            eprintln!("╠══ PRI Recovery Attempt ════════════════════════════════════╣");
            let recovered = monitor.attempt_recovery();
            eprintln!(
                "║ Recovery: {}",
                if recovered {
                    "SUCCESS (bus clean)"
                } else {
                    "FAILED (bus locked)"
                }
            );

            // Re-probe after recovery
            let post_health = monitor.probe_all_domains();
            let post_alive = post_health
                .iter()
                .filter(|(_, _, h)| matches!(h, DomainHealth::Alive))
                .count();
            let post_faulted = post_health
                .iter()
                .filter(|(_, _, h)| matches!(h, DomainHealth::Faulted { .. }))
                .count();
            eprintln!("║ Post-recovery: {post_alive} alive, {post_faulted} faulted");

            for (name, off, h) in &post_health {
                if matches!(h, DomainHealth::Faulted { .. }) {
                    eprintln!("║   Still faulted: {name} [{off:#010x}]");
                }
            }
        }

        // Phase 3: Test write with backpressure on a safe register (PMC_ENABLE)
        eprintln!("╠══ Monitored Write Test (PMC_ENABLE) ══════════════════════╣");
        let pmc = monitor.read_u32(0x200);
        eprintln!("║ PMC_ENABLE read: {pmc:#010x}");
        let outcome = monitor.write_u32(0x200, pmc);
        eprintln!("║ PMC_ENABLE write-back: {outcome:?}");

        // Phase 4: Test write to a likely-faulted domain (FBPA0)
        eprintln!("╠══ Monitored Write Test (FBPA0) ════════════════════════════╣");
        let fbpa0 = monitor.read_u32(0x9A0000);
        eprintln!("║ FBPA0 read: {fbpa0:#010x}");
        if fbpa0 != 0xDEAD_DEAD {
            let outcome = monitor.write_u32(0x9A0004, fbpa0);
            eprintln!("║ FBPA0 write attempt: {outcome:?}");
        } else {
            eprintln!("║ FBPA0 read failed, skipping write");
        }

        let stats = monitor.into_report();
        eprintln!("╠══ Final PRI Statistics ════════════════════════════════════╣");
        eprintln!(
            "║ Reads: {} total, {} faulted",
            stats.reads_total, stats.reads_faulted
        );
        eprintln!(
            "║ Writes: {} total, {} applied, {} skipped",
            stats.writes_total, stats.writes_applied, stats.writes_skipped_faulted
        );
        eprintln!("║ Recoveries: {}", stats.bus_recoveries);
        if !stats.domains_faulted.is_empty() {
            eprintln!("║ Faulted domains: {:?}", stats.domains_faulted);
        }
        eprintln!("╚══════════════════════════════════════════════════════════════╝");
    }
}
