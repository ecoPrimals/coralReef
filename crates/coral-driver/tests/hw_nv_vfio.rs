// SPDX-License-Identifier: AGPL-3.0-only
//! NVIDIA VFIO hardware validation — direct BAR0/DMA dispatch.
//!
//! These tests exercise the VFIO compute pipeline:
//! open → alloc → upload → dispatch → sync → readback.
//!
//! # Prerequisites
//!
//! - GPU bound to `vfio-pci` (not nouveau/nvidia)
//! - IOMMU enabled in BIOS and kernel
//! - User has `/dev/vfio/*` permissions
//! - Set `CORALREEF_VFIO_BDF` env var to the GPU's PCIe address
//!
//! Run: `CORALREEF_VFIO_BDF=0000:01:00.0 cargo test --test hw_nv_vfio --features vfio -- --ignored`

#[cfg(feature = "vfio")]
mod tests {
    use coral_driver::nv::NvVfioComputeDevice;
    use coral_driver::{ComputeDevice, DispatchDims, MemoryDomain, ShaderInfo};

    fn vfio_bdf() -> String {
        std::env::var("CORALREEF_VFIO_BDF")
            .expect("set CORALREEF_VFIO_BDF=0000:XX:XX.X to run VFIO tests")
    }

    fn vfio_sm() -> u32 {
        std::env::var("CORALREEF_VFIO_SM")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(86)
    }

    fn sm_to_compute_class(sm: u32) -> u32 {
        match sm {
            70..=74 => coral_driver::nv::pushbuf::class::VOLTA_COMPUTE_A,
            75..=79 => coral_driver::nv::pushbuf::class::TURING_COMPUTE_A,
            _ => coral_driver::nv::pushbuf::class::AMPERE_COMPUTE_A,
        }
    }

    fn open_vfio() -> NvVfioComputeDevice {
        let bdf = vfio_bdf();
        let sm = vfio_sm();
        let cc = sm_to_compute_class(sm);
        NvVfioComputeDevice::open(&bdf, sm, cc)
            .expect("NvVfioComputeDevice::open() — is GPU bound to vfio-pci?")
    }

    #[test]
    #[ignore = "requires VFIO-bound GPU hardware"]
    fn vfio_open_and_bar0_read() {
        let _dev = open_vfio();
    }

    #[test]
    #[ignore = "requires VFIO-bound GPU hardware"]
    fn vfio_alloc_and_free() {
        let mut dev = open_vfio();
        let handle = dev.alloc(4096, MemoryDomain::Gtt).expect("alloc");
        dev.free(handle).expect("free");
    }

    #[test]
    #[ignore = "requires VFIO-bound GPU hardware"]
    fn vfio_upload_and_readback() {
        let mut dev = open_vfio();
        let handle = dev.alloc(256, MemoryDomain::Gtt).expect("alloc");
        let data: Vec<u8> = (0..256).map(|i| (i & 0xFF) as u8).collect();
        dev.upload(handle, 0, &data).expect("upload");
        let result = dev.readback(handle, 0, 256).expect("readback");
        assert_eq!(result, data);
        dev.free(handle).expect("free");
    }

    #[test]
    #[ignore = "requires VFIO-bound GPU hardware"]
    fn vfio_multiple_buffers() {
        let mut dev = open_vfio();
        let handles: Vec<_> = (0..4)
            .map(|_| dev.alloc(4096, MemoryDomain::Gtt).expect("alloc"))
            .collect();
        for h in handles {
            dev.free(h).expect("free");
        }
    }

    #[test]
    #[ignore = "requires VFIO-bound GPU hardware + compute shader binary"]
    fn vfio_dispatch_nop_shader() {
        let mut dev = open_vfio();
        let sm = vfio_sm();

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
        let compiled = coral_reef::compile_wgsl_full(wgsl, &opts).expect("compile");
        let info = ShaderInfo {
            gpr_count: compiled.info.gpr_count,
            shared_mem_bytes: compiled.info.shared_mem_bytes,
            barrier_count: compiled.info.barrier_count,
            workgroup: compiled.info.local_size,
        };

        dev.dispatch(&compiled.binary, &[], DispatchDims::linear(1), &info)
            .expect("dispatch");
        dev.sync().expect("sync");
    }

