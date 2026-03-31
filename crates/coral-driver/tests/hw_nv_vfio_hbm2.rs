// SPDX-License-Identifier: AGPL-3.0-only
//! NVIDIA VFIO — HBM2 training, capture, and PCLOCK probe tests.
//!
//! Run: `CORALREEF_VFIO_BDF=0000:01:00.0 cargo test --test hw_nv_vfio_hbm2 --features vfio -- --ignored`

#[cfg(feature = "vfio")]
#[path = "glowplug_client.rs"]
mod glowplug_client;

#[cfg(feature = "vfio")]
#[path = "ember_client.rs"]
mod ember_client;

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

    fn open_raw(bdf: &str) -> coral_driver::nv::RawVfioDevice {
        match super::ember_client::request_fds(bdf) {
            Ok(fds) => {
                eprintln!("ember: received VFIO fds for {bdf}");
                coral_driver::nv::RawVfioDevice::open_from_fds(bdf, fds)
                    .expect("RawVfioDevice::open_from_fds()")
            }
            Err(e) => {
                eprintln!("ember unavailable ({e}), opening VFIO directly");
                coral_driver::nv::RawVfioDevice::open(bdf)
                    .expect("RawVfioDevice::open() — is GPU bound to vfio-pci?")
            }
        }
    }

    fn open_vfio() -> (Option<VfioLease>, coral_driver::nv::RawVfioDevice) {
        let bdf = vfio_bdf();
        let lease = try_lease(&bdf);
        let raw = open_raw(&bdf);
        (lease, raw)
    }

    #[test]
    #[ignore = "requires VFIO-bound GPU hardware"]
    fn vfio_hbm2_phy_probe() {
        use coral_driver::vfio::channel::hbm2_training::{snapshot_fbpa, volta_hbm2};

        let bdf = vfio_bdf();
        let raw = open_raw(&bdf);

        eprintln!("╔══════════════════════════════════════════════════════════════╗");
        eprintln!("║ HBM2 PHY PROBE — FBPA Partition Status                    ║");
        eprintln!("╠══════════════════════════════════════════════════════════════╣");

        let snaps = snapshot_fbpa(&raw.bar0, volta_hbm2::FBPA_COUNT);
        let alive_count = snaps.iter().filter(|s| s.alive).count();
        let configured_count = snaps.iter().filter(|s| s.cfg != 0 && s.alive).count();

        for snap in &snaps {
            eprintln!(
                "║ FBPA{}: base={:#010x} cfg={:#010x} t0={:#010x} t1={:#010x} t2={:#010x} {}",
                snap.index,
                snap.base,
                snap.cfg,
                snap.timing0,
                snap.timing1,
                snap.timing2,
                if snap.alive { "ALIVE" } else { "DEAD" },
            );
        }

        eprintln!("║");
        eprintln!(
            "║ Summary: {alive_count}/{} alive, {configured_count}/{} configured",
            volta_hbm2::FBPA_COUNT,
            volta_hbm2::FBPA_COUNT
        );

        // Also probe LTC partitions
        eprintln!("╠══ LTC Partitions ══════════════════════════════════════════╣");
        for i in 0..volta_hbm2::LTC_COUNT {
            let base = volta_hbm2::LTC_BASE + i * volta_hbm2::LTC_STRIDE;
            let val = raw.bar0.read_u32(base).unwrap_or(0xDEAD_DEAD);
            let is_err = val == 0xFFFF_FFFF || val == 0xDEAD_DEAD || (val >> 16) == 0xBADF;
            eprintln!(
                "║ LTC{i}: base={base:#010x} val={val:#010x} {}",
                if is_err { "DEAD" } else { "alive" }
            );
        }

        // Probe PFB status registers
        eprintln!("╠══ PFB Status ══════════════════════════════════════════════╣");
        let pfb_regs: &[(&str, usize)] = &[
            ("CFG0", 0x100000),
            ("CFG1", 0x100004),
            ("PART_CTRL", 0x100200),
            ("ZBC_CTRL", 0x100300),
            ("MEM_STATUS", 0x100800),
            ("MEM_CTRL", 0x100804),
            ("MEM_ACK", 0x100808),
            ("MMU_CTRL", 0x100C80),
        ];
        for (name, off) in pfb_regs {
            let val = raw.bar0.read_u32(*off).unwrap_or(0xDEAD);
            eprintln!("║ {name:12}: [{off:#010x}] = {val:#010x}");
        }

        eprintln!("╚══════════════════════════════════════════════════════════════╝");
    }

    #[test]
    #[ignore = "requires VFIO-bound GPU hardware"]
    fn vfio_hbm2_timing_capture() {
        use coral_driver::vfio::channel::hbm2_training::{self as hbm2, HBM2_CAPTURE_DOMAINS};

        let (_lease, raw) = open_vfio();

        eprintln!("╔══════════════════════════════════════════════════════════════╗");
        eprintln!("║ HBM2 TIMING CAPTURE — Record FBPA/LTC/CLK registers       ║");
        eprintln!("╠══════════════════════════════════════════════════════════════╣");

        // Capture all HBM2-critical domains
        let mut total_regs = 0;
        let mut domain_data = Vec::new();
        for &(name, start, end) in HBM2_CAPTURE_DOMAINS {
            let mut registers = Vec::new();
            for off in (start..end).step_by(4) {
                let val = raw.bar0.read_u32(off).unwrap_or(0xDEAD_DEAD);
                let is_err = val == 0xFFFF_FFFF || val == 0xDEAD_DEAD || (val >> 16) == 0xBADF;
                if !is_err {
                    registers.push((off, val));
                }
            }
            eprintln!(
                "║ {name:12}: {} registers captured ({start:#010x}..{end:#010x})",
                registers.len()
            );
            total_regs += registers.len();
            domain_data.push(hbm2::DomainCapture {
                name: name.into(),
                registers,
            });
        }

        eprintln!("║");
        eprintln!(
            "║ Total: {total_regs} registers captured across {} domains",
            domain_data.len()
        );

        // Save capture as JSON
        let capture = hbm2::GoldenCapture {
            boot0: raw.bar0.read_u32(0).unwrap_or(0),
            pmc_enable: raw.bar0.read_u32(0x200).unwrap_or(0),
            domains: domain_data,
            timestamp: format!(
                "{}s",
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs()
            ),
        };

        let json = serde_json::to_string_pretty(&capture).unwrap_or_default();
        let out_dir = std::env::var("HOTSPRING_DATA_DIR")
            .map(|d| format!("{d}/metal_maps"))
            .unwrap_or_else(|_| "/tmp/coralreef/metal_maps".into());
        let out_path = format!("{out_dir}/titan_v_hbm2_timing_capture.json");
        if let Err(e) = std::fs::create_dir_all(&out_dir) {
            eprintln!("║ WARNING: cannot create {out_dir}: {e}");
        }
        match std::fs::write(&out_path, &json) {
            Ok(()) => eprintln!("║ Saved to {out_path} ({} bytes)", json.len()),
            Err(e) => eprintln!("║ WARNING: cannot write {out_path}: {e}"),
        }

        eprintln!("╚══════════════════════════════════════════════════════════════╝");
    }

    #[test]
    #[ignore = "requires VFIO-bound GPU hardware"]
    fn vfio_hbm2_training_attempt() {
        use coral_driver::vfio::channel::hbm2_training::{
            Hbm2Controller, TrainingAction, Untrained, volta_hbm2,
        };

        let bdf = vfio_bdf();
        let (_lease, raw) = open_vfio();

        eprintln!("╔══════════════════════════════════════════════════════════════╗");
        eprintln!("║ HBM2 TRAINING ATTEMPT — Typestate Sequence                 ║");
        eprintln!("╠══════════════════════════════════════════════════════════════╣");

        let ctrl = Hbm2Controller::<Untrained>::new(&raw.bar0, Some(&bdf), volta_hbm2::FBPA_COUNT);

        let result = ctrl
            .enable_phy()
            .and_then(|c| c.train_links())
            .and_then(|c| c.init_dram())
            .and_then(|c| c.verify_vram());

        match result {
            Ok(verified) => {
                let tlog = verified.training_log();
                eprintln!("║");
                eprintln!("║ *** TRAINING SUCCEEDED ***");
                eprintln!("║ Total actions: {}", tlog.actions.len());
                eprintln!("║ Register writes: {}", tlog.write_count());

                // Print phase transitions
                for action in &tlog.actions {
                    if let TrainingAction::PhaseTransition { from, to } = action {
                        eprintln!("║   {from} → {to}");
                    }
                }

                // Print verification results
                let verifications: Vec<_> = tlog
                    .actions
                    .iter()
                    .filter_map(|a| {
                        if let TrainingAction::Verification {
                            offset,
                            expected,
                            actual,
                            ok,
                        } = a
                        {
                            Some((offset, expected, actual, ok))
                        } else {
                            None
                        }
                    })
                    .collect();
                if !verifications.is_empty() {
                    eprintln!("║");
                    eprintln!("║ Verifications:");
                    for (off, exp, actual, ok) in &verifications {
                        eprintln!(
                            "║   [{off:#010x}] exp={exp:#010x} actual={actual:#010x} {}",
                            if **ok { "OK" } else { "FAIL" }
                        );
                    }
                }

                // Save FBPA state
                let fbpa_state = verified.fbpa_state();
                for snap in &fbpa_state {
                    eprintln!(
                        "║ FBPA{}: cfg={:#010x} {}",
                        snap.index,
                        snap.cfg,
                        if snap.alive { "alive" } else { "DEAD" }
                    );
                }
            }
            Err(err) => {
                eprintln!("║");
                eprintln!("║ TRAINING FAILED at phase: {}", err.phase);
                eprintln!("║ Detail: {}", err.detail);
                for (off, val) in &err.register_snapshot {
                    eprintln!("║   [{off:#010x}] = {val:#010x}");
                }
            }
        }

        eprintln!("╚══════════════════════════════════════════════════════════════╝");
    }

    #[test]
    #[ignore = "requires VFIO-bound GPU hardware"]
    fn vfio_hbm2_falcon_diagnostic() {
        use coral_driver::vfio::channel::devinit::FalconDiagnostic;

        let bdf = vfio_bdf();
        let (_lease, raw) = open_vfio();

        eprintln!("╔══════════════════════════════════════════════════════════════╗");
        eprintln!("║ PMU FALCON DIAGNOSTIC — Security, PROM, VBIOS Sources      ║");
        eprintln!("╠══════════════════════════════════════════════════════════════╣");

        let diag = FalconDiagnostic::probe(&raw.bar0, Some(&bdf));
        diag.print_report();

        // Try to read VBIOS from best source
        match diag.best_vbios(&raw.bar0, Some(&bdf)) {
            Ok(rom) => {
                eprintln!("║ VBIOS loaded: {} KB", rom.len() / 1024);

                // Parse BIT table
                match coral_driver::vfio::channel::devinit::BitTable::parse(&rom) {
                    Ok(bit) => {
                        eprintln!("║ BIT table: {} entries", bit.entries.len());
                        for entry in &bit.entries {
                            eprintln!(
                                "║   BIT '{}'  ver={} offset={:#06x} size={}",
                                entry.id as char, entry.version, entry.data_offset, entry.data_size
                            );
                        }
                    }
                    Err(e) => eprintln!("║ BIT parse failed: {e}"),
                }

                // Extract boot script writes for analysis
                match coral_driver::vfio::channel::devinit::extract_boot_script_writes(&rom) {
                    Ok(writes) => {
                        eprintln!("║ Boot script writes: {}", writes.len());

                        // Categorize by register domain
                        let mut fbpa_count = 0;
                        let mut ltc_count = 0;
                        let mut pfb_count = 0;
                        let mut clk_count = 0;
                        let mut other_count = 0;

                        for w in &writes {
                            let r = w.reg as usize;
                            if (0x9A0000..0x9B0000).contains(&r) {
                                fbpa_count += 1;
                            } else if (0x17E000..0x190000).contains(&r) {
                                ltc_count += 1;
                            } else if (0x100000..0x102000).contains(&r) {
                                pfb_count += 1;
                            } else if (0x132000..0x138000).contains(&r) {
                                clk_count += 1;
                            } else {
                                other_count += 1;
                            }
                        }

                        eprintln!(
                            "║   FBPA: {fbpa_count}, LTC: {ltc_count}, PFB: {pfb_count}, CLK: {clk_count}, other: {other_count}"
                        );
                    }
                    Err(e) => eprintln!("║ Script extraction failed: {e}"),
                }
            }
            Err(e) => eprintln!("║ No VBIOS available: {e}"),
        }

        eprintln!("╚══════════════════════════════════════════════════════════════╝");
    }

    #[test]
    #[ignore = "requires VFIO-bound GPU hardware"]
    fn vfio_pclock_deep_probe() {
        use coral_driver::vfio::channel::registers::pri;

        let (_lease, raw) = open_vfio();

        eprintln!("╔══════════════════════════════════════════════════════════════╗");
        eprintln!("║ PCLOCK DEEP PROBE — Scanning clock domain for live regs     ║");
        eprintln!("╠══════════════════════════════════════════════════════════════╣");

        // Phase 1: Enable PMC and wait
        let r = |off: usize| -> u32 { raw.bar0.read_u32(off).unwrap_or(0xDEAD_DEAD) };
        let w = |off: usize, val: u32| {
            let _ = raw.bar0.write_u32(off, val);
        };

        w(0x200, 0xFFFF_FFFF);
        std::thread::sleep(std::time::Duration::from_millis(50));
        eprintln!("║ PMC_ENABLE = {:#010x}", r(0x200));

        // Phase 2: Disable PTHERM clock gating first
        for &cg_off in &[0x020200_usize, 0x020204, 0x020208] {
            let old = r(cg_off);
            if !pri::is_pri_error(old) {
                w(cg_off, 0);
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(20));

        // Phase 3: Scan PCLOCK range (0x130000-0x138000) for readable registers
        eprintln!("╠══ PCLOCK Register Scan (0x130000-0x138000) ════════════════╣");
        let mut live_regs = Vec::new();
        let mut faulted_patterns: std::collections::HashMap<u32, usize> =
            std::collections::HashMap::new();

        for off in (0x130000..0x138000).step_by(4) {
            let val = r(off);
            if pri::is_pri_error(val) {
                *faulted_patterns.entry(val).or_default() += 1;
            } else if val != 0xDEAD_DEAD {
                live_regs.push((off, val));
            }
        }

        eprintln!("║ Live registers: {}", live_regs.len());
        for &(off, val) in &live_regs {
            eprintln!("║   [{off:#08x}] = {val:#010x}");
        }

        eprintln!("║ Faulted patterns:");
        let mut sorted: Vec<_> = faulted_patterns.iter().collect();
        sorted.sort_by(|a, b| b.1.cmp(a.1));
        for (pattern, count) in &sorted {
            eprintln!(
                "║   {pattern:#010x}: {count} registers — {}",
                pri::decode_pri_error(**pattern)
            );
        }

        // Phase 4: Try enabling clocks through accessible registers
        eprintln!("╠══ PLL Enable Attempts ═════════════════════════════════════╣");

        // The PCLOCK_BYPASS register is accessible — try different bypass modes
        let bypass = r(0x137020);
        eprintln!("║ PCLOCK_BYPASS before: {bypass:#010x}");

        // Try enabling various PLL control bits
        let attempts: &[(usize, u32, &str)] = &[
            // NVPLL control — try enable bit
            (0x137050, 0x00000001, "NVPLL_CTL enable"),
            (0x137050, 0x00000009, "NVPLL_CTL enable+current"),
            // Memory PLL — try enable
            (0x137100, 0x00000001, "MEMPLL_CTL enable"),
            (0x137100, 0x00000003, "MEMPLL_CTL enable+bypass"),
            // PCLOCK bypass — try different modes
            (0x137020, 0x00030011, "BYPASS mode +1"),
            (0x137020, 0x00010010, "BYPASS no upper"),
            (0x137020, 0x00030000, "BYPASS mask only"),
            // CLK domain at 0x132000 range (from envytools PCLOCK starts at 0x130000)
            (0x132000, 0x00000001, "CLK_BASE enable"),
            (0x132004, 0x00000001, "CLK_BASE+4 enable"),
        ];

        for &(reg, val, desc) in attempts {
            let before = r(reg);
            let before_err = pri::is_pri_error(before);
            if before_err {
                eprintln!("║ {desc}: [{reg:#08x}] is faulted ({before:#010x}), writing anyway...");
            }
            w(reg, val);
            std::thread::sleep(std::time::Duration::from_millis(10));
            let after = r(reg);
            let pclock_status = r(0x137000);

            let changed = if before_err {
                "was faulted"
            } else if before == after {
                "unchanged"
            } else {
                "CHANGED"
            };
            let pclock_alive = if pri::is_pri_error(pclock_status) {
                "dead"
            } else {
                "ALIVE"
            };
            eprintln!(
                "║   {desc}: {before:#010x} → {after:#010x} ({changed}) | PCLOCK[0]={pclock_status:#010x} ({pclock_alive})"
            );
        }

        // Phase 5: PRI recovery and re-scan
        eprintln!("╠══ Post-PLL Re-scan ════════════════════════════════════════╣");
        // Clear PRI faults
        w(0x12004C, 0x02);
        w(0x000100, r(0x000100) | (1 << 26));
        std::thread::sleep(std::time::Duration::from_millis(50));

        // Re-probe critical domains
        let domains: &[(usize, &str)] = &[
            (0x137000, "PCLOCK"),
            (0x137050, "NVPLL"),
            (0x137100, "MEMPLL"),
            (0x17E200, "LTC0"),
            (0x9A0000, "FBPA0"),
            (0x9A4000, "FBPA1"),
            (0x9A8000, "FBPA2"),
            (0x9AC000, "FBPA3"),
            (0x001200, "PBUS"),
            (0x100000, "PFB"),
        ];

        for &(off, name) in domains {
            let val = r(off);
            let status = if pri::is_pri_error(val) {
                format!("FAULTED — {}", pri::decode_pri_error(val))
            } else {
                format!("ALIVE ({val:#010x})")
            };
            eprintln!("║ {name:12} [{off:#08x}]: {status}");
        }

        // Phase 6: Deeper CLK range scan (0x130000-0x133000 = core CLK block)
        eprintln!("╠══ CLK Block Scan (0x130000-0x133000) ═════════════════════╣");
        let mut clk_live = Vec::new();
        for off in (0x130000..0x133000).step_by(4) {
            let val = r(off);
            if !pri::is_pri_error(val) && val != 0xDEAD_DEAD {
                clk_live.push((off, val));
            }
        }
        eprintln!("║ Live CLK registers: {}", clk_live.len());
        for &(off, val) in &clk_live {
            eprintln!("║   [{off:#08x}] = {val:#010x}");
        }

        eprintln!("╚══════════════════════════════════════════════════════════════╝");
    }
}
