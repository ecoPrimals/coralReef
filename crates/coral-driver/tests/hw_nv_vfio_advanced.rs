// SPDX-License-Identifier: AGPL-3.0-or-later
//! NVIDIA VFIO — advanced tests: diagnostic matrix, HBM2 capture, metal discovery.
//!
//! Run: `CORALREEF_VFIO_BDF=0000:01:00.0 cargo test --test hw_nv_vfio_advanced --features vfio -- --ignored`

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
    fn vfio_devinit_pmu_probe() {
        use coral_driver::vfio::channel::devinit;
        use coral_driver::vfio::channel::glowplug::GlowPlug;

        let bdf = vfio_bdf();
        let oracle_bdf = std::env::var("CORALREEF_ORACLE_BDF").ok();
        let (_lease, raw) = open_vfio();

        // Phase 1: Check devinit status and PMU FALCON state
        eprintln!("╔══════════════════════════════════════════════════════════════╗");
        let status = devinit::DevinitStatus::probe(&raw.bar0);
        status.print_summary();

        // Phase 2: Read VBIOS — PROM first (sovereign), then sysfs, then file
        let rom_result = devinit::read_vbios_prom(&raw.bar0)
            .or_else(|e1| {
                eprintln!("║ PROM failed: {e1}");
                devinit::read_vbios_sysfs(&bdf)
            })
            .or_else(|e2| {
                eprintln!("║ sysfs ROM failed: {e2} — trying /tmp/titan_v_vbios.rom");
                devinit::read_vbios_file("/tmp/titan_v_vbios.rom")
            });

        match rom_result {
            Ok(rom) => {
                eprintln!("╠══ VBIOS ROM ═══════════════════════════════════════════════╣");
                eprintln!("║ ROM size: {} bytes ({} KB)", rom.len(), rom.len() / 1024);
                eprintln!("║ Signature: {:#04x} {:#04x}", rom[0], rom[1]);

                // Dump first few bytes for debugging
                eprintln!("║ First 32 bytes:");
                for chunk in rom[..32.min(rom.len())].chunks(16) {
                    let hex: Vec<String> = chunk.iter().map(|b| format!("{b:02x}")).collect();
                    eprintln!("║   {}", hex.join(" "));
                }

                // Scan for known ROM signatures
                eprintln!("║ Scanning for known structures:");
                for (i, w) in rom.windows(4).enumerate() {
                    if w == b"PCIR" || w == b"NPDS" || w == b"NPDE" || w == b"BIT\0" {
                        let sig = std::str::from_utf8(w).unwrap_or("????");
                        eprintln!("║   Found '{sig}' at offset {i:#06x}");
                    }
                }
                // Also scan for the 0xFF prefix BIT signature
                for (i, w) in rom.windows(5).enumerate() {
                    if w == b"\xffBIT\0" {
                        eprintln!("║   Found 0xFF+'BIT' at offset {i:#06x}");
                    }
                }

                // Phase 3: Parse BIT table
                match devinit::BitTable::parse(&rom) {
                    Ok(bit) => {
                        eprintln!("╠══ BIT TABLE ═══════════════════════════════════════════════╣");
                        eprintln!("║ Entries: {}", bit.entries.len());
                        for entry in &bit.entries {
                            let ch = if entry.id.is_ascii_graphic() {
                                entry.id as char
                            } else {
                                '?'
                            };
                            eprintln!(
                                "║   '{}' (0x{:02x})  ver={}  offset={:#06x}  size={}",
                                ch, entry.id, entry.version, entry.data_offset, entry.data_size
                            );
                        }

                        // Phase 4: Parse PMU firmware table
                        match devinit::parse_pmu_table(&rom, &bit) {
                            Ok(pmu_fws) => {
                                eprintln!(
                                    "╠══ PMU FIRMWARE TABLE ══════════════════════════════════════╣"
                                );
                                eprintln!("║ Entries: {}", pmu_fws.len());
                                for fw in &pmu_fws {
                                    let type_name = match fw.app_type {
                                        0x01 => "PRE_OS",
                                        0x04 => "DEVINIT",
                                        _ => "UNKNOWN",
                                    };
                                    eprintln!(
                                        "║   type={:#04x} ({type_name}) boot_pmu={:#x} code_pmu={:#x} init={:#x}",
                                        fw.app_type,
                                        fw.boot_addr_pmu,
                                        fw.code_addr_pmu,
                                        fw.init_addr_pmu,
                                    );
                                }

                                // Phase 5: If devinit is needed, try to execute it
                                if status.needs_post {
                                    eprintln!(
                                        "╠══ EXECUTING PMU DEVINIT ═══════════════════════════════════╣"
                                    );
                                    match devinit::execute_devinit(&raw.bar0, &rom) {
                                        Ok(true) => {
                                            eprintln!("║ DEVINIT COMPLETED! Checking VRAM...");
                                            std::thread::sleep(std::time::Duration::from_millis(
                                                100,
                                            ));

                                            let gp = GlowPlug::with_bdf(
                                                &raw.bar0,
                                                raw.container.clone(),
                                                &bdf,
                                            );
                                            let vram_ok = gp.check_vram();
                                            eprintln!("║ VRAM accessible: {vram_ok}");

                                            if vram_ok {
                                                eprintln!(
                                                    "║ *** SUCCESS: HBM2 training via PMU DEVINIT worked! ***"
                                                );
                                            } else {
                                                eprintln!(
                                                    "║ VRAM still dead. Devinit ran but HBM2 not trained."
                                                );
                                                eprintln!(
                                                    "║ Possible: devinit script didn't include memory training,"
                                                );
                                                eprintln!(
                                                    "║ or the FALCON wasn't properly authenticated."
                                                );
                                            }
                                        }
                                        Ok(false) => {
                                            eprintln!("║ Devinit not needed (already done).");
                                        }
                                        Err(e) => {
                                            eprintln!("║ DEVINIT FAILED: {e}");
                                        }
                                    }
                                } else {
                                    eprintln!("║ Devinit already complete — no need to run PMU.");
                                }
                            }
                            Err(e) => {
                                eprintln!("║ PMU table parse failed: {e}");
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("║ BIT table parse failed: {e}");
                    }
                }
            }
            Err(e) => {
                eprintln!("║ Cannot read VBIOS from any source: {e}");
            }
        }

        // Phase 6: Run GlowPlug with BDF + oracle for full sovereign warm-up
        eprintln!("╠══ FULL GLOWPLUG WITH DEVINIT + ORACLE ═════════════════════╣");
        let gp = if let Some(ref oracle) = oracle_bdf {
            eprintln!("║ Oracle card: {oracle}");
            GlowPlug::with_oracle(&raw.bar0, raw.container.clone(), &bdf, oracle)
        } else {
            GlowPlug::with_bdf(&raw.bar0, raw.container.clone(), &bdf)
        };
        let result = gp.full_init();
        for msg in &result.log {
            eprintln!("║ {msg}");
        }
        eprintln!(
            "║ Final state: {:?}  success={}",
            result.final_state, result.success
        );
        eprintln!("╚══════════════════════════════════════════════════════════════╝");
    }

    #[test]
    #[ignore = "requires VFIO-bound GPU hardware + nouveau-bound oracle card"]
    fn vfio_cross_card_fb_init_oracle() {
        use coral_driver::vfio::channel::ProbeInterpreter;
        use coral_driver::vfio::channel::glowplug::GlowPlug;
        use coral_driver::vfio::channel::memory_probe;

        let vfio_bdf = vfio_bdf();
        let oracle_bdf = std::env::var("CORALREEF_ORACLE_BDF")
            .expect("set CORALREEF_ORACLE_BDF=0000:XX:XX.X for the nouveau-bound oracle card");

        // ── Phase 1: Read oracle (nouveau-warm) NV_PFB via sysfs BAR0 ──
        let resource0_path = format!("/sys/bus/pci/devices/{oracle_bdf}/resource0");
        let oracle_bar0 = std::fs::OpenOptions::new()
            .read(true)
            .open(&resource0_path)
            .unwrap_or_else(|e| panic!("cannot open {resource0_path}: {e}"));

        let bar0_size: usize = 16 * 1024 * 1024; // 16 MB
        let oracle_ptr = unsafe {
            rustix::mm::mmap(
                std::ptr::null_mut(),
                bar0_size,
                rustix::mm::ProtFlags::READ,
                rustix::mm::MapFlags::SHARED,
                &oracle_bar0,
                0,
            )
            .expect("mmap oracle BAR0")
        };

        let oracle_read = |offset: usize| -> u32 {
            assert!(offset + 4 <= bar0_size);
            unsafe { std::ptr::read_volatile(oracle_ptr.cast::<u8>().add(offset).cast::<u32>()) }
        };

        // Read NV_PFB register region from oracle
        let pfb_start = 0x0010_0000_usize;
        let pfb_end = 0x0010_1000_usize;
        let mut oracle_pfb: Vec<(usize, u32)> = Vec::new();
        for offset in (pfb_start..pfb_end).step_by(4) {
            let val = oracle_read(offset);
            if val != 0xDEAD_DEAD && val != 0xBAD0_DA00 {
                oracle_pfb.push((offset, val));
            }
        }

        // Also read extended FB regions
        let fb_ext_ranges: &[(usize, usize)] = &[
            (0x0010_0C00, 0x0010_0D00), // MMU region
            (0x0010_0E00, 0x0010_0F00), // Fault buffer region
            (0x0010_9000, 0x0010_A000), // FB misc
            (0x0010_F000, 0x0011_0000), // FB config
            (0x0009_A000, 0x0009_A100), // PTOP region
        ];
        let mut oracle_ext: Vec<(usize, u32)> = Vec::new();
        for &(start, end) in fb_ext_ranges {
            for offset in (start..end).step_by(4) {
                if offset + 4 <= bar0_size {
                    let val = oracle_read(offset);
                    if val != 0 && val != 0xDEAD_DEAD && val != 0xBAD0_DA00 {
                        oracle_ext.push((offset, val));
                    }
                }
            }
        }

        // Also snapshot the PMC and PFIFO oracle state for reference
        let oracle_pmc = oracle_read(0x200);
        let oracle_pfifo = oracle_read(0x2200);
        let oracle_boot0 = oracle_read(0x0);

        eprintln!("╔══ ORACLE CARD ({oracle_bdf}, nouveau) ═════════════════════╗");
        eprintln!("║ BOOT0={oracle_boot0:#010x} PMC={oracle_pmc:#010x} PFIFO={oracle_pfifo:#010x}");
        eprintln!(
            "║ NV_PFB: {} registers in 0x100000-0x101000",
            oracle_pfb.len()
        );
        eprintln!(
            "║ Extended: {} registers in FB/MMU ranges",
            oracle_ext.len()
        );

        // Show key PFB registers
        for &(offset, val) in oracle_pfb.iter().take(40) {
            if val != 0 {
                eprintln!("║   [{offset:#010x}] = {val:#010x}");
            }
        }
        if oracle_pfb.len() > 40 {
            eprintln!("║   ... {} more", oracle_pfb.len() - 40);
        }

        unsafe {
            let _ = rustix::mm::munmap(oracle_ptr, bar0_size);
        }

        // ── Phase 2: Open VFIO card and read its NV_PFB (cold state) ────
        let (_lease, raw) = open_vfio();

        // Run glowplug to get PFIFO alive
        let gp = GlowPlug::new(&raw.bar0, raw.container.clone());
        let warm_result = gp.full_init();
        eprintln!("╠══ VFIO CARD ({vfio_bdf}, vfio-pci) ════════════════════════╣");
        for msg in &warm_result.log {
            eprintln!("║ GP: {msg}");
        }

        // Read VFIO card's NV_PFB
        let mut vfio_pfb: Vec<(usize, u32)> = Vec::new();
        for offset in (pfb_start..pfb_end).step_by(4) {
            let val = raw.bar0.read_u32(offset).unwrap_or(0xDEAD_DEAD);
            vfio_pfb.push((offset, val));
        }

        // ── Phase 3: Diff oracle vs VFIO ────────────────────────────────
        eprintln!("╠══ NV_PFB DIFF (oracle vs cold VFIO) ═══════════════════════╣");
        let mut diff_regs: Vec<(usize, u32, u32)> = Vec::new(); // (offset, oracle_val, vfio_val)
        for &(offset, oracle_val) in &oracle_pfb {
            let vfio_val = vfio_pfb
                .iter()
                .find(|(o, _)| *o == offset)
                .map(|(_, v)| *v)
                .unwrap_or(0xDEAD_DEAD);
            if oracle_val != vfio_val {
                diff_regs.push((offset, oracle_val, vfio_val));
                eprintln!(
                    "║   [{offset:#010x}] oracle={oracle_val:#010x}  vfio={vfio_val:#010x}  Δ"
                );
            }
        }
        eprintln!("║ Total diffs: {}", diff_regs.len());

        if diff_regs.is_empty() {
            eprintln!("║ No PFB register differences! FB controller may be in same state.");
            eprintln!("║ VRAM dead for other reasons (check FBPA/LTC/MEM_STATUS).");
        }

        // ── Phase 4: Memory topology before any changes ─────────────────
        let topo_before = memory_probe::discover_memory_topology(&raw.bar0, raw.container.clone());
        eprintln!("╠══ MEMORY TOPOLOGY BEFORE FB INIT ══════════════════════════╣");
        topo_before.print_summary();

        // ── Phase 5: Apply oracle register values to VFIO card ──────────
        // Sort by address (apply in order) and skip dangerous-looking registers.
        let mut apply_regs: Vec<(usize, u32)> = diff_regs
            .iter()
            .filter(|(offset, _, _)| {
                // Skip MMU invalidation triggers and fault buffer registers
                // (those need careful sequencing, not blind copy)
                !matches!(
                    *offset,
                    0x0010_0CBC | 0x0010_0CB8 | 0x0010_0CEC |  // MMU invalidate
                    0x0010_0E24..=0x0010_0E54 // Fault buffers
                )
            })
            .map(|(offset, oracle_val, _)| (*offset, *oracle_val))
            .collect();
        apply_regs.sort_by_key(|(o, _)| *o);

        eprintln!(
            "╠══ APPLYING {} ORACLE REGISTERS TO VFIO CARD ═══════════════╣",
            apply_regs.len()
        );
        for &(offset, val) in &apply_regs {
            let _before = raw.bar0.read_u32(offset).unwrap_or(0xDEAD_DEAD);
            let _ = raw.bar0.write_u32(offset, val);
            std::thread::sleep(std::time::Duration::from_micros(100));
            let after = raw.bar0.read_u32(offset).unwrap_or(0xDEAD_DEAD);
            let stuck = after != val;
            if stuck {
                eprintln!(
                    "║   [{offset:#010x}] wrote {val:#010x}, read back {after:#010x} (STUCK)"
                );
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(50));

        // ── Phase 6: Re-probe memory topology after applying oracle regs ─
        let topo_after = memory_probe::discover_memory_topology(&raw.bar0, raw.container.clone());
        eprintln!("╠══ MEMORY TOPOLOGY AFTER FB INIT ═══════════════════════════╣");
        topo_after.print_summary();

        let delta = coral_driver::vfio::memory::MemoryDelta::compute(
            (pfb_start, 0),
            topo_before.clone(),
            topo_after.clone(),
        );

        if delta.unlocked_memory() {
            eprintln!(
                "╠══ VRAM UNLOCKED! {} paths gained ═════════════════════════╣",
                delta.paths_gained.len()
            );
            for path in &delta.paths_gained {
                eprintln!("║   {} → {} via {}", path.from, path.to, path.method);
            }
        } else {
            eprintln!("╠══ VRAM STILL DEAD ═════════════════════════════════════════╣");
            eprintln!("║ Oracle PFB registers alone did not unlock VRAM.");
            eprintln!("║ Next: try FBPA registers, LTC config, or full ramgv100 sequence.");
        }

        // ── Phase 7: Dump NV_PFB state for the fossil record ────────────
        eprintln!("╠══ FINAL NV_PFB STATE DUMP ═════════════════════════════════╣");
        memory_probe::dump_pfb_registers(&raw.bar0);

        // Run the full interpreter now with the (possibly warm) state
        let interpreter = ProbeInterpreter::new(&raw.bar0, raw.container.clone());
        let report = interpreter.run();
        report.print_summary();

        eprintln!(
            "║ Oracle test complete. VRAM accessible = {}",
            topo_after.vram_accessible
        );
    }

    /// Scan a VBIOS ROM file for init script register writes.
    /// Works without hardware — uses a pre-dumped VBIOS from hotSpring/data/.
    #[test]
    #[ignore = "requires VBIOS dump file"]
    fn vbios_script_scanner() {
        use coral_driver::vfio::channel::devinit;

        let vbios_path = match std::env::var("CORALREEF_VBIOS_PATH") {
            Ok(p) => p,
            Err(_) => {
                eprintln!(
                    "skipping vbios_script_scanner: set CORALREEF_VBIOS_PATH to a VBIOS dump (.bin); no default path (portable builds must not assume a developer home directory)"
                );
                return;
            }
        };

        let rom = devinit::read_vbios_file(&vbios_path).expect("Cannot read VBIOS file");
        eprintln!("VBIOS: {} bytes from {vbios_path}", rom.len());

        // Parse BIT table
        let bit = devinit::BitTable::parse(&rom).expect("BIT parse failed");
        eprintln!("BIT: {} entries", bit.entries.len());
        for entry in &bit.entries {
            let ch = if entry.id.is_ascii_graphic() {
                entry.id as char
            } else {
                '?'
            };
            eprintln!(
                "  '{}' (0x{:02x}) ver={} off={:#06x} sz={}",
                ch, entry.id, entry.version, entry.data_offset, entry.data_size
            );
        }

        // Extract register writes from boot scripts
        let writes = devinit::extract_boot_script_writes(&rom).expect("Script extraction failed");
        eprintln!("\nFound {} register writes in boot scripts", writes.len());

        // Categorize by domain
        let mut domains: std::collections::BTreeMap<&str, Vec<&devinit::ScriptRegWrite>> =
            std::collections::BTreeMap::new();
        for w in &writes {
            let domain = match w.reg {
                0x000000..=0x000FFF => "PMC",
                0x001000..=0x001FFF => "PBUS",
                0x009000..=0x009FFF => "PTIMER",
                0x00E000..=0x00EFFF => "PVPE",
                0x020000..=0x02FFFF => "PTOP/FUSE",
                0x088000..=0x088FFF => "CTXCTL",
                0x100000..=0x101FFF => "PFB",
                0x10A000..=0x10AFFF => "PMU",
                0x110000..=0x11FFFF => "PCOPY",
                0x122000..=0x122FFF => "PRI_MASTER",
                0x132000..=0x136FFF => "CLK",
                0x137000..=0x137FFF => "PCLOCK",
                0x17E000..=0x17FFFF => "LTC",
                0x1FA000..=0x1FAFFF => "PMEM",
                0x610000..=0x61FFFF => "PDISP",
                0x9A0000..=0x9BFFFF => "FBPA",
                _ => "OTHER",
            };
            domains.entry(domain).or_default().push(w);
        }

        eprintln!("\nRegister writes by domain:");
        for (domain, domain_writes) in &domains {
            eprintln!("  {domain:12}: {} writes", domain_writes.len());
            for w in domain_writes.iter().take(5) {
                let op_name = match w.opcode {
                    0x6E => "NV_REG",
                    0x7A => "ZM_REG",
                    0x58 => "ZM_SEQ",
                    0x77 => "ZM_R16",
                    _ => "???",
                };
                if let Some(mask) = w.mask {
                    eprintln!(
                        "    {op_name:6} [{:#010x}] &{mask:#010x} |{:#010x}",
                        w.reg, w.value
                    );
                } else {
                    eprintln!("    {op_name:6} [{:#010x}] = {:#010x}", w.reg, w.value);
                }
            }
            if domain_writes.len() > 5 {
                eprintln!("    ... {} more", domain_writes.len() - 5);
            }
        }

        // Key domains for HBM2 training
        let hbm2_domains = ["FBPA", "LTC", "PCLOCK", "CLK", "PFB", "PMU"];
        let hbm2_writes: usize = hbm2_domains
            .iter()
            .filter_map(|d| domains.get(d))
            .map(|w| w.len())
            .sum();
        eprintln!(
            "\nHBM2-critical writes (FBPA+LTC+PCLOCK+CLK+PFB+PMU): {}",
            hbm2_writes
        );

        assert!(
            !writes.is_empty(),
            "Expected some register writes in VBIOS scripts"
        );
    }

    /// Full sovereign GlowPlug warm-up test with all 5 strategies.
    /// Tests the complete warm-up pipeline on a cold/warm GPU.
    #[test]
    #[ignore = "requires VFIO-bound GPU hardware"]
    fn vfio_sovereign_glowplug_full() {
        use coral_driver::vfio::channel::devinit;
        use coral_driver::vfio::channel::glowplug::GlowPlug;

        let bdf = vfio_bdf();
        let oracle_bdf = std::env::var("CORALREEF_ORACLE_BDF").ok();
        let (_lease, raw) = open_vfio();

        eprintln!("╔══════════════════════════════════════════════════════════════╗");
        eprintln!("║ SOVEREIGN GLOWPLUG — FULL STRATEGY TEST                     ║");
        eprintln!("╠══════════════════════════════════════════════════════════════╣");

        // Pre-check
        let status = devinit::DevinitStatus::probe(&raw.bar0);
        status.print_summary();

        // Run GlowPlug with all strategies — load oracle from best available source
        let mut gp = GlowPlug::with_bdf(&raw.bar0, raw.container.clone(), &bdf);
        if let Some(ref oracle) = oracle_bdf {
            eprintln!("║ Oracle (live): {oracle}");
            gp.load_oracle_live(oracle)
                .expect("failed to load live oracle");
        } else if let Ok(text_path) = std::env::var("CORALREEF_ORACLE_TEXT") {
            eprintln!("║ Oracle (text dump): {text_path}");
            gp.load_oracle_text(std::path::Path::new(&text_path))
                .expect("failed to load oracle text dump");
        } else if let Ok(dump_path) = std::env::var("CORALREEF_ORACLE_DUMP") {
            eprintln!("║ Oracle (binary dump): {dump_path}");
            gp.load_oracle_dump(std::path::Path::new(&dump_path))
                .expect("failed to load oracle binary dump");
        } else {
            eprintln!("║ No oracle — running sovereign-only strategies");
        }

        let result = gp.full_init();
        eprintln!("╠══ GLOWPLUG LOG ════════════════════════════════════════════╣");
        for msg in &result.log {
            eprintln!("║ {msg}");
        }
        eprintln!("╠══════════════════════════════════════════════════════════════╣");
        eprintln!("║ Initial: {:?}", result.initial_state);
        eprintln!("║ Final:   {:?}", result.final_state);
        eprintln!("║ Success: {}", result.success);
        if let Some(ref mem) = result.memory {
            eprintln!("║ VRAM accessible: {}", mem.vram_accessible);
            eprintln!("║ BAR2 configured: {}", mem.bar2_configured);
        }
        eprintln!("╚══════════════════════════════════════════════════════════════╝");
    }

    /// Full BAR0 cartography: cold scan → GlowPlug warm → warm scan → diff.
    ///
    /// Follows the rustChip probe methodology: measure, then model.
    /// The diff reveals exactly what GlowPlug changes, and what remains
    /// unexplored. Persists comprehensive JSON for cross-card comparison.
    #[test]
    #[ignore = "requires VFIO-bound GPU hardware"]
    fn vfio_metal_cartography() {
        use coral_driver::vfio::bar_cartography;
        use coral_driver::vfio::channel::glowplug::GlowPlug;
        use coral_driver::vfio::gpu_vendor::GpuMetal;
        use coral_driver::vfio::nv_metal::NvVoltaMetal;
        use coral_driver::vfio::pci_discovery::PciDeviceInfo;

        let bdf = vfio_bdf();
        let (_lease, raw) = open_vfio();

        eprintln!("╔══════════════════════════════════════════════════════════════╗");
        eprintln!("║ METAL CARTOGRAPHY — COLD → WARM → DIFF                     ║");
        eprintln!("╠══════════════════════════════════════════════════════════════╣");

        // ── Phase 1: PCI identity ───────────────────────────────────────
        let pci_info = PciDeviceInfo::from_sysfs(&bdf).ok();
        if let Some(ref info) = pci_info {
            info.print_summary();
        }

        let boot0 = raw.bar0.read_u32(0).unwrap_or(0xDEAD_DEAD);
        let metal = NvVoltaMetal::from_boot0(boot0);
        eprintln!(
            "║ GPU: {} {} (BOOT0={boot0:#010x})",
            metal.identity().chip_name(),
            metal.identity().architecture(),
        );

        // ── Phase 2: Cold scan (before GlowPlug) ───────────────────────
        eprintln!("╠══ PHASE 2: COLD SCAN ══════════════════════════════════════╣");
        let domain_ranges: Vec<(&str, usize, usize)> = metal
            .domain_hints()
            .iter()
            .map(|h| (h.name, h.start, h.end))
            .collect();

        let cold_map = bar_cartography::scan_ranges(&raw.bar0, &domain_ranges);
        cold_map.print_summary();

        let cold_probe = metal.probe_live(&raw.bar0);
        cold_probe.print_summary();

        // ── Phase 3: GlowPlug warm-up ───────────────────────────────────
        eprintln!("╠══ PHASE 3: GLOWPLUG WARM-UP ══════════════════════════════╣");
        let gp = GlowPlug::with_bdf(&raw.bar0, raw.container.clone(), &bdf)
            .with_metal(Box::new(NvVoltaMetal::from_boot0(boot0)));
        let warm_result = gp.warm();
        for msg in &warm_result.log {
            eprintln!("║ {msg}");
        }
        eprintln!(
            "║ Warm: {:?} → {:?} (success={})",
            warm_result.initial_state, warm_result.final_state, warm_result.success
        );

        // Print step snapshot summaries
        for snap in &warm_result.step_snapshots {
            let deltas = snap.deltas();
            if !deltas.is_empty() {
                eprintln!(
                    "║ Step '{}': {} register(s) changed",
                    snap.step,
                    deltas.len()
                );
                for (off, before, after) in deltas.iter().take(5) {
                    eprintln!("║   [{off:#08x}] {before:#010x} → {after:#010x}");
                }
            }
        }

        // ── Phase 4: Warm scan (after GlowPlug) ─────────────────────────
        eprintln!("╠══ PHASE 4: WARM SCAN ══════════════════════════════════════╣");
        let warm_map = bar_cartography::scan_ranges(&raw.bar0, &domain_ranges);
        warm_map.print_summary();

        let warm_probe = metal.probe_live(&raw.bar0);
        warm_probe.print_summary();

        // ── Phase 5: Diff cold vs warm ───────────────────────────────────
        eprintln!("╠══ PHASE 5: COLD vs WARM DIFF ═════════════════════════════╣");
        let diff = bar_cartography::diff_bar_maps(&cold_map, &warm_map);
        diff.print_summary();

        // ── Phase 6: Persist comprehensive JSON ──────────────────────────
        let full_result = serde_json::json!({
            "bdf": bdf,
            "boot0": format!("{boot0:#010x}"),
            "chip": metal.identity().chip_name(),
            "architecture": metal.identity().architecture(),
            "timestamp": chrono_timestamp(),
            "cold_scan": {
                "bar_map": cold_map.to_json_value(),
                "probe": cold_probe.to_json_value(),
            },
            "warm_scan": {
                "bar_map": warm_map.to_json_value(),
                "probe": warm_probe.to_json_value(),
            },
            "diff": diff.to_json_value(),
            "glowplug": {
                "initial": format!("{:?}", warm_result.initial_state),
                "final": format!("{:?}", warm_result.final_state),
                "success": warm_result.success,
                "steps": warm_result.step_snapshots.iter().map(|s| {
                    let deltas = s.deltas();
                    serde_json::json!({
                        "step": s.step,
                        "changes": deltas.len(),
                        "deltas": deltas.iter().take(20).map(|(o, b, a)| serde_json::json!({
                            "offset": format!("{o:#x}"),
                            "before": format!("{b:#010x}"),
                            "after": format!("{a:#010x}"),
                        })).collect::<Vec<_>>(),
                    })
                }).collect::<Vec<_>>(),
            },
            "engines": metal.engine_list().iter().map(|e| serde_json::json!({
                "name": e.name,
                "kind": format!("{:?}", e.kind),
                "base": format!("{:#x}", e.base_offset),
                "has_firmware": e.has_firmware,
            })).collect::<Vec<_>>(),
            "memory_regions": metal.memory_regions().iter().map(|m| serde_json::json!({
                "name": m.name,
                "kind": format!("{:?}", m.kind),
                "control_base": m.control_base.map(|b| format!("{b:#x}")),
                "size": m.size,
                "partitions": m.partitions,
            })).collect::<Vec<_>>(),
        });

        let json_str = serde_json::to_string_pretty(&full_result).unwrap();
        let output_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("hotSpring/data/metal_maps");
        let _ = std::fs::create_dir_all(&output_dir);
        let output_path = output_dir.join("titan_v_gv100_metal_map.json");
        let _ = std::fs::write(&output_path, &json_str);
        eprintln!("╠══════════════════════════════════════════════════════════════╣");
        eprintln!("║ Results: {}", output_path.display());
        eprintln!("╚══════════════════════════════════════════════════════════════╝");
    }

    fn chrono_timestamp() -> String {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        format!("{}", now.as_secs())
    }

    /// PCI Discovery test — vendor-agnostic PCI config space parsing.
    #[test]
    #[ignore = "requires VFIO-bound GPU hardware"]
    fn vfio_pci_discovery() {
        use coral_driver::vfio::pci_discovery::PciDeviceInfo;

        let bdf = vfio_bdf();

        eprintln!("╔══════════════════════════════════════════════════════════════╗");
        eprintln!("║ PCI DISCOVERY — VENDOR-AGNOSTIC                            ║");
        eprintln!("╠══════════════════════════════════════════════════════════════╣");

        let info = PciDeviceInfo::from_sysfs(&bdf).expect("PCI config parse");
        info.print_summary();

        assert_eq!(info.vendor_id, 0x10DE, "Expected NVIDIA vendor ID");
        assert!(!info.bars.is_empty(), "Expected at least one BAR");
        // vfio-pci may truncate config space to 64 bytes, so capabilities
        // may come from sysfs fallback rather than the capability chain
        if info.capabilities.is_empty() {
            eprintln!("║ NOTE: No PCI capabilities found (config space truncated by vfio-pci)");
        }

        eprintln!("╚══════════════════════════════════════════════════════════════╝");
    }

    /// GlowPlug with GpuMetal trait — vendor-agnostic warm-up test.
    #[test]
    #[ignore = "requires VFIO-bound GPU hardware"]
    fn vfio_metal_glowplug() {
        use coral_driver::vfio::channel::glowplug::GlowPlug;
        use coral_driver::vfio::nv_metal::NvVoltaMetal;

        let bdf = vfio_bdf();
        let (_lease, raw) = open_vfio();

        let boot0 = raw.bar0.read_u32(0).unwrap_or(0xDEAD_DEAD);
        let metal = NvVoltaMetal::from_boot0(boot0);

        eprintln!("╔══════════════════════════════════════════════════════════════╗");
        eprintln!("║ METAL GLOWPLUG — TRAIT-BASED WARM-UP                       ║");
        eprintln!("╠══════════════════════════════════════════════════════════════╣");

        let gp =
            GlowPlug::with_bdf(&raw.bar0, raw.container.clone(), &bdf).with_metal(Box::new(metal));

        let result = gp.warm();
        for msg in &result.log {
            eprintln!("║ {msg}");
        }
        eprintln!("║ Initial: {:?}", result.initial_state);
        eprintln!("║ Final:   {:?}", result.final_state);
        eprintln!("║ Success: {}", result.success);
        eprintln!("╚══════════════════════════════════════════════════════════════╝");
    }

    /// Power bounds probing — empirical power transition mapping.
    #[test]
    #[ignore = "requires VFIO-bound GPU hardware"]
    fn vfio_power_bounds() {
        use coral_driver::vfio::channel::glowplug::GlowPlug;

        let bdf = vfio_bdf();
        let (_lease, raw) = open_vfio();

        eprintln!("╔══════════════════════════════════════════════════════════════╗");
        eprintln!("║ POWER BOUNDS — EMPIRICAL TRANSITION MAPPING                ║");
        eprintln!("╠══════════════════════════════════════════════════════════════╣");

        let gp = GlowPlug::with_bdf(&raw.bar0, raw.container.clone(), &bdf);

        // Ensure GPU is warm first
        let warm = gp.warm();
        eprintln!(
            "║ Pre-warm: {:?} → {:?}",
            warm.initial_state, warm.final_state
        );

        let bounds = gp.probe_bounds();
        eprintln!("╠══ D3HOT RESULTS ═══════════════════════════════════════════╣");
        for s in &bounds.d3hot_survives {
            eprintln!("║  ✓ {s}");
        }
        for s in &bounds.d3hot_lost {
            eprintln!("║  ✗ {s}");
        }
        eprintln!("╠══ CLOCK GATE RESULTS ══════════════════════════════════════╣");
        for s in &bounds.clock_gate_survives {
            eprintln!("║  ✓ {s}");
        }
        for s in &bounds.clock_gate_lost {
            eprintln!("║  ✗ {s}");
        }
        eprintln!("╚══════════════════════════════════════════════════════════════╝");
    }
}