    #[test]
    #[ignore = "requires VFIO-bound GPU hardware"]
    fn vfio_pfifo_diagnostic_matrix() {
        use coral_driver::nv::RawVfioDevice;
        use coral_driver::vfio::channel::{build_experiment_matrix, diagnostic_matrix};

        let bdf = vfio_bdf();
        let mut raw =
            RawVfioDevice::open(&bdf).expect("RawVfioDevice::open() — is GPU bound to vfio-pci?");

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
        use coral_driver::nv::RawVfioDevice;
        use coral_driver::vfio::channel::ProbeInterpreter;

        let bdf = vfio_bdf();
        let raw =
            RawVfioDevice::open(&bdf).expect("RawVfioDevice::open() — is GPU bound to vfio-pci?");

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
    fn vfio_devinit_pmu_probe() {
        use coral_driver::nv::RawVfioDevice;
        use coral_driver::vfio::channel::devinit;
        use coral_driver::vfio::channel::glowplug::GlowPlug;

        let bdf = vfio_bdf();
        let oracle_bdf = std::env::var("CORALREEF_ORACLE_BDF").ok();
        let raw = RawVfioDevice::open(&bdf)
            .expect("RawVfioDevice::open() — is GPU bound to vfio-pci?");

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
                                eprintln!("╠══ PMU FIRMWARE TABLE ══════════════════════════════════════╣");
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
                                    eprintln!("╠══ EXECUTING PMU DEVINIT ═══════════════════════════════════╣");
                                    match devinit::execute_devinit(&raw.bar0, &rom) {
                                        Ok(true) => {
                                            eprintln!("║ DEVINIT COMPLETED! Checking VRAM...");
                                            std::thread::sleep(std::time::Duration::from_millis(100));

                                            let gp = GlowPlug::with_bdf(&raw.bar0, raw.container_fd, &bdf);
                                            let vram_ok = gp.check_vram();
                                            eprintln!("║ VRAM accessible: {vram_ok}");

                                            if vram_ok {
                                                eprintln!("║ *** SUCCESS: HBM2 training via PMU DEVINIT worked! ***");
                                            } else {
                                                eprintln!("║ VRAM still dead. Devinit ran but HBM2 not trained.");
                                                eprintln!("║ Possible: devinit script didn't include memory training,");
                                                eprintln!("║ or the FALCON wasn't properly authenticated.");
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
            GlowPlug::with_oracle(&raw.bar0, raw.container_fd, &bdf, oracle)
        } else {
            GlowPlug::with_bdf(&raw.bar0, raw.container_fd, &bdf)
        };
        let result = gp.full_init();
        for msg in &result.log {
            eprintln!("║ {msg}");
        }
        eprintln!("║ Final state: {:?}  success={}", result.final_state, result.success);
        eprintln!("╚══════════════════════════════════════════════════════════════╝");
    }

    #[test]
    #[ignore = "requires VFIO-bound GPU hardware + nouveau-bound oracle card"]
    fn vfio_cross_card_fb_init_oracle() {
        use coral_driver::nv::RawVfioDevice;
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

        use std::os::fd::AsRawFd;
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
        eprintln!("║ NV_PFB: {} registers in 0x100000-0x101000", oracle_pfb.len());
        eprintln!("║ Extended: {} registers in FB/MMU ranges", oracle_ext.len());

        // Show key PFB registers
        for &(offset, val) in oracle_pfb.iter().take(40) {
            if val != 0 {
                eprintln!("║   [{offset:#010x}] = {val:#010x}");
            }
        }
        if oracle_pfb.len() > 40 {
            eprintln!("║   ... {} more", oracle_pfb.len() - 40);
        }

        unsafe { let _ = rustix::mm::munmap(oracle_ptr, bar0_size); }

        // ── Phase 2: Open VFIO card and read its NV_PFB (cold state) ────
        let raw = RawVfioDevice::open(&vfio_bdf)
            .expect("RawVfioDevice::open() — is GPU bound to vfio-pci?");

        // Run glowplug to get PFIFO alive
        let gp = GlowPlug::new(&raw.bar0, raw.container_fd);
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
            let vfio_val = vfio_pfb.iter()
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
        let topo_before = memory_probe::discover_memory_topology(&raw.bar0, raw.container_fd);
        eprintln!("╠══ MEMORY TOPOLOGY BEFORE FB INIT ══════════════════════════╣");
        topo_before.print_summary();

        // ── Phase 5: Apply oracle register values to VFIO card ──────────
        // Sort by address (apply in order) and skip dangerous-looking registers.
        let mut apply_regs: Vec<(usize, u32)> = diff_regs.iter()
            .filter(|(offset, _, _)| {
                // Skip MMU invalidation triggers and fault buffer registers
                // (those need careful sequencing, not blind copy)
                !matches!(*offset,
                    0x0010_0CBC | 0x0010_0CB8 | 0x0010_0CEC |  // MMU invalidate
                    0x0010_0E24..=0x0010_0E54                    // Fault buffers
                )
            })
            .map(|(offset, oracle_val, _)| (*offset, *oracle_val))
            .collect();
        apply_regs.sort_by_key(|(o, _)| *o);

        eprintln!("╠══ APPLYING {} ORACLE REGISTERS TO VFIO CARD ═══════════════╣", apply_regs.len());
        for &(offset, val) in &apply_regs {
            let before = raw.bar0.read_u32(offset).unwrap_or(0xDEAD_DEAD);
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
        let topo_after = memory_probe::discover_memory_topology(&raw.bar0, raw.container_fd);
        eprintln!("╠══ MEMORY TOPOLOGY AFTER FB INIT ═══════════════════════════╣");
        topo_after.print_summary();

        let delta = coral_driver::vfio::memory::MemoryDelta::compute(
            (pfb_start, 0),
            topo_before.clone(),
            topo_after.clone(),
        );

        if delta.unlocked_memory() {
            eprintln!("╠══ VRAM UNLOCKED! {} paths gained ═════════════════════════╣", delta.paths_gained.len());
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
        let interpreter = ProbeInterpreter::new(&raw.bar0, raw.container_fd);
        let report = interpreter.run();
        report.print_summary();

        eprintln!("║ Oracle test complete. VRAM accessible = {}", topo_after.vram_accessible);
    }

    #[cfg(feature = "test-utils")]
    #[test]
    #[ignore = "requires VFIO-bound GPU hardware"]
    fn vfio_free_invalid_handle() {
        let mut dev = open_vfio();
        let result = dev.free(coral_driver::BufferHandle::from_id(9999));
        assert!(result.is_err());
    }

    #[cfg(feature = "test-utils")]
    #[test]
    #[ignore = "requires VFIO-bound GPU hardware"]
    fn vfio_readback_invalid_handle() {
        let dev = open_vfio();
        let result = dev.readback(coral_driver::BufferHandle::from_id(9999), 0, 16);
        assert!(result.is_err());
    }

    /// Scan a VBIOS ROM file for init script register writes.
    /// Works without hardware — uses a pre-dumped VBIOS from hotSpring/data/.
    #[test]
    #[ignore = "requires VBIOS dump file"]
    fn vbios_script_scanner() {
        use coral_driver::vfio::channel::devinit;

        let vbios_path = std::env::var("CORALREEF_VBIOS_PATH")
            .unwrap_or_else(|_| {
                let home = std::env::var("HOME").unwrap_or_default();
                format!("{home}/Development/ecoPrimals/hotSpring/data/vbios_0000_03_00_0.bin")
            });

        let rom = devinit::read_vbios_file(&vbios_path)
            .expect("Cannot read VBIOS file");
        eprintln!("VBIOS: {} bytes from {vbios_path}", rom.len());

        // Parse BIT table
        let bit = devinit::BitTable::parse(&rom).expect("BIT parse failed");
        eprintln!("BIT: {} entries", bit.entries.len());
        for entry in &bit.entries {
            let ch = if entry.id.is_ascii_graphic() { entry.id as char } else { '?' };
            eprintln!("  '{}' (0x{:02x}) ver={} off={:#06x} sz={}",
                ch, entry.id, entry.version, entry.data_offset, entry.data_size);
        }

        // Extract register writes from boot scripts
        let writes = devinit::extract_boot_script_writes(&rom)
            .expect("Script extraction failed");
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
                    eprintln!("    {op_name:6} [{:#010x}] &{mask:#010x} |{:#010x}", w.reg, w.value);
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
        let hbm2_writes: usize = hbm2_domains.iter()
            .filter_map(|d| domains.get(d))
            .map(|w| w.len())
            .sum();
        eprintln!(
            "\nHBM2-critical writes (FBPA+LTC+PCLOCK+CLK+PFB+PMU): {}",
            hbm2_writes
        );

        assert!(!writes.is_empty(), "Expected some register writes in VBIOS scripts");
    }

    /// Full sovereign GlowPlug warm-up test with all 5 strategies.
    /// Tests the complete warm-up pipeline on a cold/warm GPU.
    #[test]
    #[ignore = "requires VFIO-bound GPU hardware"]
    fn vfio_sovereign_glowplug_full() {
        use coral_driver::nv::RawVfioDevice;
        use coral_driver::vfio::channel::devinit;
        use coral_driver::vfio::channel::glowplug::GlowPlug;

        let bdf = vfio_bdf();
        let oracle_bdf = std::env::var("CORALREEF_ORACLE_BDF").ok();
        let raw = RawVfioDevice::open(&bdf)
            .expect("RawVfioDevice::open() — is GPU bound to vfio-pci?");

        eprintln!("╔══════════════════════════════════════════════════════════════╗");
        eprintln!("║ SOVEREIGN GLOWPLUG — FULL STRATEGY TEST                     ║");
        eprintln!("╠══════════════════════════════════════════════════════════════╣");

        // Pre-check
        let status = devinit::DevinitStatus::probe(&raw.bar0);
        status.print_summary();

        // Run GlowPlug with all strategies — load oracle from best available source
        let mut gp = GlowPlug::with_bdf(&raw.bar0, raw.container_fd, &bdf);
        if let Some(ref oracle) = oracle_bdf {
            eprintln!("║ Oracle (live): {oracle}");
            gp.load_oracle_live(oracle).expect("failed to load live oracle");
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
        use coral_driver::nv::RawVfioDevice;
        use coral_driver::vfio::bar_cartography;
        use coral_driver::vfio::channel::glowplug::GlowPlug;
        use coral_driver::vfio::gpu_vendor::GpuMetal;
        use coral_driver::vfio::nv_metal::NvVoltaMetal;
        use coral_driver::vfio::pci_discovery::PciDeviceInfo;

        let bdf = vfio_bdf();
        let raw = RawVfioDevice::open(&bdf)
            .expect("RawVfioDevice::open() — is GPU bound to vfio-pci?");

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
        let gp = GlowPlug::with_bdf(&raw.bar0, raw.container_fd, &bdf)
            .with_metal(Box::new(NvVoltaMetal::from_boot0(boot0)));
        let warm_result = gp.warm();
        for msg in &warm_result.log {
            eprintln!("║ {msg}");
        }
        eprintln!("║ Warm: {:?} → {:?} (success={})",
            warm_result.initial_state, warm_result.final_state, warm_result.success);

        // Print step snapshot summaries
        for snap in &warm_result.step_snapshots {
            let deltas = snap.deltas();
            if !deltas.is_empty() {
                eprintln!("║ Step '{}': {} register(s) changed", snap.step, deltas.len());
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
            .parent().unwrap()
            .parent().unwrap()
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

        let info = PciDeviceInfo::from_sysfs(&bdf)
            .expect("PCI config parse");

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
        use coral_driver::nv::RawVfioDevice;
        use coral_driver::vfio::channel::glowplug::GlowPlug;
        use coral_driver::vfio::nv_metal::NvVoltaMetal;

        let bdf = vfio_bdf();
        let raw = RawVfioDevice::open(&bdf)
            .expect("RawVfioDevice::open()");

        let boot0 = raw.bar0.read_u32(0).unwrap_or(0xDEAD_DEAD);
        let metal = NvVoltaMetal::from_boot0(boot0);

        eprintln!("╔══════════════════════════════════════════════════════════════╗");
        eprintln!("║ METAL GLOWPLUG — TRAIT-BASED WARM-UP                       ║");
        eprintln!("╠══════════════════════════════════════════════════════════════╣");

        let gp = GlowPlug::with_bdf(&raw.bar0, raw.container_fd, &bdf)
            .with_metal(Box::new(metal));

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
        use coral_driver::nv::RawVfioDevice;
        use coral_driver::vfio::channel::glowplug::GlowPlug;

        let bdf = vfio_bdf();
        let raw = RawVfioDevice::open(&bdf)
            .expect("RawVfioDevice::open()");

        eprintln!("╔══════════════════════════════════════════════════════════════╗");
        eprintln!("║ POWER BOUNDS — EMPIRICAL TRANSITION MAPPING                ║");
        eprintln!("╠══════════════════════════════════════════════════════════════╣");

        let gp = GlowPlug::with_bdf(&raw.bar0, raw.container_fd, &bdf);

        // Ensure GPU is warm first
        let warm = gp.warm();
        eprintln!("║ Pre-warm: {:?} → {:?}", warm.initial_state, warm.final_state);

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

    // ── HBM2 Training Experiments ────────────────────────────────────────

    #[test]
    #[ignore = "requires VFIO-bound GPU hardware"]
    fn vfio_hbm2_phy_probe() {
        use coral_driver::nv::RawVfioDevice;
        use coral_driver::vfio::channel::hbm2_training::{
            snapshot_fbpa, volta_hbm2,
        };

        let bdf = vfio_bdf();
        let raw = RawVfioDevice::open(&bdf)
            .expect("RawVfioDevice::open()");

        eprintln!("╔══════════════════════════════════════════════════════════════╗");
        eprintln!("║ HBM2 PHY PROBE — FBPA Partition Status                    ║");
        eprintln!("╠══════════════════════════════════════════════════════════════╣");

        let snaps = snapshot_fbpa(&raw.bar0, volta_hbm2::FBPA_COUNT);
        let alive_count = snaps.iter().filter(|s| s.alive).count();
        let configured_count = snaps.iter().filter(|s| s.cfg != 0 && s.alive).count();

        for snap in &snaps {
            eprintln!(
                "║ FBPA{}: base={:#010x} cfg={:#010x} t0={:#010x} t1={:#010x} t2={:#010x} {}",
                snap.index, snap.base, snap.cfg,
                snap.timing0, snap.timing1, snap.timing2,
                if snap.alive { "ALIVE" } else { "DEAD" },
            );
        }

        eprintln!("║");
        eprintln!("║ Summary: {alive_count}/{} alive, {configured_count}/{} configured",
            volta_hbm2::FBPA_COUNT, volta_hbm2::FBPA_COUNT);

        // Also probe LTC partitions
        eprintln!("╠══ LTC Partitions ══════════════════════════════════════════╣");
        for i in 0..volta_hbm2::LTC_COUNT {
            let base = volta_hbm2::LTC_BASE + i * volta_hbm2::LTC_STRIDE;
            let val = raw.bar0.read_u32(base).unwrap_or(0xDEAD_DEAD);
            let is_err = val == 0xFFFF_FFFF || val == 0xDEAD_DEAD || (val >> 16) == 0xBADF;
            eprintln!("║ LTC{i}: base={base:#010x} val={val:#010x} {}", if is_err { "DEAD" } else { "alive" });
        }

        // Probe PFB status registers
        eprintln!("╠══ PFB Status ══════════════════════════════════════════════╣");
        let pfb_regs: &[(&str, usize)] = &[
            ("CFG0", 0x100000), ("CFG1", 0x100004),
            ("PART_CTRL", 0x100200), ("ZBC_CTRL", 0x100300),
            ("MEM_STATUS", 0x100800), ("MEM_CTRL", 0x100804),
            ("MEM_ACK", 0x100808), ("MMU_CTRL", 0x100C80),
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
        use coral_driver::nv::RawVfioDevice;
        use coral_driver::vfio::channel::hbm2_training::{
            self as hbm2, snapshot_fbpa, volta_hbm2, HBM2_CAPTURE_DOMAINS,
        };

        let bdf = vfio_bdf();
        let raw = RawVfioDevice::open(&bdf)
            .expect("RawVfioDevice::open()");

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
            eprintln!("║ {name:12}: {} registers captured ({start:#010x}..{end:#010x})", registers.len());
            total_regs += registers.len();
            domain_data.push(hbm2::DomainCapture {
                name: name.into(),
                registers,
            });
        }

        eprintln!("║");
        eprintln!("║ Total: {total_regs} registers captured across {} domains", domain_data.len());

        // Save capture as JSON
        let capture = hbm2::GoldenCapture {
            boot0: raw.bar0.read_u32(0).unwrap_or(0),
            pmc_enable: raw.bar0.read_u32(0x200).unwrap_or(0),
            domains: domain_data,
            timestamp: format!("{}s", std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs()),
        };

        let json = serde_json::to_string_pretty(&capture).unwrap_or_default();
        let out_dir = "/home/biomegate/Development/ecoPrimals/hotSpring/data/metal_maps";
        let out_path = format!("{out_dir}/titan_v_hbm2_timing_capture.json");
        if let Err(e) = std::fs::create_dir_all(out_dir) {
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
        use coral_driver::nv::RawVfioDevice;
        use coral_driver::vfio::channel::hbm2_training::{
            self as hbm2, Hbm2Controller, Untrained, volta_hbm2,
            TrainingAction,
        };

        let bdf = vfio_bdf();
        let raw = RawVfioDevice::open(&bdf)
            .expect("RawVfioDevice::open()");

        eprintln!("╔══════════════════════════════════════════════════════════════╗");
        eprintln!("║ HBM2 TRAINING ATTEMPT — Typestate Sequence                 ║");
        eprintln!("╠══════════════════════════════════════════════════════════════╣");

        let ctrl = Hbm2Controller::<Untrained>::new(
            &raw.bar0,
            Some(&bdf),
            volta_hbm2::FBPA_COUNT,
        );

        let result = ctrl.enable_phy()
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
                let verifications: Vec<_> = tlog.actions.iter().filter_map(|a| {
                    if let TrainingAction::Verification { offset, expected, actual, ok } = a {
                        Some((offset, expected, actual, ok))
                    } else {
                        None
                    }
                }).collect();
                if !verifications.is_empty() {
                    eprintln!("║");
                    eprintln!("║ Verifications:");
                    for (off, exp, actual, ok) in &verifications {
                        eprintln!("║   [{off:#010x}] exp={exp:#010x} actual={actual:#010x} {}",
                            if **ok { "OK" } else { "FAIL" });
                    }
                }

                // Save FBPA state
                let fbpa_state = verified.fbpa_state();
                for snap in &fbpa_state {
                    eprintln!("║ FBPA{}: cfg={:#010x} {}",
                        snap.index, snap.cfg, if snap.alive { "alive" } else { "DEAD" });
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
        use coral_driver::nv::RawVfioDevice;
        use coral_driver::vfio::channel::devinit::FalconDiagnostic;

        let bdf = vfio_bdf();
        let raw = RawVfioDevice::open(&bdf)
            .expect("RawVfioDevice::open()");

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
                            eprintln!("║   BIT '{}'  ver={} offset={:#06x} size={}",
                                entry.id as char, entry.version, entry.data_offset, entry.data_size);
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
                            if (0x9A0000..0x9B0000).contains(&r) { fbpa_count += 1; }
                            else if (0x17E000..0x190000).contains(&r) { ltc_count += 1; }
                            else if (0x100000..0x102000).contains(&r) { pfb_count += 1; }
                            else if (0x132000..0x138000).contains(&r) { clk_count += 1; }
                            else { other_count += 1; }
                        }

                        eprintln!("║   FBPA: {fbpa_count}, LTC: {ltc_count}, PFB: {pfb_count}, CLK: {clk_count}, other: {other_count}");
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
    fn vfio_pri_backpressure_probe() {
        use coral_driver::nv::RawVfioDevice;
        use coral_driver::vfio::channel::pri_monitor::{PriBusMonitor, DomainHealth};

        let bdf = vfio_bdf();
        let raw = RawVfioDevice::open(&bdf)
            .expect("RawVfioDevice::open()");

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
        let alive = health.iter().filter(|(_, _, h)| matches!(h, DomainHealth::Alive)).count();
        let faulted = health.iter().filter(|(_, _, h)| matches!(h, DomainHealth::Faulted { .. })).count();
        eprintln!("║");
        eprintln!("║ Summary: {alive} alive, {faulted} faulted");

        // Phase 2: If faulted, try recovery
        if faulted > 0 {
            eprintln!("╠══ PRI Recovery Attempt ════════════════════════════════════╣");
            let recovered = monitor.attempt_recovery();
            eprintln!("║ Recovery: {}", if recovered { "SUCCESS (bus clean)" } else { "FAILED (bus locked)" });

            // Re-probe after recovery
            let post_health = monitor.probe_all_domains();
            let post_alive = post_health.iter().filter(|(_, _, h)| matches!(h, DomainHealth::Alive)).count();
            let post_faulted = post_health.iter().filter(|(_, _, h)| matches!(h, DomainHealth::Faulted { .. })).count();
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
        eprintln!("║ Reads: {} total, {} faulted", stats.reads_total, stats.reads_faulted);
        eprintln!("║ Writes: {} total, {} applied, {} skipped", stats.writes_total, stats.writes_applied, stats.writes_skipped_faulted);
        eprintln!("║ Recoveries: {}", stats.bus_recoveries);
        if !stats.domains_faulted.is_empty() {
            eprintln!("║ Faulted domains: {:?}", stats.domains_faulted);
        }
        eprintln!("╚══════════════════════════════════════════════════════════════╝");
    }

    #[test]
    #[ignore = "requires VFIO-bound GPU hardware"]
    fn vfio_pclock_deep_probe() {
        use coral_driver::nv::RawVfioDevice;
        use coral_driver::vfio::channel::registers::pri;

        let bdf = vfio_bdf();
        let raw = RawVfioDevice::open(&bdf)
            .expect("RawVfioDevice::open()");

        eprintln!("╔══════════════════════════════════════════════════════════════╗");
        eprintln!("║ PCLOCK DEEP PROBE — Scanning clock domain for live regs     ║");
        eprintln!("╠══════════════════════════════════════════════════════════════╣");

        // Phase 1: Enable PMC and wait
        let r = |off: usize| -> u32 { raw.bar0.read_u32(off).unwrap_or(0xDEAD_DEAD) };
        let w = |off: usize, val: u32| { let _ = raw.bar0.write_u32(off, val); };

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
        let mut faulted_patterns: std::collections::HashMap<u32, usize> = std::collections::HashMap::new();

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
            eprintln!("║   {pattern:#010x}: {count} registers — {}", pri::decode_pri_error(**pattern));
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

            let changed = if before_err { "was faulted" } else if before == after { "unchanged" } else { "CHANGED" };
            let pclock_alive = if pri::is_pri_error(pclock_status) { "dead" } else { "ALIVE" };
            eprintln!("║   {desc}: {before:#010x} → {after:#010x} ({changed}) | PCLOCK[0]={pclock_status:#010x} ({pclock_alive})");
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
        use coral_driver::nv::RawVfioDevice;
        use coral_driver::vfio::channel::oracle::{OracleState, DigitalPmu};
        use coral_driver::vfio::channel::registers::pri;

        let bdf = vfio_bdf();
        let raw = RawVfioDevice::open(&bdf)
            .expect("RawVfioDevice::open()");

        eprintln!("╔══════════════════════════════════════════════════════════════╗");
        eprintln!("║ Oracle Root PLL Programming                                 ║");
        eprintln!("╠══════════════════════════════════════════════════════════════╣");

        // Load oracle data from best available source
        let oracle = if let Ok(oracle_bdf) = std::env::var("CORALREEF_ORACLE_BDF") {
            eprintln!("║ Loading oracle from live card: {oracle_bdf}");
            OracleState::from_live_card(&oracle_bdf)
                .expect("failed to read oracle BAR0")
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

        eprintln!("║ Oracle: {} total registers from {}", oracle.registers.len(), oracle.source);
        eprintln!("║ Root PLLs (0x136xxx): {} registers", oracle.root_pll_registers().len());
        eprintln!("║ PCLOCK (0x137xxx): {} registers", oracle.pclock_registers().len());

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
        eprintln!("║ Root PLL comparison: {cold_match} match, {cold_diff} differ, {cold_dead} dead");

        // Phase 2: Check PCLOCK before programming
        let pclock_before = r(0x137000);
        eprintln!("║ PCLOCK[0] before: {pclock_before:#010x} ({})",
            if pri::is_pri_error(pclock_before) { "FAULTED" } else { "ALIVE" }
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
        eprintln!("║ PCLOCK[0] after root PLLs: {pclock_after:#010x} ({})",
            if pri::is_pri_error(pclock_after) { "FAULTED" } else { "ALIVE" }
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
        use coral_driver::nv::RawVfioDevice;
        use coral_driver::vfio::channel::oracle::{OracleState, DigitalPmu};
        use coral_driver::vfio::channel::glowplug::GlowPlug;

        let bdf = vfio_bdf();
        let raw = RawVfioDevice::open(&bdf)
            .expect("RawVfioDevice::open()");

        eprintln!("╔══════════════════════════════════════════════════════════════╗");
        eprintln!("║ Digital PMU Full Emulation                                  ║");
        eprintln!("╠══════════════════════════════════════════════════════════════╣");

        // Load oracle
        let oracle = if let Ok(oracle_bdf) = std::env::var("CORALREEF_ORACLE_BDF") {
            OracleState::from_live_card(&oracle_bdf)
                .expect("failed to read oracle BAR0")
        } else if let Ok(dump_path) = std::env::var("CORALREEF_ORACLE_DUMP") {
            OracleState::from_bar0_dump(std::path::Path::new(&dump_path))
                .expect("failed to load BAR0 dump")
        } else if let Ok(text_path) = std::env::var("CORALREEF_ORACLE_TEXT") {
            OracleState::from_text_dump(std::path::Path::new(&text_path))
                .expect("failed to load text dump")
        } else {
            panic!("Set CORALREEF_ORACLE_BDF, CORALREEF_ORACLE_DUMP, or CORALREEF_ORACLE_TEXT");
        };

        eprintln!("║ Oracle: {} registers from {}", oracle.registers.len(), oracle.source);

        // Check pre-state
        let plug = GlowPlug::with_bdf(&raw.bar0, raw.container_fd, &bdf);
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
                eprintln!("║   {}: {} diffs, {} applied, {} stuck, {} PRI-skipped",
                    dr.name, dr.diffs, dr.applied, dr.stuck, dr.pri_skipped);
            }
        }

        eprintln!("╠══ Summary ═════════════════════════════════════════════════╣");
        eprintln!("║ Total diffs: {}", result.total_diffs);
        eprintln!("║ Applied: {}", result.applied);
        eprintln!("║ Stuck: {}", result.stuck);
        eprintln!("║ PRI-skipped: {}", result.pri_skipped);
        eprintln!("║ Danger-skipped: {}", result.danger_skipped);
        eprintln!("║ VRAM unlocked: {} (after {:?})", result.vram_unlocked, result.vram_unlocked_after);

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
        use coral_driver::nv::RawVfioDevice;
        use coral_driver::vfio::channel::oracle::OracleState;
        use coral_driver::vfio::channel::diagnostic::boot_follower::{BootDiff, BootTrace};
        use coral_driver::vfio::channel::registers::pri;

        let bdf = vfio_bdf();
        let raw = RawVfioDevice::open(&bdf)
            .expect("RawVfioDevice::open()");

        eprintln!("╔══════════════════════════════════════════════════════════════╗");
        eprintln!("║ Boot Sequence Follower — Oracle vs Cold Diff                ║");
        eprintln!("╠══════════════════════════════════════════════════════════════╣");

        // Load oracle
        let oracle = if let Ok(oracle_bdf) = std::env::var("CORALREEF_ORACLE_BDF") {
            OracleState::from_live_card(&oracle_bdf)
                .expect("failed to read oracle BAR0")
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
                eprintln!("║ {domain:12}: {}/{} changed, {} cold-dead, {} warm-alive",
                    stats.changed, stats.compared, stats.cold_dead, stats.warm_alive);
            }
        }

        // Extract and display recipe
        let recipe = diff.to_recipe();
        eprintln!("║");
        eprintln!("╠══ Init Recipe ({} steps) ═════════════════════════════════╣", recipe.len());
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
        use coral_driver::nv::RawVfioDevice;

        let bdf = vfio_bdf();

        // ── Helper: probe all VRAM-related domains ────────────────────────
        fn probe_hbm2_health(bar0: &coral_driver::vfio::device::MappedBar) -> Vec<(&'static str, usize, u32, bool)> {
            let domains: &[(&str, usize)] = &[
                ("BOOT0",   0x000000),
                ("PMC_EN",  0x000200),
                ("PFIFO",   0x002004),
                ("PFB",     0x100000),
                ("FBHUB",   0x100800),
                ("PFB_NISO",0x100C80),
                ("PMU",     0x10A000),
                ("LTC0",    0x17E200),
                ("FBPA0",   0x9A0000),
                ("NVPLL",   0x137050),
                ("MEMPLL",  0x137100),
                ("PRAMIN",  0x700000),
                ("PRAMIN+4",0x700004),
                ("PRAMIN+8",0x700008),
            ];

            domains.iter().map(|&(name, off)| {
                let val = bar0.read_u32(off).unwrap_or(0xDEAD_DEAD);
                let alive = val != 0xDEAD_DEAD
                    && val != 0xFFFF_FFFF
                    && (val >> 16) != 0xBADF
                    && (val >> 16) != 0xBAD0
                    && (val >> 16) != 0xBAD1;
                (name, off, val, alive)
            }).collect()
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
            let raw = RawVfioDevice::open(&bdf)
                .expect("VFIO open failed");
            let h = probe_hbm2_health(&raw.bar0);
            print_health("POST state", &h);

            let pramin_alive = h.iter().any(|x| x.0.starts_with("PRAMIN") && x.3);
            eprintln!("║ VRAM accessible: {pramin_alive}");

            // Write a sentinel to PRAMIN if accessible
            if pramin_alive {
                let sentinel: u32 = 0xC0EE_1EEF;
                raw.bar0.write_u32(0x700000, sentinel).ok();
                let readback = raw.bar0.read_u32(0x700000).unwrap_or(0);
                eprintln!("║ Sentinel write/read: wrote {sentinel:#010x}, read {readback:#010x}, match={}",
                    readback == sentinel);
            }

            eprintln!("║ Dropping VFIO fd (this triggers PM reset)...");
            // raw drops here — fd closes, kernel does PM reset
        }

        std::thread::sleep(std::time::Duration::from_secs(2));

        // ── Phase 2: Re-open after fd close (PM reset happened) ───────────
        eprintln!("╠══ PHASE 2: RE-OPEN AFTER PM RESET ═════════════════════════╣");
        {
            // Pin D0 first
            let _ = std::fs::write(
                format!("/sys/bus/pci/devices/{bdf}/power/control"), "on"
            );
            std::thread::sleep(std::time::Duration::from_millis(500));

            let raw = RawVfioDevice::open(&bdf)
                .expect("VFIO re-open failed");
            let h = probe_hbm2_health(&raw.bar0);
            print_health("After PM reset", &h);

            let pramin_alive = h.iter().any(|x| x.0.starts_with("PRAMIN") && x.3);
            eprintln!("║ VRAM accessible: {pramin_alive}");

            if pramin_alive {
                let readback = raw.bar0.read_u32(0x700000).unwrap_or(0);
                eprintln!("║ Sentinel survived PM reset? read {readback:#010x} (expected 0xC0EE1EEF)");
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
                        if let Some(s) = c.stdin.as_mut() { s.write_all(val.as_bytes())?; }
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
        sysfs_write(&format!("/sys/bus/pci/devices/{bdf}/driver_override"), "vfio-pci");
        sysfs_write("/sys/bus/pci/drivers/vfio-pci/bind", &bdf);
        std::thread::sleep(std::time::Duration::from_secs(1));

        // Pin D0
        let _ = std::fs::write(
            format!("/sys/bus/pci/devices/{bdf}/power/control"), "on"
        );
        std::thread::sleep(std::time::Duration::from_millis(500));

        let raw = RawVfioDevice::open(&bdf)
            .expect("VFIO open after resurrection failed");
        let h = probe_hbm2_health(&raw.bar0);
        print_health("After nouveau resurrection", &h);

        let pramin_alive = h.iter().any(|x| x.0.starts_with("PRAMIN") && x.3);
        eprintln!("║ VRAM RESURRECTED: {pramin_alive}");

        // Try the sentinel test on resurrected VRAM
        if pramin_alive {
            let sentinel: u32 = 0xDEAD_BEEF;
            raw.bar0.write_u32(0x700000, sentinel).ok();
            let readback = raw.bar0.read_u32(0x700000).unwrap_or(0);
            eprintln!("║ Post-resurrection sentinel: wrote {sentinel:#010x}, read {readback:#010x}, match={}",
                readback == sentinel);
        }

        let alive_count = h.iter().filter(|x| x.3).count();
        eprintln!("╠══════════════════════════════════════════════════════════════╣");
        eprintln!("║ SUMMARY:");
        eprintln!("║   Phase 1 (POST state):       check log above");
        eprintln!("║   Phase 2 (after PM reset):    check log above");
        eprintln!("║   Phase 3 (nouveau warm):      driver={:?}", drv);
        eprintln!("║   Phase 4 (resurrection):      {alive_count}/{} domains, VRAM={pramin_alive}", h.len());
        eprintln!("╚══════════════════════════════════════════════════════════════╝");

    }
}
