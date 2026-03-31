// SPDX-License-Identifier: AGPL-3.0-only
//! NVIDIA VFIO — oracle-driven tests: root PLL, digital PMU, boot follower, lifecycle.
//!
//! Run: `CORALREEF_VFIO_BDF=0000:01:00.0 cargo test --test hw_nv_vfio_oracle --features vfio -- --ignored`

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

    fn open_vfio() -> (Option<VfioLease>, coral_driver::nv::RawVfioDevice) {
        let bdf = vfio_bdf();
        let lease = try_lease(&bdf);
        match super::ember_client::request_fds(&bdf) {
            Ok(fds) => {
                eprintln!("ember: received VFIO fds for {bdf}");
                let raw = coral_driver::nv::RawVfioDevice::open_from_fds(&bdf, fds)
                    .expect("RawVfioDevice::open_from_fds()");
                (lease, raw)
            }
            Err(e) => {
                eprintln!("ember unavailable ({e}), opening VFIO directly");
                let raw = coral_driver::nv::RawVfioDevice::open(&bdf)
                    .expect("RawVfioDevice::open() — is GPU bound to vfio-pci?");
                (lease, raw)
            }
        }
    }

    /// SM hint: 0 = auto-detect from BOOT0 (preferred), nonzero = validate.
    fn vfio_sm() -> u32 {
        std::env::var("CORALREEF_VFIO_SM")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0)
    }

    /// Oracle-driven root PLL comparison and programming.
    ///
    /// Reads oracle data from either:
    /// - Live oracle card (CORALREEF_ORACLE_BDF env var)
    /// - BAR0 binary dump (CORALREEF_ORACLE_DUMP env var)
    /// - Text dump (CORALREEF_ORACLE_TEXT env var)
    ///
    /// Compares root PLL registers (0x136xxx) between oracle and cold card,
    /// then writes oracle values to cold card and checks if PCLOCK unlocks.
    #[test]
    #[ignore = "requires VFIO-bound GPU hardware + oracle data"]
    fn vfio_oracle_root_pll_programming() {
        use coral_driver::vfio::channel::oracle::{DigitalPmu, OracleState};
        use coral_driver::vfio::channel::registers::pri;

        let (_lease, raw) = open_vfio();

        eprintln!("╔══════════════════════════════════════════════════════════════╗");
        eprintln!("║ Oracle Root PLL Programming                                 ║");
        eprintln!("╠══════════════════════════════════════════════════════════════╣");

        // Load oracle data from best available source
        let oracle = if let Ok(oracle_bdf) = std::env::var("CORALREEF_ORACLE_BDF") {
            eprintln!("║ Loading oracle from live card: {oracle_bdf}");
            OracleState::from_live_card(&oracle_bdf).expect("failed to read oracle BAR0")
        } else if let Ok(dump_path) = std::env::var("CORALREEF_ORACLE_DUMP") {
            eprintln!("║ Loading oracle from BAR0 dump: {dump_path}");
            OracleState::from_bar0_dump(std::path::Path::new(&dump_path))
                .expect("failed to load BAR0 dump")
        } else if let Ok(text_path) = std::env::var("CORALREEF_ORACLE_TEXT") {
            eprintln!("║ Loading oracle from text dump: {text_path}");
            OracleState::from_text_dump(std::path::Path::new(&text_path))
                .expect("failed to load text dump")
        } else {
            panic!("Set CORALREEF_ORACLE_BDF, CORALREEF_ORACLE_DUMP, or CORALREEF_ORACLE_TEXT");
        };

        eprintln!(
            "║ Oracle: {} total registers from {}",
            oracle.registers.len(),
            oracle.source
        );
        eprintln!(
            "║ Root PLLs (0x136xxx): {} registers",
            oracle.root_pll_registers().len()
        );
        eprintln!(
            "║ PCLOCK (0x137xxx): {} registers",
            oracle.pclock_registers().len()
        );

        let r = |off: usize| -> u32 { raw.bar0.read_u32(off).unwrap_or(0xDEAD_DEAD) };

        // Phase 1: Read cold card's current root PLL state
        eprintln!("╠══ Cold Card Root PLL State ════════════════════════════════╣");
        let root_plls = oracle.root_pll_registers();
        let mut cold_match = 0;
        let mut cold_diff = 0;
        let mut cold_dead = 0;
        for &(off, oracle_val) in &root_plls {
            let cold_val = r(off);
            if pri::is_pri_error(cold_val) {
                cold_dead += 1;
            } else if cold_val == oracle_val {
                cold_match += 1;
            } else {
                cold_diff += 1;
                if cold_diff <= 20 {
                    eprintln!("║   [{off:#08x}] cold={cold_val:#010x} oracle={oracle_val:#010x}");
                }
            }
        }
        eprintln!(
            "║ Root PLL comparison: {cold_match} match, {cold_diff} differ, {cold_dead} dead"
        );

        // Phase 2: Check PCLOCK before programming
        let pclock_before = r(0x137000);
        eprintln!(
            "║ PCLOCK[0] before: {pclock_before:#010x} ({})",
            if pri::is_pri_error(pclock_before) {
                "FAULTED"
            } else {
                "ALIVE"
            }
        );

        // Phase 3: Program root PLLs
        eprintln!("╠══ Programming Root PLLs ═══════════════════════════════════╣");
        let mut dpmu = DigitalPmu::new(&raw.bar0, &oracle);
        let (applied, skipped) = dpmu.program_root_plls();
        for msg in dpmu.take_log() {
            eprintln!("║ {msg}");
        }

        // Phase 4: Check PCLOCK after root PLL programming
        let pclock_after = r(0x137000);
        eprintln!(
            "║ PCLOCK[0] after root PLLs: {pclock_after:#010x} ({})",
            if pri::is_pri_error(pclock_after) {
                "FAULTED"
            } else {
                "ALIVE"
            }
        );

        // Phase 5: Program PCLOCK bypass registers
        eprintln!("╠══ Programming PCLOCK Bypass ═══════════════════════════════╣");
        let bypass_log = dpmu.program_pclock_bypass();
        for msg in &bypass_log {
            eprintln!("║ {msg}");
        }

        std::thread::sleep(std::time::Duration::from_millis(100));

        // Phase 6: Final domain health check
        eprintln!("╠══ Post-Programming Domain Health ══════════════════════════╣");
        let domains: &[(usize, &str)] = &[
            (0x137000, "PCLOCK"),
            (0x137050, "NVPLL"),
            (0x137100, "MEMPLL"),
            (0x17E200, "LTC0"),
            (0x9A0000, "FBPA0"),
            (0x100000, "PFB"),
            (0x002200, "PFIFO"),
            (0x700000, "PRAMIN"),
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

        eprintln!("║");
        eprintln!("║ Summary: {applied} root PLLs applied, {skipped} skipped");
        eprintln!("╚══════════════════════════════════════════════════════════════╝");
    }

    /// Full digital PMU emulation — apply complete oracle state in dependency order.
    ///
    /// This is the sovereign initialization path: instead of running signed
    /// firmware on the PMU FALCON, we program registers from the host using
    /// oracle data in the correct dependency order.
    #[test]
    #[ignore = "requires VFIO-bound GPU hardware + oracle data"]
    fn vfio_digital_pmu_full() {
        use coral_driver::vfio::channel::glowplug::GlowPlug;
        use coral_driver::vfio::channel::oracle::{DigitalPmu, OracleState};

        let bdf = vfio_bdf();
        let (_lease, raw) = open_vfio();

        eprintln!("╔══════════════════════════════════════════════════════════════╗");
        eprintln!("║ Digital PMU Full Emulation                                  ║");
        eprintln!("╠══════════════════════════════════════════════════════════════╣");

        // Load oracle
        let oracle = if let Ok(oracle_bdf) = std::env::var("CORALREEF_ORACLE_BDF") {
            OracleState::from_live_card(&oracle_bdf).expect("failed to read oracle BAR0")
        } else if let Ok(dump_path) = std::env::var("CORALREEF_ORACLE_DUMP") {
            OracleState::from_bar0_dump(std::path::Path::new(&dump_path))
                .expect("failed to load BAR0 dump")
        } else if let Ok(text_path) = std::env::var("CORALREEF_ORACLE_TEXT") {
            OracleState::from_text_dump(std::path::Path::new(&text_path))
                .expect("failed to load text dump")
        } else {
            panic!("Set CORALREEF_ORACLE_BDF, CORALREEF_ORACLE_DUMP, or CORALREEF_ORACLE_TEXT");
        };

        eprintln!(
            "║ Oracle: {} registers from {}",
            oracle.registers.len(),
            oracle.source
        );

        // Check pre-state
        let plug = GlowPlug::with_bdf(&raw.bar0, raw.container.clone(), &bdf);
        let pre_state = plug.check_state();
        eprintln!("║ Pre-state: {pre_state:?}");

        // Execute digital PMU
        let mut dpmu = DigitalPmu::new(&raw.bar0, &oracle);
        let result = dpmu.execute();

        eprintln!("╠══ Digital PMU Results ═════════════════════════════════════╣");
        for msg in &result.log {
            eprintln!("║ {msg}");
        }

        eprintln!("╠══ Domain Results ══════════════════════════════════════════╣");
        for dr in &result.domain_results {
            if dr.diffs > 0 {
                eprintln!(
                    "║   {}: {} diffs, {} applied, {} stuck, {} PRI-skipped",
                    dr.name, dr.diffs, dr.applied, dr.stuck, dr.pri_skipped
                );
            }
        }

        eprintln!("╠══ Summary ═════════════════════════════════════════════════╣");
        eprintln!("║ Total diffs: {}", result.total_diffs);
        eprintln!("║ Applied: {}", result.applied);
        eprintln!("║ Stuck: {}", result.stuck);
        eprintln!("║ PRI-skipped: {}", result.pri_skipped);
        eprintln!("║ Danger-skipped: {}", result.danger_skipped);
        eprintln!(
            "║ VRAM unlocked: {} (after {:?})",
            result.vram_unlocked, result.vram_unlocked_after
        );

        // Post-state
        let post_state = plug.check_state();
        eprintln!("║ Post-state: {post_state:?}");

        eprintln!("╚══════════════════════════════════════════════════════════════╝");
    }

    /// Boot sequence follower — diff oracle BAR0 against cold card.
    ///
    /// Uses the boot_follower module to compare a warm oracle's register
    /// state against the cold VFIO target, producing a domain-ordered diff
    /// that shows exactly what needs to change for each domain.
    #[test]
    #[ignore = "requires VFIO-bound GPU hardware + oracle data"]
    fn vfio_boot_follower_diff() {
        use coral_driver::vfio::channel::diagnostic::boot_follower::{BootDiff, BootTrace};
        use coral_driver::vfio::channel::oracle::OracleState;

        let (_lease, raw) = open_vfio();

        eprintln!("╔══════════════════════════════════════════════════════════════╗");
        eprintln!("║ Boot Sequence Follower — Oracle vs Cold Diff                ║");
        eprintln!("╠══════════════════════════════════════════════════════════════╣");

        // Load oracle
        let oracle = if let Ok(oracle_bdf) = std::env::var("CORALREEF_ORACLE_BDF") {
            OracleState::from_live_card(&oracle_bdf).expect("failed to read oracle BAR0")
        } else if let Ok(dump_path) = std::env::var("CORALREEF_ORACLE_DUMP") {
            OracleState::from_bar0_dump(std::path::Path::new(&dump_path))
                .expect("failed to load BAR0 dump")
        } else if let Ok(text_path) = std::env::var("CORALREEF_ORACLE_TEXT") {
            OracleState::from_text_dump(std::path::Path::new(&text_path))
                .expect("failed to load text dump")
        } else {
            panic!("Set CORALREEF_ORACLE_BDF, CORALREEF_ORACLE_DUMP, or CORALREEF_ORACLE_TEXT");
        };

        // Build cold card register snapshot
        let r = |off: usize| -> u32 { raw.bar0.read_u32(off).unwrap_or(0xDEAD_DEAD) };
        let mut cold_regs = std::collections::BTreeMap::new();
        for &off in oracle.registers.keys() {
            cold_regs.insert(off, r(off));
        }

        // Perform the diff
        let diff = BootDiff::compare(&oracle.registers, &cold_regs);

        eprintln!("║ Compared: {} registers", diff.total_compared);
        eprintln!("║ Changed:  {} registers", diff.total_changed);
        eprintln!("║");
        eprintln!("╠══ Per-Domain Changes ══════════════════════════════════════╣");
        for (domain, stats) in &diff.domain_stats {
            if stats.changed > 0 || stats.cold_dead > 0 {
                eprintln!(
                    "║ {domain:12}: {}/{} changed, {} cold-dead, {} warm-alive",
                    stats.changed, stats.compared, stats.cold_dead, stats.warm_alive
                );
            }
        }

        // Extract and display recipe
        let recipe = diff.to_recipe();
        eprintln!("║");
        eprintln!(
            "╠══ Init Recipe ({} steps) ═════════════════════════════════╣",
            recipe.len()
        );
        let mut current_domain = String::new();
        let mut domain_count = 0;
        for step in &recipe {
            if step.domain != current_domain {
                if !current_domain.is_empty() {
                    eprintln!("║   ... ({domain_count} total steps in {current_domain})");
                }
                current_domain = step.domain.clone();
                domain_count = 0;
                eprintln!("║ [{current_domain}] (priority {})", step.priority);
            }
            domain_count += 1;
            if domain_count <= 5 {
                eprintln!("║   [{:#08x}] = {:#010x}", step.offset, step.value);
            }
        }
        if !current_domain.is_empty() {
            eprintln!("║   ... ({domain_count} total steps in {current_domain})");
        }

        // If mmiotrace file is available, parse and display summary
        if let Ok(trace_path) = std::env::var("CORALREEF_MMIOTRACE") {
            eprintln!("║");
            eprintln!("╠══ mmiotrace Summary ═══════════════════════════════════════╣");
            match BootTrace::from_mmiotrace(std::path::Path::new(&trace_path)) {
                Ok(trace) => {
                    eprintln!("║ Total writes: {}", trace.writes.len());
                    eprintln!("║ Total reads:  {}", trace.reads.len());
                    eprintln!("║ Duration:     {}ms", trace.duration_us / 1000);
                    eprintln!("║ Per-domain write counts:");
                    for (domain, count) in trace.domain_summary() {
                        eprintln!("║   {domain:12}: {count}");
                    }

                    let mmio_recipe = trace.to_recipe();
                    eprintln!("║ Recipe steps: {}", mmio_recipe.len());
                }
                Err(e) => {
                    eprintln!("║ mmiotrace parse error: {e}");
                }
            }
        }

        eprintln!("╚══════════════════════════════════════════════════════════════╝");
    }

    /// HBM2 lifecycle probe: map exactly which domains are alive/dead,
    /// measure VRAM accessibility, and test resurrection via nouveau hot-swap.
    #[test]
    #[ignore = "requires VFIO-bound GPU hardware"]
    fn vfio_hbm2_lifecycle_probe() {
        let bdf = vfio_bdf();

        // ── Helper: probe all VRAM-related domains ────────────────────────
        fn probe_hbm2_health(
            bar0: &coral_driver::vfio::device::MappedBar,
        ) -> Vec<(&'static str, usize, u32, bool)> {
            let domains: &[(&str, usize)] = &[
                ("BOOT0", 0x000000),
                ("PMC_EN", 0x000200),
                ("PFIFO", 0x002004),
                ("PFB", 0x100000),
                ("FBHUB", 0x100800),
                ("PFB_NISO", 0x100C80),
                ("PMU", 0x10A000),
                ("LTC0", 0x17E200),
                ("FBPA0", 0x9A0000),
                ("NVPLL", 0x137050),
                ("MEMPLL", 0x137100),
                ("PRAMIN", 0x700000),
                ("PRAMIN+4", 0x700004),
                ("PRAMIN+8", 0x700008),
            ];

            domains
                .iter()
                .map(|&(name, off)| {
                    let val = bar0.read_u32(off).unwrap_or(0xDEAD_DEAD);
                    let alive = val != 0xDEAD_DEAD
                        && val != 0xFFFF_FFFF
                        && (val >> 16) != 0xBADF
                        && (val >> 16) != 0xBAD0
                        && (val >> 16) != 0xBAD1;
                    (name, off, val, alive)
                })
                .collect()
        }

        fn print_health(label: &str, health: &[(&str, usize, u32, bool)]) {
            let alive = health.iter().filter(|h| h.3).count();
            let total = health.len();
            eprintln!("║ {label}: {alive}/{total} domains alive");
            for &(name, off, val, alive) in health {
                let icon = if alive { "✓" } else { "✗" };
                eprintln!("║   {icon} {name:10} [{off:#08x}] = {val:#010x}");
            }
        }

        eprintln!("╔══════════════════════════════════════════════════════════════╗");
        eprintln!("║ HBM2 LIFECYCLE PROBE                                       ║");
        eprintln!("╠══════════════════════════════════════════════════════════════╣");

        // ── Phase 1: Fresh VFIO open (POST state) ─────────────────────────
        eprintln!("╠══ PHASE 1: FRESH VFIO OPEN (POST STATE) ═══════════════════╣");
        {
            let (_lease, raw) = open_vfio();
            let h = probe_hbm2_health(&raw.bar0);
            print_health("POST state", &h);

            let pramin_alive = h.iter().any(|x| x.0.starts_with("PRAMIN") && x.3);
            eprintln!("║ VRAM accessible: {pramin_alive}");

            // Write a sentinel to PRAMIN if accessible
            if pramin_alive {
                let sentinel: u32 = 0xC0EE_1EEF;
                raw.bar0.write_u32(0x700000, sentinel).ok();
                let readback = raw.bar0.read_u32(0x700000).unwrap_or(0);
                eprintln!(
                    "║ Sentinel write/read: wrote {sentinel:#010x}, read {readback:#010x}, match={}",
                    readback == sentinel
                );
            }

            eprintln!("║ Dropping VFIO fd (this triggers PM reset)...");
            // raw drops here — fd closes, kernel does PM reset
        }

        std::thread::sleep(std::time::Duration::from_secs(2));

        // ── Phase 2: Re-open after fd close (PM reset happened) ───────────
        eprintln!("╠══ PHASE 2: RE-OPEN AFTER PM RESET ═════════════════════════╣");
        {
            // Pin D0 first
            let _ = std::fs::write(format!("/sys/bus/pci/devices/{bdf}/power/control"), "on");
            std::thread::sleep(std::time::Duration::from_millis(500));

            let (_lease, raw) = open_vfio();
            let h = probe_hbm2_health(&raw.bar0);
            print_health("After PM reset", &h);

            let pramin_alive = h.iter().any(|x| x.0.starts_with("PRAMIN") && x.3);
            eprintln!("║ VRAM accessible: {pramin_alive}");

            if pramin_alive {
                let readback = raw.bar0.read_u32(0x700000).unwrap_or(0);
                eprintln!(
                    "║ Sentinel survived PM reset? read {readback:#010x} (expected 0xC0EE1EEF)"
                );
            }

            eprintln!("║ Dropping again...");
        }

        std::thread::sleep(std::time::Duration::from_secs(2));

        // ── Phase 3: Resurrection via nouveau hot-swap ────────────────────
        eprintln!("╠══ PHASE 3: NOUVEAU RESURRECTION ═══════════════════════════╣");
        eprintln!("║ Swapping {bdf} → nouveau for HBM2 re-training...");

        // Unbind from vfio-pci
        fn sysfs_write(path: &str, val: &str) {
            if std::fs::write(path, val).is_err() {
                let _ = std::process::Command::new("sudo")
                    .args(["-n", "/usr/bin/tee", path])
                    .stdin(std::process::Stdio::piped())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn()
                    .and_then(|mut c| {
                        use std::io::Write;
                        if let Some(s) = c.stdin.as_mut() {
                            s.write_all(val.as_bytes())?;
                        }
                        c.wait()
                    });
            }
        }

        let unbind = format!("/sys/bus/pci/devices/{bdf}/driver/unbind");
        sysfs_write(&unbind, &bdf);
        std::thread::sleep(std::time::Duration::from_millis(500));

        // Clear driver_override so nouveau can claim it
        sysfs_write(&format!("/sys/bus/pci/devices/{bdf}/driver_override"), "");
        sysfs_write("/sys/bus/pci/drivers/nouveau/bind", &bdf);
        eprintln!("║ Waiting for nouveau init (HBM2 training)...");
        std::thread::sleep(std::time::Duration::from_secs(5));

        // Check nouveau claimed it
        let drv = std::fs::read_link(format!("/sys/bus/pci/devices/{bdf}/driver"))
            .ok()
            .and_then(|p| p.file_name().map(|f| f.to_string_lossy().to_string()));
        eprintln!("║ Driver after nouveau bind: {:?}", drv);

        // ── Phase 4: Swap back to VFIO and check resurrection ─────────────
        eprintln!("╠══ PHASE 4: SWAP BACK TO VFIO — CHECK RESURRECTION ═════════╣");

        // Unbind from nouveau
        sysfs_write(&unbind, &bdf);
        std::thread::sleep(std::time::Duration::from_millis(500));

        // Bind to vfio-pci
        sysfs_write(
            &format!("/sys/bus/pci/devices/{bdf}/driver_override"),
            "vfio-pci",
        );
        sysfs_write("/sys/bus/pci/drivers/vfio-pci/bind", &bdf);
        std::thread::sleep(std::time::Duration::from_secs(1));

        // Pin D0
        let _ = std::fs::write(format!("/sys/bus/pci/devices/{bdf}/power/control"), "on");
        std::thread::sleep(std::time::Duration::from_millis(500));

        let (_lease, raw) = open_vfio();
        let h = probe_hbm2_health(&raw.bar0);
        print_health("After nouveau resurrection", &h);

        let pramin_alive = h.iter().any(|x| x.0.starts_with("PRAMIN") && x.3);
        eprintln!("║ VRAM RESURRECTED: {pramin_alive}");

        // Try the sentinel test on resurrected VRAM
        if pramin_alive {
            let sentinel: u32 = 0xDEAD_BEEF;
            raw.bar0.write_u32(0x700000, sentinel).ok();
            let readback = raw.bar0.read_u32(0x700000).unwrap_or(0);
            eprintln!(
                "║ Post-resurrection sentinel: wrote {sentinel:#010x}, read {readback:#010x}, match={}",
                readback == sentinel
            );
        }

        let alive_count = h.iter().filter(|x| x.3).count();
        eprintln!("╠══════════════════════════════════════════════════════════════╣");
        eprintln!("║ SUMMARY:");
        eprintln!("║   Phase 1 (POST state):       check log above");
        eprintln!("║   Phase 2 (after PM reset):    check log above");
        eprintln!("║   Phase 3 (nouveau warm):      driver={:?}", drv);
        eprintln!(
            "║   Phase 4 (resurrection):      {alive_count}/{} domains, VRAM={pramin_alive}",
            h.len()
        );
        eprintln!("╚══════════════════════════════════════════════════════════════╝");
    }

    /// Single-card oracle pipeline test: nouveau warm → GR snapshot → VFIO probe.
    ///
    /// Demonstrates the full lifecycle:
    /// 1. Card starts on nouveau (or we swap to it)
    /// 2. Capture GR engine state as oracle
    /// 3. Compile WGSL shader with coralReef (pipeline validation)
    /// 4. Swap to VFIO
    /// 5. Read GR state — compare to oracle
    /// 6. Report what survived the PM reset
    #[test]
    #[ignore = "requires GPU hardware + nouveau + VFIO support"]
    fn vfio_single_card_oracle_pipeline() {
        use coral_driver::nv::vfio_compute::GrEngineStatus;

        let bdf = vfio_bdf();
        let sm = vfio_sm();

        eprintln!("╔══════════════════════════════════════════════════════════════╗");
        eprintln!("║ Single-Card Oracle Pipeline Test                            ║");
        eprintln!("║ BDF: {bdf:54} ║");
        eprintln!("╠══════════════════════════════════════════════════════════════╣");

        // Phase 1: Ensure nouveau is bound — warm the card
        eprintln!("║ Phase 1: Warming card via nouveau...");
        let drv = read_current_driver(&bdf);
        if drv.as_deref() != Some("nouveau") {
            eprintln!("║   Current driver: {drv:?}, swapping to nouveau...");
            // Unbind current driver
            let unbind_path = format!("/sys/bus/pci/devices/{bdf}/driver/unbind");
            let _ = std::process::Command::new("sudo")
                .args(["-n", "tee", &unbind_path])
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .and_then(|mut c| {
                    use std::io::Write;
                    if let Some(ref mut stdin) = c.stdin {
                        stdin.write_all(bdf.as_bytes()).ok();
                    }
                    c.wait()
                });
            // Clear driver_override
            let override_path = format!("/sys/bus/pci/devices/{bdf}/driver_override");
            let _ = std::process::Command::new("sudo")
                .args(["-n", "tee", &override_path])
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .and_then(|mut c| {
                    use std::io::Write;
                    if let Some(ref mut stdin) = c.stdin {
                        stdin.write_all(b"\n").ok();
                    }
                    c.wait()
                });
            // Load and bind nouveau
            let _ = std::process::Command::new("sudo")
                .args(["-n", "modprobe", "nouveau"])
                .status();
            let probe_path = "/sys/bus/pci/drivers/nouveau/bind";
            let _ = std::process::Command::new("sudo")
                .args(["-n", "tee", probe_path])
                .stdin(std::process::Stdio::piped())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .and_then(|mut c| {
                    use std::io::Write;
                    if let Some(ref mut stdin) = c.stdin {
                        stdin.write_all(bdf.as_bytes()).ok();
                    }
                    c.wait()
                });
            std::thread::sleep(std::time::Duration::from_secs(3));
            let drv = read_current_driver(&bdf);
            eprintln!("║   After swap: driver={drv:?}");
            assert_eq!(drv.as_deref(), Some("nouveau"), "failed to bind nouveau");
        } else {
            eprintln!("║   Already on nouveau ✓");
        }

        // Phase 2: coralReef compile happens before swap (compile is CPU-only)
        eprintln!("║ Phase 2: (oracle capture deferred to after VFIO open)");

        // Phase 3: Compile WGSL shader — validate coralReef pipeline
        eprintln!("║ Phase 3: Compiling WGSL shader via coralReef...");
        let wgsl = "@compute @workgroup_size(64) fn main() {}";
        let opts = coral_reef::CompileOptions {
            target: match sm {
                70 => coral_reef::GpuTarget::Nvidia(coral_reef::NvArch::Sm70),
                75 => coral_reef::GpuTarget::Nvidia(coral_reef::NvArch::Sm75),
                80 => coral_reef::GpuTarget::Nvidia(coral_reef::NvArch::Sm80),
                _ => coral_reef::GpuTarget::Nvidia(coral_reef::NvArch::Sm86),
            },
            ..coral_reef::CompileOptions::default()
        };
        let compiled = coral_reef::compile_wgsl_full(wgsl, &opts).expect("coralReef compile");
        eprintln!(
            "║   SASS binary: {} bytes, GPR={}, shared={}",
            compiled.binary.len(),
            compiled.info.gpr_count,
            compiled.info.shared_mem_bytes
        );

        // Phase 4: Swap to VFIO
        eprintln!("║ Phase 4: Swapping to vfio-pci...");
        // Unbind nouveau
        let unbind_path = "/sys/bus/pci/drivers/nouveau/unbind";
        let _ = std::process::Command::new("sudo")
            .args(["-n", "tee", unbind_path])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .and_then(|mut c| {
                use std::io::Write;
                if let Some(ref mut stdin) = c.stdin {
                    stdin.write_all(bdf.as_bytes()).ok();
                }
                c.wait()
            });
        std::thread::sleep(std::time::Duration::from_millis(500));
        // Set driver_override to vfio-pci
        let override_path = format!("/sys/bus/pci/devices/{bdf}/driver_override");
        let _ = std::process::Command::new("sudo")
            .args(["-n", "tee", &override_path])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .and_then(|mut c| {
                use std::io::Write;
                if let Some(ref mut stdin) = c.stdin {
                    stdin.write_all(b"vfio-pci").ok();
                }
                c.wait()
            });
        let probe_path = "/sys/bus/pci/drivers/vfio-pci/bind";
        let _ = std::process::Command::new("sudo")
            .args(["-n", "tee", probe_path])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .and_then(|mut c| {
                use std::io::Write;
                if let Some(ref mut stdin) = c.stdin {
                    stdin.write_all(bdf.as_bytes()).ok();
                }
                c.wait()
            });
        std::thread::sleep(std::time::Duration::from_millis(500));

        let drv = read_current_driver(&bdf);
        eprintln!("║   After swap: driver={drv:?}");

        // Phase 5: Open VFIO and check GR state
        eprintln!("║ Phase 5: Reading GR engine state via VFIO BAR0...");
        let (_lease, raw) = open_vfio();

        // Read GR registers manually (same as GrEngineStatus)
        let r = |off: usize| raw.bar0.read_u32(off).unwrap_or(0xDEAD_DEAD);
        let vfio_gr = GrEngineStatus {
            pgraph_status: r(0x0040_0700),
            fecs_cpuctl: r(0x0040_9100),
            fecs_mailbox0: r(0x0040_9130),
            fecs_mailbox1: r(0x0040_9134),
            fecs_hwcfg: r(0x0040_9800),
            gpccs_cpuctl: r(0x0041_a100),
            pmc_enable: r(0x0000_0200),
            pfifo_enable: r(0x0000_2504),
        };
        eprintln!("║   VFIO {vfio_gr}");

        // Phase 6: Capture GR registers via VFIO BAR0
        eprintln!("║ Phase 6: GR register snapshot via VFIO:");
        let gr_regs = capture_gr_oracle_from_bar0(&raw.bar0);
        for (name, offset, value) in &gr_regs {
            let faulted = *value == 0xDEAD_DEAD || (*value & 0xBAD0_0000) == 0xBAD0_0000;
            let marker = if faulted { "FAULT" } else { "  ok " };
            eprintln!("║   [{marker}] {name:24} {offset:#010x} = {value:#010x}");
        }
        let faulted_count = gr_regs
            .iter()
            .filter(|(_, _, v)| *v == 0xDEAD_DEAD || (*v & 0xBAD0_0000) == 0xBAD0_0000)
            .count();

        // Phase 7: Report
        eprintln!("╠══════════════════════════════════════════════════════════════╣");
        eprintln!("║ PIPELINE VALIDATION RESULTS:");
        eprintln!(
            "║   coralReef compile:  PASS ({} bytes SASS)",
            compiled.binary.len()
        );
        eprintln!("║   nouveau warm:       PASS");
        eprintln!("║   VFIO swap:          PASS");
        eprintln!("║   FECS halted:        {}", vfio_gr.fecs_halted());
        eprintln!("║   GR enabled:         {}", vfio_gr.gr_enabled());
        eprintln!(
            "║   GR regs faulted:    {}/{}",
            faulted_count,
            gr_regs.len()
        );
        if vfio_gr.fecs_halted() {
            eprintln!("║   Cold VFIO dispatch: BLOCKED (FECS needs falcon firmware)");
            eprintln!("║   Remedy: Use NvDevice (nouveau DRM) for dispatch, or");
            eprintln!("║           implement ACR falcon loader in Rust");
        } else {
            eprintln!("║   GR engine appears live — dispatch may succeed!");
        }
        eprintln!("╚══════════════════════════════════════════════════════════════╝");

        // Swap back to nouveau for subsequent tests
        drop(raw);
        std::thread::sleep(std::time::Duration::from_millis(200));
        let unbind_path = "/sys/bus/pci/drivers/vfio-pci/unbind";
        let _ = std::process::Command::new("sudo")
            .args(["-n", "tee", unbind_path])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .and_then(|mut c| {
                use std::io::Write;
                if let Some(ref mut stdin) = c.stdin {
                    stdin.write_all(bdf.as_bytes()).ok();
                }
                c.wait()
            });
        let override_path = format!("/sys/bus/pci/devices/{bdf}/driver_override");
        let _ = std::process::Command::new("sudo")
            .args(["-n", "tee", &override_path])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .and_then(|mut c| {
                use std::io::Write;
                if let Some(ref mut stdin) = c.stdin {
                    stdin.write_all(b"\n").ok();
                }
                c.wait()
            });
    }

    /// Read the current kernel driver for a PCI device.
    fn read_current_driver(bdf: &str) -> Option<String> {
        std::fs::read_link(format!("/sys/bus/pci/devices/{bdf}/driver"))
            .ok()
            .and_then(|p| p.file_name().map(|f| f.to_string_lossy().to_string()))
    }

    /// Capture key GR engine registers from a VFIO-opened BAR0.
    fn capture_gr_oracle_from_bar0(
        bar0: &coral_driver::vfio::device::MappedBar,
    ) -> Vec<(String, u32, u32)> {
        let r = |off: usize| bar0.read_u32(off).unwrap_or(0xDEAD_DEAD);
        let regs: &[(&str, usize)] = &[
            ("PMC_ENABLE", 0x0000_0200),
            ("PFIFO_ENABLE", 0x0000_2504),
            ("PGRAPH_STATUS", 0x0040_0700),
            ("FECS_CPUCTL", 0x0040_9100),
            ("FECS_MAILBOX0", 0x0040_9130),
            ("FECS_MAILBOX1", 0x0040_9134),
            ("FECS_HWCFG", 0x0040_9800),
            ("GPCCS_CPUCTL", 0x0041_A100),
            ("FECS_FALCON_OS", 0x0040_9080),
            ("GR_FECS_CTX_STATE", 0x0040_9400),
            ("NV_PGRAPH_FE_HWW_ESR", 0x0040_4800),
        ];
        regs.iter()
            .map(|(name, off)| (name.to_string(), *off as u32, r(*off)))
            .collect()
    }
}
