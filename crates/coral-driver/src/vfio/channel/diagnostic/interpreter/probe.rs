// SPDX-License-Identifier: AGPL-3.0-only
//! Probe interpreter — chains layer probes and collects a full report.

use std::os::fd::RawFd;
use std::time::Instant;

use crate::vfio::channel::glowplug::GlowPlug;
use crate::vfio::device::MappedBar;
use crate::vfio::dma::DmaBuffer;
use crate::vfio::memory::{MemoryRegion, MemoryTopology, PathStatus, PraminRegion};

use super::layers::*;
use super::memory_probe;
use crate::vfio::channel::registers::*;

/// Full probe report — contains either a successful layer output or
/// the failure point with all evidence collected up to that layer.
#[derive(Debug)]
pub struct ProbeReport {
    pub bar: Option<BarTopology>,
    pub identity: Option<GpuIdentity>,
    pub power: Option<PowerState>,
    pub engines: Option<EngineTopology>,
    pub memory: Option<MemoryTopology>,
    pub dma: Option<DmaCapability>,
    pub channel: Option<ChannelConfig>,
    pub dispatch: Option<DispatchCapability>,
    pub failures: Vec<ProbeFailure>,
    pub elapsed_ms: u128,
}

impl ProbeReport {
    /// Highest layer reached successfully.
    pub fn depth(&self) -> u8 {
        if self.dispatch.is_some() {
            8
        } else if self.channel.is_some() {
            7
        } else if self.dma.is_some() {
            6
        } else if self.memory.is_some() {
            5
        } else if self.engines.is_some() {
            4
        } else if self.power.is_some() {
            3
        } else if self.identity.is_some() {
            2
        } else if self.bar.is_some() {
            1
        } else {
            0
        }
    }

    /// Print human-readable summary to stderr.
    pub fn print_summary(&self) {
        eprintln!("╔══ INTERPRETER PROBE REPORT ════════════════════════════════╗");
        eprintln!("║ Depth: Layer {}/8 in {}ms", self.depth(), self.elapsed_ms);

        if let Some(bar) = &self.bar {
            eprintln!(
                "║ L0 BAR:     read={} write={} d3hot={} BOOT0={:#010x}",
                bar.bar0_readable, bar.bar0_writable, bar.in_d3hot, bar.boot0_raw
            );
        }
        if let Some(id) = &self.identity {
            eprintln!(
                "║ L1 ID:      {} impl={} rev={} BOOT0={:#010x}",
                id.architecture, id.implementation, id.revision, id.boot0
            );
        }
        if let Some(pwr) = &self.power {
            eprintln!(
                "║ L2 POWER:   {:?} pfifo={} ptimer={} PMC={:#010x}",
                pwr.method, pwr.pfifo_enabled, pwr.ptimer_ticking, pwr.pmc_enable_final
            );
        }
        if let Some(eng) = &self.engines {
            eprintln!(
                "║ L3 ENGINES: pbdma_map={:#010x} gr_rl={:?} gr_pbdma={:?}",
                eng.pbdma_map, eng.gr_runlist, eng.gr_pbdma
            );
            eprintln!(
                "║             BAR1={:#010x} BAR2={:#010x} bar2_setup={}",
                eng.bar1_block, eng.bar2_block, eng.bar2_setup_needed
            );
        }
        if let Some(mem) = &self.memory {
            let pramin_ok = mem.pramin_works();
            let dma_ok = mem.dma_works();
            let working = mem.paths.iter().filter(|p| p.status.is_working()).count();
            let total = mem.paths.len();
            eprintln!(
                "║ L3.5 MEM:   vram={} pramin={pramin_ok} dma={dma_ok} bar2={} paths={working}/{total}",
                mem.vram_accessible, mem.bar2_configured
            );
        }
        if let Some(dma) = &self.dma {
            eprintln!(
                "║ L4 DMA:     read={} write={} iommu={} pt={} inst={}",
                dma.gpu_can_read_sysmem,
                dma.gpu_can_write_sysmem,
                dma.iommu_mapping_ok,
                dma.page_tables_ok,
                dma.instance_block_accessible
            );
            if !dma.ctx_evidence.is_empty() {
                eprint!("║             CTX:");
                for (name, val) in &dma.ctx_evidence {
                    eprint!(" {name}={val:#010x}");
                }
                eprintln!();
            }
        }
        if let Some(ch) = &self.channel {
            eprintln!(
                "║ L5 CHANNEL: inst_tgt={} userd_tgt={} vram_inst={} method={:?}",
                ch.working_inst_target,
                ch.working_userd_target,
                ch.instance_requires_vram,
                ch.scheduling_method
            );
        }
        if let Some(disp) = &self.dispatch {
            eprintln!(
                "║ L6 DISPATCH: consumed={} nop={} ready={}",
                disp.gpfifo_consumed, disp.nop_executed, disp.dispatch_ready
            );
            for b in &disp.blockers {
                eprintln!("║   BLOCKER: {b}");
            }
        }

        if !self.failures.is_empty() {
            eprintln!("╠══ FAILURES ═══════════════════════════════════════════════╣");
            for f in &self.failures {
                eprintln!("║ {f}");
            }
        }
        eprintln!("╚═══════════════════════════════════════════════════════════╝");
    }
}

/// The probe interpreter — runs layered discovery on a VFIO GPU.
pub struct ProbeInterpreter<'a> {
    bar0: &'a MappedBar,
    container_fd: RawFd,
}

impl<'a> ProbeInterpreter<'a> {
    pub fn new(bar0: &'a MappedBar, container_fd: RawFd) -> Self {
        Self { bar0, container_fd }
    }

    fn r(&self, reg: usize) -> u32 {
        self.bar0.read_u32(reg).unwrap_or(0xDEAD_DEAD)
    }

    fn w(&self, reg: usize, val: u32) {
        let _ = self.bar0.write_u32(reg, val);
    }

    /// Run the full probe chain, stopping at the first fatal failure.
    pub fn run(&self) -> ProbeReport {
        let start = Instant::now();
        let mut report = ProbeReport {
            bar: None,
            identity: None,
            power: None,
            engines: None,
            memory: None,
            dma: None,
            channel: None,
            dispatch: None,
            failures: Vec::new(),
            elapsed_ms: 0,
        };

        // Layer 0: BAR
        let bar = self.probe_bar();
        if bar.in_d3hot {
            report.failures.push(ProbeFailure {
                layer: "L0_BAR",
                step: "d3hot_check",
                evidence: vec![("BOOT0".into(), bar.boot0_raw)],
                message: "GPU in D3hot — PCIe sleep. Set power/control=on.".into(),
            });
            report.bar = Some(bar);
            report.elapsed_ms = start.elapsed().as_millis();
            return report;
        }
        report.bar = Some(bar.clone());

        // Layer 1: Identity
        let identity = self.probe_identity(bar);
        report.identity = Some(identity.clone());

        // Layer 2: Power
        match self.probe_power(identity) {
            Ok(power) => {
                if !power.pfifo_enabled {
                    report.failures.push(ProbeFailure {
                        layer: "L2_POWER",
                        step: "pfifo_enable",
                        evidence: vec![
                            ("PFIFO_ENABLE".into(), power.pfifo_enable_raw),
                            ("PMC_ENABLE".into(), power.pmc_enable_final),
                        ],
                        message: format!(
                            "PFIFO_ENABLE stuck at {:#x} — method {:?} insufficient. \
                             Trying PMC reset cycle.",
                            power.pfifo_enable_raw, power.method
                        ),
                    });
                }

                // L2.5: GlowPlug — if power probe left GPU partially warm,
                // use the full glowplug sequence (PMC + PFIFO + BAR2 + FB init).
                let power = if matches!(power.method, PowerMethod::GlowPlug | PowerMethod::Failed) {
                    eprintln!(
                        "║ L2.5: Power method {:?} — invoking GlowPlug.full_init()",
                        power.method
                    );
                    let gp = GlowPlug::new(self.bar0, self.container_fd);
                    let warm_result = gp.full_init();
                    for msg in &warm_result.log {
                        eprintln!("║   GP: {msg}");
                    }
                    if let Some(mem) = &warm_result.memory {
                        mem.print_summary();
                    }

                    // Re-probe power state after glowplug
                    let pmc_final = self.r(pmc::ENABLE);
                    let pfifo_final = self.r(pfifo::ENABLE);
                    let pbdma_after = self.r(pfifo::PBDMA_MAP);
                    let ptimer = self.check_ptimer();
                    let pfifo_ok =
                        pmc_final & (1 << 8) != 0 && pbdma_after != 0 && pbdma_after != 0xBAD0_DA00;

                    let method = if warm_result.success {
                        PowerMethod::GlowPlug
                    } else if pfifo_ok {
                        PowerMethod::PmcResetCycle
                    } else {
                        PowerMethod::Failed
                    };

                    PowerState {
                        identity: power.identity.clone(),
                        pmc_enable_initial: power.pmc_enable_initial,
                        pmc_enable_final: pmc_final,
                        engines_present: pmc_final,
                        pfifo_enabled: pfifo_ok,
                        pfifo_enable_raw: pfifo_final,
                        method,
                        ptimer_ticking: ptimer,
                    }
                } else {
                    power.clone()
                };

                report.power = Some(power.clone());

                // Layer 3: Engines
                match self.probe_engines(power) {
                    Ok(engines) => {
                        report.engines = Some(engines.clone());

                        // Layer 3.5: Memory Topology
                        let mem_topo =
                            memory_probe::discover_memory_topology(self.bar0, self.container_fd);
                        mem_topo.print_summary();
                        report.memory = Some(mem_topo);

                        // Layer 4: DMA validation
                        match self.probe_dma(engines.clone()) {
                            Ok(dma) => {
                                if !dma.instance_block_accessible {
                                    report.failures.push(ProbeFailure {
                                        layer: "L4_DMA",
                                        step: "instance_access",
                                        evidence: dma.ctx_evidence.clone(),
                                        message:
                                            "GPU cannot read instance block from system memory. \
                                             PBDMA CTX shows error pattern (0xdead prefix)."
                                                .into(),
                                    });
                                }
                                report.dma = Some(dma.clone());

                                // Layer 5: Channel — test GPFIFO consumption
                                if dma.instance_block_accessible {
                                    match self.probe_channel(&dma) {
                                        Ok(ch) => {
                                            report.channel = Some(ch.clone());

                                            // Layer 6: Dispatch — test method execution
                                            match self.probe_dispatch(&ch) {
                                                Ok(disp) => {
                                                    report.dispatch = Some(disp);
                                                }
                                                Err(f) => {
                                                    report.failures.push(f);
                                                }
                                            }
                                        }
                                        Err(f) => {
                                            report.failures.push(f);
                                        }
                                    }
                                }
                            }
                            Err(f) => {
                                report.failures.push(f);
                            }
                        }
                    }
                    Err(f) => {
                        report.failures.push(f);
                    }
                }
            }
            Err(f) => {
                report.failures.push(f);
            }
        }

        report.elapsed_ms = start.elapsed().as_millis();
        report
    }

    // ─── Layer 0: BAR Topology ─────────────────────────────────────────

    fn probe_bar(&self) -> BarTopology {
        let boot0 = self.r(0);
        let all_ff = boot0 == 0xFFFF_FFFF;
        let all_zero = boot0 == 0;
        let in_d3hot = all_ff;

        // Test BAR0 write capability. NV_PMC_SCRATCH_0 (0x1400) doesn't exist on GV100.
        // PRAMIN window (0x1700) is writable on all arches: steer it and restore.
        let bar0_writable = if !in_d3hot {
            let saved_window = self.r(0x1700);
            self.w(0x1700, 0x0001_0000);
            let readback = self.r(0x1700);
            self.w(0x1700, saved_window);
            readback == 0x0001_0000
        } else {
            false
        };

        BarTopology {
            bar0_readable: !all_ff && !all_zero,
            bar0_writable,
            boot0_raw: boot0,
            in_d3hot,
        }
    }

    // ─── Layer 1: Identity ─────────────────────────────────────────────

    fn probe_identity(&self, bar: BarTopology) -> GpuIdentity {
        let boot0 = bar.boot0_raw;
        let arch = GpuArch::from_boot0(boot0);
        let implementation = ((boot0 >> 20) & 0xFF) as u8;
        let revision = (boot0 & 0xFF) as u8;
        let boot42 = {
            let v = self.r(0x0000_00A8);
            if v != 0 && v != 0xDEAD_DEAD && v != 0xBAD0_0200 {
                Some(v)
            } else {
                None
            }
        };

        GpuIdentity {
            bar,
            boot0,
            architecture: arch,
            implementation,
            revision,
            boot42,
        }
    }

    // ─── Layer 2: Power ────────────────────────────────────────────────

    fn probe_power(&self, identity: GpuIdentity) -> Result<PowerState, ProbeFailure> {
        let pmc_initial = self.r(pmc::ENABLE);
        let pfifo_reg = self.r(pfifo::ENABLE);

        // On GV100 (Volta), NV_PFIFO_ENGINE (0x2200) does NOT exist — the nouveau oracle
        // also reads 0 here. PFIFO is controlled purely via PMC_ENABLE bit 8.
        // Check PBDMA_MAP (0x2004) as the real indicator of PFIFO health.
        let is_volta_plus = matches!(
            identity.architecture,
            GpuArch::Volta | GpuArch::Turing | GpuArch::Ampere | GpuArch::Ada | GpuArch::Blackwell
        );

        let pbdma_map = self.r(pfifo::PBDMA_MAP);
        let pfifo_functional = if is_volta_plus {
            // On Volta+, PFIFO health = PMC bit 8 set AND PBDMA_MAP non-zero
            let pmc_pfifo_bit = pmc_initial & (1 << 8) != 0;
            let pbdma_alive = pbdma_map != 0 && pbdma_map != 0xBAD0_DA00;
            pmc_pfifo_bit && pbdma_alive
        } else {
            pfifo_reg == 1
        };

        let already_warm =
            pmc_initial != 0x4000_0020 && pfifo_reg != 0xBAD0_DA00 && pfifo_functional;

        if already_warm {
            let ptimer = self.check_ptimer();
            return Ok(PowerState {
                identity,
                pmc_enable_initial: pmc_initial,
                pmc_enable_final: pmc_initial,
                engines_present: pmc_initial,
                pfifo_enabled: true,
                pfifo_enable_raw: pfifo_reg,
                method: PowerMethod::AlreadyWarm,
                ptimer_ticking: ptimer,
            });
        }

        // Step 1: PMC_ENABLE — clock all engines
        eprintln!("║ L2: PMC_ENABLE={pmc_initial:#010x} PBDMA_MAP={pbdma_map:#010x} — warming");
        self.w(pmc::ENABLE, 0xFFFF_FFFF);
        std::thread::sleep(std::time::Duration::from_millis(50));
        let pmc_after = self.r(pmc::ENABLE);

        // Step 2: On pre-Volta, try direct PFIFO enable
        if !is_volta_plus {
            self.w(pfifo::ENABLE, 1);
            std::thread::sleep(std::time::Duration::from_millis(20));
        }

        // Step 3: PMC reset cycle for PFIFO engine (bit 8)
        let pfifo_bit: u32 = 1 << 8;
        let pmc_cur = self.r(pmc::ENABLE);
        self.w(pmc::ENABLE, pmc_cur & !pfifo_bit);
        std::thread::sleep(std::time::Duration::from_millis(20));
        self.w(pmc::ENABLE, pmc_cur | pfifo_bit);
        std::thread::sleep(std::time::Duration::from_millis(50));

        if !is_volta_plus {
            self.w(pfifo::ENABLE, 1);
            std::thread::sleep(std::time::Duration::from_millis(20));
        }

        let pmc_final = self.r(pmc::ENABLE);
        let pfifo_final_reg = self.r(pfifo::ENABLE);
        let pbdma_after = self.r(pfifo::PBDMA_MAP);
        let ptimer = self.check_ptimer();

        let pfifo_ok = if is_volta_plus {
            let has_bit8 = pmc_final & pfifo_bit != 0;
            let has_pbdma = pbdma_after != 0 && pbdma_after != 0xBAD0_DA00;
            eprintln!(
                "║ L2: PMC={pmc_final:#010x} bit8={has_bit8} PBDMA_MAP={pbdma_after:#010x} ptimer={ptimer}"
            );
            has_bit8 && has_pbdma
        } else {
            pfifo_final_reg == 1
        };

        let method = if pfifo_ok {
            PowerMethod::PmcResetCycle
        } else if pmc_final != 0x4000_0020 {
            PowerMethod::GlowPlug
        } else {
            PowerMethod::Failed
        };

        Ok(PowerState {
            identity,
            pmc_enable_initial: pmc_initial,
            pmc_enable_final: pmc_final,
            engines_present: pmc_final,
            pfifo_enabled: pfifo_ok,
            pfifo_enable_raw: pfifo_final_reg,
            method,
            ptimer_ticking: ptimer,
        })
    }

    fn check_ptimer(&self) -> bool {
        let t0 = self.r(0x009400);
        std::thread::sleep(std::time::Duration::from_millis(5));
        let t1 = self.r(0x009400);
        t0 != t1 && t0 != 0xDEAD_DEAD && t0 != 0xBAD0_DA00
    }

    // ─── Layer 3: Engine Topology ──────────────────────────────────────

    #[expect(clippy::cast_possible_truncation)]
    fn probe_engines(&self, power: PowerState) -> Result<EngineTopology, ProbeFailure> {
        let pbdma_map = self.r(pfifo::PBDMA_MAP);
        if pbdma_map == 0 || pbdma_map == 0xBAD0_DA00 {
            return Err(ProbeFailure {
                layer: "L3_ENGINES",
                step: "pbdma_map",
                evidence: vec![
                    ("PBDMA_MAP".into(), pbdma_map),
                    ("PFIFO_ENABLE".into(), power.pfifo_enable_raw),
                ],
                message: "No PBDMAs detected — PFIFO not functional".into(),
            });
        }

        let mut pbdma_to_runlist = Vec::new();
        let mut gr_runlist: Option<u32> = None;
        let mut gr_pbdma: Option<usize> = None;
        let mut alt_pbdma: Option<usize> = None;

        // Enumerate PBDMA → runlist mapping
        let mut seq = 0_usize;
        for pid in 0..32_usize {
            if pbdma_map & (1 << pid) == 0 {
                continue;
            }
            let rl = self.r(0x2390 + seq * 4);
            pbdma_to_runlist.push((pid, rl));
            seq += 1;
        }

        // Find GR engine's runlist via PTOP engine table
        let mut cur_type: u32 = 0xFFFF;
        let mut cur_rl: u32 = 0xFFFF;
        for i in 0..64_u32 {
            let data = self.r(0x0002_2700 + (i as usize) * 4);
            if data == 0 {
                break;
            }
            match data & 3 {
                1 => cur_type = (data >> 2) & 0x3F,
                3 => cur_rl = (data >> 11) & 0x1F,
                _ => {}
            }
            if data & (1 << 31) != 0 {
                if cur_type == 0 && gr_runlist.is_none() && cur_rl != 0xFFFF {
                    gr_runlist = Some(cur_rl);
                }
                cur_type = 0xFFFF;
                cur_rl = 0xFFFF;
            }
        }

        // Fallback: use ENGN0_STATUS
        if gr_runlist.is_none() {
            let engn0 = self.r(0x2640);
            let rl = (engn0 >> 12) & 0xF;
            if rl <= 31 {
                gr_runlist = Some(rl);
            }
        }

        // Find PBDMAs serving the GR runlist.
        // PBDMA_RUNL_MAP is a bitmask: bit N set = PBDMA serves runlist N.
        if let Some(target_rl) = gr_runlist {
            let rl_bit = 1u32 << target_rl;
            let mut found_first = false;
            for &(pid, rl_mask) in &pbdma_to_runlist {
                if rl_mask & rl_bit != 0 {
                    if !found_first {
                        gr_pbdma = Some(pid);
                        found_first = true;
                    } else if alt_pbdma.is_none() {
                        alt_pbdma = Some(pid);
                    }
                }
            }
        }

        // BAR block registers
        let bar1_block = self.r(misc::PBUS_BAR1_BLOCK);
        let bar2_block = self.r(misc::PBUS_BAR2_BLOCK);
        let bar2_invalid =
            bar2_block == 0x4000_0000 || bar2_block == 0 || bar2_block == 0xBAD0_DA00;

        let bar2_setup_needed = bar2_invalid;
        if bar2_setup_needed {
            eprintln!("║ L3: BAR2_BLOCK={bar2_block:#010x} (invalid) — will need page table setup");
            if let Err(e) = crate::vfio::channel::pfifo::setup_bar2_page_table(self.bar0) {
                eprintln!("║ L3: BAR2 setup failed: {e}");
            }
        }

        // Enable PBDMA and HCE interrupts for all active PBDMAs
        // (nouveau's gk104_fifo_init_pbdma does this for scheduler to function)
        for pid in 0..32_usize {
            if pbdma_map & (1 << pid) == 0 {
                continue;
            }
            self.w(pbdma::intr(pid), 0xFFFF_FFFF);
            self.w(pbdma::intr_en(pid), 0xEFFF_FEFF);
            self.w(pbdma::hce_intr(pid), 0xFFFF_FFFF);
            self.w(pbdma::hce_intr_en(pid), 0x8000_001F);
        }

        // Enable PFIFO interrupts — use the oracle's mask, not 0x7FFFFFFF
        self.w(pfifo::INTR, 0xFFFF_FFFF);
        self.w(pfifo::INTR_EN, 0x6181_0101);

        // MMU fault buffers — allocate in system memory DMA (not VRAM).
        // The GPU's PFIFO scheduler stalls silently without valid fault buffers.
        // Runner.rs pattern: DmaBuffer at FAULT_BUF_IOVA, BUF0_PUT bit 31 = enable.
        {
            let fb = DmaBuffer::new(self.container_fd, 4096, FAULT_BUF_IOVA);
            if let Ok(fault_buf) = &fb {
                fault_buf.as_slice(); // ensure mlock'd
            }
            let fb_lo = (FAULT_BUF_IOVA >> 12) as u32;
            let fb_entries: u32 = 64; // 64 entries x 32 bytes = 2KB fits in 4K page
            self.w(mmu::FAULT_BUF0_LO, fb_lo);
            self.w(mmu::FAULT_BUF0_HI, 0);
            self.w(mmu::FAULT_BUF0_SIZE, fb_entries);
            self.w(mmu::FAULT_BUF0_PUT, 0x8000_0000); // enable bit
            self.w(mmu::FAULT_BUF1_LO, fb_lo);
            self.w(mmu::FAULT_BUF1_HI, 0);
            self.w(mmu::FAULT_BUF1_SIZE, fb_entries);
            self.w(mmu::FAULT_BUF1_PUT, 0x8000_0000);
            eprintln!(
                "║ L3: MMU fault buffers (DMA): BUF0_LO={:#x} BUF1_LO={:#x} (IOVA={FAULT_BUF_IOVA:#x})",
                self.r(mmu::FAULT_BUF0_LO),
                self.r(mmu::FAULT_BUF1_LO),
            );
            // Keep DMA buffer alive through the engine topology scope.
            // It will be dropped when `fb` goes out of scope after this block,
            // but the GPU retains the physical mapping via IOMMU.
            std::mem::drop(fb);
        }

        Ok(EngineTopology {
            power,
            pbdma_map,
            pbdma_to_runlist,
            gr_runlist,
            gr_pbdma,
            alt_pbdma,
            bar1_block: self.r(misc::PBUS_BAR1_BLOCK),
            bar2_block: self.r(misc::PBUS_BAR2_BLOCK),
            bar2_setup_needed,
        })
    }

    // ─── Layer 4: DMA Validation ───────────────────────────────────────

    /// Test whether the GPU can DMA-read an instance block from system memory.
    ///
    /// Strategy: allocate a DMA buffer, populate it with known RAMFC values,
    /// INST_BIND it, then read back the PBDMA CTX registers to see if the
    /// GPU loaded the expected values or returned error patterns.
    #[expect(clippy::cast_possible_truncation)]
    fn probe_dma(&self, engines: EngineTopology) -> Result<DmaCapability, ProbeFailure> {
        let channel_id: u32 = 0;
        let gpfifo_iova: u64 = 0x1000; // matches RawVfioDevice::gpfifo_iova()
        let userd_iova: u64 = 0x2000; // matches RawVfioDevice::userd_iova()

        // Allocate test DMA buffers
        let mut instance = match DmaBuffer::new(self.container_fd, 4096, INSTANCE_IOVA) {
            Ok(b) => b,
            Err(e) => {
                return Err(ProbeFailure {
                    layer: "L4_DMA",
                    step: "alloc_instance",
                    evidence: vec![],
                    message: format!("DMA buffer allocation failed: {e}"),
                });
            }
        };
        let mut pd3 = DmaBuffer::new(self.container_fd, 4096, PD3_IOVA).ok();
        let mut pd2 = DmaBuffer::new(self.container_fd, 4096, PD2_IOVA).ok();
        let mut pd1 = DmaBuffer::new(self.container_fd, 4096, PD1_IOVA).ok();
        let mut pd0 = DmaBuffer::new(self.container_fd, 4096, PD0_IOVA).ok();
        let mut pt0 = DmaBuffer::new(self.container_fd, 4096, PT0_IOVA).ok();

        let iommu_ok = pd3.is_some() && pd2.is_some() && pd1.is_some();

        // Set up page tables if we got all the buffers
        let pt_ok = if let (Some(d3), Some(d2), Some(d1), Some(d0), Some(t0)) =
            (&mut pd3, &mut pd2, &mut pd1, &mut pd0, &mut pt0)
        {
            crate::vfio::channel::page_tables::populate_page_tables(
                d3.as_mut_slice(),
                d2.as_mut_slice(),
                d1.as_mut_slice(),
                d0.as_mut_slice(),
                t0.as_mut_slice(),
            );
            true
        } else {
            false
        };

        // Populate instance block with known RAMFC values
        crate::vfio::channel::page_tables::populate_instance_block_static(
            instance.as_mut_slice(),
            gpfifo_iova,
            8,
            userd_iova,
            channel_id,
        );

        // Flush CPU caches to ensure DMA coherence (AMD Zen 2 + VFIO)
        #[cfg(target_arch = "x86_64")]
        {
            let ptr = instance.as_slice().as_ptr();
            let len = instance.as_slice().len();
            let mut addr = ptr as usize & !63;
            let end = (ptr as usize + len + 63) & !63;
            while addr < end {
                unsafe { std::arch::x86_64::_mm_clflush(addr as *const u8) };
                addr += 64;
            }
            unsafe { std::arch::x86_64::_mm_mfence() };
        }

        // Find the PBDMA to use
        let pbdma_id = engines.gr_pbdma.unwrap_or(1);
        let pb = 0x040000 + pbdma_id * 0x2000;

        // Write sentinels to PBDMA CTX registers so we can detect if INST_BIND overwrites them
        self.w(pb + pbdma::CTX_USERD_LO, 0xBEEF_0008);
        self.w(pb + pbdma::CTX_SIGNATURE, 0xBEEF_0010);
        self.w(pb + pbdma::CTX_GP_BASE_LO, 0xBEEF_0048);
        self.w(pb + pbdma::CTX_ACQUIRE, 0xBEEF_0030);

        // Clear stale PCCSR
        let stale = self.r(pccsr::channel(channel_id));
        if stale & 1 != 0 {
            self.w(pccsr::channel(channel_id), pccsr::CHANNEL_ENABLE_CLR);
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        self.w(pccsr::inst(channel_id), 0);
        std::thread::sleep(std::time::Duration::from_millis(5));
        self.w(pfifo::INTR, 0xFFFF_FFFF);

        // ── Progressive INST_BIND sequence ──────────────────────────────
        // Nouveau's init: (1) enable PFIFO interrupts, (2) submit runlists,
        // (3) write PCCSR_INST with INST_BIND, (4) write PCCSR_CHANNEL ENABLE_SET.
        // We try incrementally to discover which steps are required.

        let mut ctx_evidence: Vec<(String, u32)> = vec![];
        let mut instance_accessible = false;
        let mut gpu_can_read = false;

        // ── PRAMIN self-test: check if VRAM is accessible at ALL ───────
        // Uses the unified PraminRegion abstraction for RAII window management.
        let mut pramin_ok = false;
        let vram_inst_addr: u32;
        {
            // Test 1: Read glow plug's BAR2 instance block at VRAM 0x20000
            let bar2_inst_0;
            let bar2_inst_4;
            let bar2_pdb_lo;
            if let Ok(region) = PraminRegion::new(self.bar0, BAR2_VRAM_BASE, 0x1000) {
                bar2_inst_0 = region.read_u32(0).unwrap_or(0xDEAD_DEAD);
                bar2_inst_4 = region.read_u32(4).unwrap_or(0xDEAD_DEAD);
                bar2_pdb_lo = region.read_u32(0x200).unwrap_or(0xDEAD_DEAD);
            } else {
                bar2_inst_0 = 0xDEAD_DEAD;
                bar2_inst_4 = 0xDEAD_DEAD;
                bar2_pdb_lo = 0xDEAD_DEAD;
            }
            let is_bad = (bar2_inst_0 >> 16) == 0xBAD0 || bar2_inst_0 == 0xBAD0_AC00;
            eprintln!(
                "║ L4: VRAM readback @ 0x{:x}: [0]={bar2_inst_0:#x} [4]={bar2_inst_4:#x} \
                 [PDB]={bar2_pdb_lo:#x} bad={}",
                BAR2_VRAM_BASE, is_bad
            );

            // Test 2: Write/readback at VRAM 0x0 via PraminRegion
            let vram0_status = if let Ok(mut region) = PraminRegion::new(self.bar0, 0, 8) {
                region.probe_sentinel(0, 0xDEAD_BEEF)
            } else {
                PathStatus::ErrorPattern { pattern: 0 }
            };
            let vram0_ok = vram0_status.is_working();
            let vram0_after = match &vram0_status {
                PathStatus::Working { .. } => 0xDEAD_BEEF,
                PathStatus::Corrupted { read, .. } => *read,
                PathStatus::ErrorPattern { pattern } => *pattern,
                PathStatus::Untested => 0,
            };
            eprintln!("║ L4: VRAM@0x00000: {vram0_status:?}");

            // Test 3: Write/readback at VRAM 0x26000 via PraminRegion
            let vram2_addr = BAR2_VRAM_BASE + 0x6000;
            let vram2_status = if let Ok(mut region) = PraminRegion::new(self.bar0, vram2_addr, 8) {
                region.probe_sentinel(0, 0xCAFE_1234)
            } else {
                PathStatus::ErrorPattern { pattern: 0 }
            };
            let vram2_ok = vram2_status.is_working();
            let test_rb = match &vram2_status {
                PathStatus::Working { .. } => 0xCAFE_1234,
                PathStatus::Corrupted { read, .. } => *read,
                PathStatus::ErrorPattern { pattern } => *pattern,
                PathStatus::Untested => 0,
            };
            eprintln!("║ L4: VRAM@{vram2_addr:#x}: {vram2_status:?}");

            pramin_ok = vram0_ok || vram2_ok;
            vram_inst_addr = if vram2_ok {
                vram2_addr
            } else if vram0_ok {
                0x0001_0000
            } else {
                0x0002_6000
            };

            ctx_evidence.push(("PRAMIN_VRAM0".into(), vram0_after));
            ctx_evidence.push(("PRAMIN_VRAM2".into(), test_rb));

            // Write instance block to VRAM via PraminRegion
            if pramin_ok {
                if let Ok(mut vram_region) = PraminRegion::new(self.bar0, vram_inst_addr, 4096) {
                    let inst_data = instance.as_slice();
                    for (i, chunk) in inst_data.chunks(4).enumerate() {
                        if chunk.len() == 4 {
                            let val = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
                            let _ = vram_region.write_u32(i * 4, val);
                        }
                    }
                    let verify_sig = vram_region.read_u32(ramfc::SIGNATURE).unwrap_or(0);
                    let verify_gpbase = vram_region.read_u32(ramfc::GP_BASE_LO).unwrap_or(0);
                    let verify_userd = vram_region.read_u32(ramfc::USERD_LO).unwrap_or(0);
                    eprintln!(
                        "║ L4: VRAM inst verify: SIG={verify_sig:#x} GP_BASE={verify_gpbase:#x} \
                         USERD={verify_userd:#x}"
                    );
                }
            }
        }

        // Run multiple bind strategies in sequence
        struct BindAttempt {
            label: &'static str,
            target: u32,          // 0=VRAM, 2=COH, 3=NCOH
            inst_addr_shr12: u32, // page-aligned address >> 12
            enable_channel: bool,
            submit_runlist: bool,
            enable_interrupts: bool,
        }

        let attempts = [
            BindAttempt {
                label: "A_sysmem_bind_only",
                target: TARGET_SYS_MEM_COHERENT,
                inst_addr_shr12: (INSTANCE_IOVA >> 12) as u32,
                enable_channel: false,
                submit_runlist: false,
                enable_interrupts: false,
            },
            BindAttempt {
                label: "B_sysmem_bind+enable",
                target: TARGET_SYS_MEM_COHERENT,
                inst_addr_shr12: (INSTANCE_IOVA >> 12) as u32,
                enable_channel: true,
                submit_runlist: false,
                enable_interrupts: false,
            },
            BindAttempt {
                label: "C_vram_bind+enable",
                target: 0, // VRAM
                inst_addr_shr12: vram_inst_addr >> 12,
                enable_channel: true,
                submit_runlist: false,
                enable_interrupts: false,
            },
            BindAttempt {
                label: "D_vram_full_sequence",
                target: 0, // VRAM
                inst_addr_shr12: vram_inst_addr >> 12,
                enable_channel: true,
                submit_runlist: true,
                enable_interrupts: true,
            },
            BindAttempt {
                label: "E_sysmem_full_sequence",
                target: TARGET_SYS_MEM_COHERENT,
                inst_addr_shr12: (INSTANCE_IOVA >> 12) as u32,
                enable_channel: true,
                submit_runlist: true,
                enable_interrupts: true,
            },
        ];

        // ── Verify RL register writes work ──────────────────────────────
        let gr_rl_id = engines.gr_runlist.unwrap_or(1) as usize;
        let rl_base_test_reg = 0x2270 + gr_rl_id * 0x10;
        let rl_submit_test_reg = rl_base_test_reg + 4;

        // Try writing a test value to RL BASE and reading back
        let rl_before = self.r(rl_base_test_reg);
        self.w(rl_base_test_reg, 0xCAFE_0001);
        let rl_after = self.r(rl_base_test_reg);
        self.w(rl_base_test_reg, rl_before); // restore

        // Also check RL0 for comparison
        let rl0_before = self.r(0x2270);
        self.w(0x2270, 0xCAFE_0002);
        let rl0_after = self.r(0x2270);
        self.w(0x2270, rl0_before);

        eprintln!(
            "║ L4: RL{gr_rl_id} write test: before={rl_before:#x} after_write={rl_after:#x} (expected 0xcafe0001)"
        );
        eprintln!(
            "║ L4: RL0 write test: before={rl0_before:#x} after_write={rl0_after:#x} (expected 0xcafe0002)"
        );
        ctx_evidence.push(("RL_WRITE_TEST".into(), rl_after));
        ctx_evidence.push(("RL0_WRITE_TEST".into(), rl0_after));

        eprintln!(
            "║ L4: Progressive INST_BIND sequence ({} attempts)",
            attempts.len()
        );

        // Also try direct PBDMA programming after the scheduled attempts
        let mut try_direct_pbdma = true;

        for attempt in &attempts {
            // Reset channel state
            self.w(pccsr::channel(channel_id), pccsr::CHANNEL_ENABLE_CLR);
            std::thread::sleep(std::time::Duration::from_millis(5));
            self.w(
                pccsr::channel(channel_id),
                pccsr::PBDMA_FAULTED_RESET | pccsr::ENG_FAULTED_RESET,
            );
            self.w(pccsr::inst(channel_id), 0);
            std::thread::sleep(std::time::Duration::from_millis(5));
            self.w(pfifo::INTR, 0xFFFF_FFFF);

            // Write sentinels
            self.w(pb + pbdma::CTX_USERD_LO, 0xBEEF_0008);
            self.w(pb + pbdma::CTX_SIGNATURE, 0xBEEF_0010);
            self.w(pb + pbdma::CTX_GP_BASE_LO, 0xBEEF_0048);

            if attempt.enable_interrupts {
                // Nouveau: nvkm_wr32(0x2140, 0x7FFFFFFF)
                self.w(pfifo::INTR_EN, 0x7FFF_FFFF);
            }

            if attempt.submit_runlist {
                // Build a GV100-format runlist and copy to VRAM via PRAMIN.
                // GV100 format (from nouveau gv100_runl_insert_cgrp/chan):
                //   TSG header (16 bytes):
                //     DW0: (tsg_id << 3) | 4  (bit 2 = TSG type)
                //     DW1: 0x80000000          (timeslice)
                //     DW2: channel_count
                //     DW3: 0
                //   Channel entry (8 bytes):
                //     DW0: channel_id
                //     DW1: 0
                // Total: 24 bytes, 2 entries (1 TSG + 1 channel)
                let vram_rl_addr: u32 = 0x0002_7000;
                if let Ok(mut rl_region) = PraminRegion::new(self.bar0, vram_rl_addr, 0x100) {
                    // TSG header
                    let _ = rl_region.write_u32(0x00, 0x0000_0004); // tsg_id=0, type=TSG
                    let _ = rl_region.write_u32(0x04, 0x8000_0000); // timeslice
                    let _ = rl_region.write_u32(0x08, 1); // 1 channel
                    let _ = rl_region.write_u32(0x0C, 0);
                    // Channel entry
                    let _ = rl_region.write_u32(0x10, channel_id);
                    let _ = rl_region.write_u32(0x14, 0);
                }

                let gr_rl = engines.gr_runlist.unwrap_or(1) as u32;
                self.w(
                    pfifo::runlist_base(gr_rl),
                    pfifo::gv100_runlist_base_value(vram_rl_addr as u64),
                );
                self.w(
                    pfifo::runlist_submit(gr_rl),
                    pfifo::gv100_runlist_submit_value(vram_rl_addr as u64, 2),
                );
                std::thread::sleep(std::time::Duration::from_millis(20));
            }

            // INST_BIND
            let inst_val = attempt.inst_addr_shr12 | (attempt.target << 28) | pccsr::INST_BIND_TRUE;
            self.w(pccsr::inst(channel_id), inst_val);
            std::thread::sleep(std::time::Duration::from_millis(10));

            if attempt.enable_channel {
                self.w(pccsr::channel(channel_id), pccsr::CHANNEL_ENABLE_SET);
                std::thread::sleep(std::time::Duration::from_millis(5));
                // Ring the doorbell — tells scheduler this channel has pending work.
                self.w(usermode::NOTIFY_CHANNEL_PENDING, channel_id);
                std::thread::sleep(std::time::Duration::from_millis(50));
            } else {
                std::thread::sleep(std::time::Duration::from_millis(50));
            }

            // Read results
            let ctx_userd = self.r(pb + pbdma::CTX_USERD_LO);
            let ctx_sig = self.r(pb + pbdma::CTX_SIGNATURE);
            let ctx_gpbase = self.r(pb + pbdma::CTX_GP_BASE_LO);
            let chsw = self.r(pfifo::CHSW_ERROR);
            let pfifo_intr = self.r(pfifo::INTR);
            let pccsr_ctrl = self.r(pccsr::channel(channel_id));
            let pccsr_inst_rb = self.r(pccsr::inst(channel_id));

            // Read PBDMA status registers for the assigned PBDMA
            let pbdma_status = self.r(pb + 0x118); // PBDMA_STATUS
            let pbdma_chan = self.r(pb + 0x120); // PBDMA_CHANNEL
            let pbdma_method = self.r(pb + 0x0B8); // PBDMA_GP_GET

            let gr_rl_rb = engines.gr_runlist.unwrap_or(1) as u32;
            let rl_submit_rb = self.r(pfifo::runlist_submit(gr_rl_rb));
            let rl_base_rb = self.r(pfifo::runlist_base(gr_rl_rb));

            // Read scheduler-related registers
            let sched_dis = self.r(pfifo::SCHED_DISABLE);

            let sentinels = (ctx_userd >> 16) == 0xBEEF || (ctx_sig >> 16) == 0xBEEF;
            let dead = (ctx_userd >> 16) == 0xDEAD || (ctx_sig >> 16) == 0xDEAD;
            let sig_ok = ctx_sig == 0x0000_FACE;
            let loaded = !sentinels && !dead;
            let status = (pccsr_ctrl >> 24) & 0xF;

            eprintln!(
                "║   {}: SIG={ctx_sig:#010x} USERD={ctx_userd:#010x} GP={ctx_gpbase:#010x} \
                 CHSW={chsw:#x} CTRL={pccsr_ctrl:#x}(st={status}) {}",
                attempt.label,
                if loaded {
                    if sig_ok {
                        "✓ LOADED+CORRECT"
                    } else {
                        "~ LOADED"
                    }
                } else if sentinels {
                    "✗ sentinels"
                } else {
                    "✗ dead"
                }
            );
            eprintln!(
                "║     PCCSR_INST={pccsr_inst_rb:#010x} PBDMA_ST={pbdma_status:#x} \
                 PBDMA_CH={pbdma_chan:#x} SCHED_DIS={sched_dis:#x} \
                 RL_BASE={rl_base_rb:#x} RL_SUB={rl_submit_rb:#x} INTR={pfifo_intr:#x}"
            );

            ctx_evidence.push((format!("{}_SIG", attempt.label), ctx_sig));
            ctx_evidence.push((format!("{}_USERD", attempt.label), ctx_userd));
            ctx_evidence.push((format!("{}_CTRL", attempt.label), pccsr_ctrl));
            ctx_evidence.push((format!("{}_PBDMA_ST", attempt.label), pbdma_status));
            ctx_evidence.push((format!("{}_SCHED_DIS", attempt.label), sched_dis));
            if chsw != 0 {
                ctx_evidence.push((format!("{}_CHSW", attempt.label), chsw));
            }

            if loaded {
                gpu_can_read = true;
                if sig_ok {
                    instance_accessible = true;
                }
                break;
            }
        }

        // ── Attempt F: Direct PBDMA programming (bypass scheduler) ─────
        if try_direct_pbdma && !instance_accessible {
            eprintln!("║ L4: Attempt F — direct PBDMA context programming (bypass scheduler)");

            // Reset channel
            self.w(pccsr::channel(channel_id), pccsr::CHANNEL_ENABLE_CLR);
            std::thread::sleep(std::time::Duration::from_millis(5));
            self.w(
                pccsr::channel(channel_id),
                pccsr::PBDMA_FAULTED_RESET | pccsr::ENG_FAULTED_RESET,
            );
            self.w(pccsr::inst(channel_id), 0);
            self.w(pfifo::INTR, 0xFFFF_FFFF);
            std::thread::sleep(std::time::Duration::from_millis(5));

            // Write VRAM instance block INST_BIND (target=0)
            let inst_val = (vram_inst_addr >> 12) | pccsr::INST_BIND_TRUE;
            self.w(pccsr::inst(channel_id), inst_val);
            std::thread::sleep(std::time::Duration::from_millis(5));

            // Read RAMFC fields from VRAM instance block via PraminRegion
            let (
                ramfc_gp_base_lo,
                ramfc_gp_base_hi,
                ramfc_gp_put,
                ramfc_userd_lo,
                ramfc_userd_hi,
                ramfc_sig,
            ) = if let Ok(inst_region) = PraminRegion::new(self.bar0, vram_inst_addr, 4096) {
                (
                    inst_region.read_u32(ramfc::GP_BASE_LO).unwrap_or(0),
                    inst_region.read_u32(ramfc::GP_BASE_HI).unwrap_or(0),
                    inst_region.read_u32(ramfc::GP_PUT).unwrap_or(0),
                    inst_region.read_u32(ramfc::USERD_LO).unwrap_or(0),
                    inst_region.read_u32(ramfc::USERD_HI).unwrap_or(0),
                    inst_region.read_u32(ramfc::SIGNATURE).unwrap_or(0),
                )
            } else {
                (0, 0, 0, 0, 0, 0)
            };

            eprintln!(
                "║   RAMFC: GP_BASE={ramfc_gp_base_lo:#x}/{ramfc_gp_base_hi:#x} \
                 USERD={ramfc_userd_lo:#x}/{ramfc_userd_hi:#x} SIG={ramfc_sig:#x} GP_PUT={ramfc_gp_put:#x}"
            );

            // Program PBDMA CTX registers directly (offsets mirror RAMFC layout)
            self.w(pb + pbdma::CTX_GP_BASE_LO, ramfc_gp_base_lo);
            self.w(pb + pbdma::CTX_GP_BASE_HI, ramfc_gp_base_hi);
            self.w(pb + pbdma::CTX_USERD_LO, ramfc_userd_lo);
            self.w(pb + pbdma::CTX_USERD_HI, ramfc_userd_hi);
            self.w(pb + pbdma::CTX_SIGNATURE, ramfc_sig);
            self.w(pb + pbdma::CTX_GP_PUT, 1);
            self.w(pb + pbdma::CTX_GP_FETCH, 0);

            // Enable channel + doorbell
            self.w(pccsr::channel(channel_id), pccsr::CHANNEL_ENABLE_SET);
            std::thread::sleep(std::time::Duration::from_millis(5));
            self.w(usermode::NOTIFY_CHANNEL_PENDING, channel_id);
            std::thread::sleep(std::time::Duration::from_millis(50));

            // Read results from correct PBDMA register offsets
            let f_userd = self.r(pb + pbdma::CTX_USERD_LO);
            let f_sig = self.r(pb + pbdma::CTX_SIGNATURE);
            let f_gpbase = self.r(pb + pbdma::CTX_GP_BASE_LO);
            let f_gp_put = self.r(pb + pbdma::CTX_GP_PUT);
            let f_gp_get = self.r(pb + pbdma::CTX_GP_FETCH);
            let f_ctrl = self.r(pccsr::channel(channel_id));
            let f_chsw = self.r(pfifo::CHSW_ERROR);
            let f_intr = self.r(pfifo::INTR);
            let f_status = (f_ctrl >> 24) & 0xF;
            let f_pbdma_intr = self.r(pbdma::intr(pbdma_id));
            let f_pbdma_status = self.r(pb + 0x118);

            let f_loaded =
                f_userd == (ramfc_userd_lo & 0xFFFF_FF00) || f_gpbase == ramfc_gp_base_lo;
            let f_fetching = f_gp_get != 0 || f_gp_put != 1;

            eprintln!(
                "║   F_direct: USERD={f_userd:#010x} SIG={f_sig:#010x} GP_BASE={f_gpbase:#010x}"
            );
            eprintln!(
                "║     GP_GET={f_gp_get:#x} GP_PUT={f_gp_put:#x} CTRL={f_ctrl:#x}(st={f_status}) \
                 CHSW={f_chsw:#x} INTR={f_intr:#x} PBDMA_INTR={f_pbdma_intr:#x} \
                 PBDMA_ST={f_pbdma_status:#x}"
            );

            ctx_evidence.push(("F_USERD".into(), f_userd));
            ctx_evidence.push(("F_SIG".into(), f_sig));
            ctx_evidence.push(("F_GP_BASE".into(), f_gpbase));
            ctx_evidence.push(("F_GP_GET".into(), f_gp_get));
            ctx_evidence.push(("F_GP_PUT".into(), f_gp_put));
            ctx_evidence.push(("F_CTRL".into(), f_ctrl));
            ctx_evidence.push(("F_PBDMA_INTR".into(), f_pbdma_intr));
            ctx_evidence.push(("F_PBDMA_ST".into(), f_pbdma_status));

            if f_loaded || f_fetching {
                gpu_can_read = true;
                eprintln!("║   ✓ Direct PBDMA programming loaded context!");
                if f_sig == 0x0000_FACE || f_gpbase == ramfc_gp_base_lo {
                    instance_accessible = true;
                }
            }
        }

        // Clean up
        self.w(pccsr::channel(channel_id), pccsr::CHANNEL_ENABLE_CLR);
        std::thread::sleep(std::time::Duration::from_millis(5));
        self.w(
            pccsr::channel(channel_id),
            pccsr::PBDMA_FAULTED_RESET | pccsr::ENG_FAULTED_RESET,
        );
        self.w(pccsr::inst(channel_id), 0);
        self.w(pfifo::INTR, 0xFFFF_FFFF);

        Ok(DmaCapability {
            engines,
            gpu_can_read_sysmem: gpu_can_read,
            gpu_can_write_sysmem: false,
            iommu_mapping_ok: iommu_ok,
            page_tables_ok: pt_ok,
            instance_block_accessible: instance_accessible,
            ctx_evidence,
        })
    }

    // ─── Layer 5: Channel Config ─────────────────────────────────────────

    fn probe_channel(&self, dma: &DmaCapability) -> Result<ChannelConfig, ProbeFailure> {
        eprintln!("╠══ L5: Channel — scheduler + doorbell + GP_GET ═════════╣");

        let engines = &dma.engines;
        let channel_id: u32 = 1;
        let pbdma_id = engines.gr_pbdma.unwrap_or(1);
        let pb = pbdma::BASE + pbdma_id * pbdma::STRIDE;

        // Use IOVAs in a separate range from L4 (0x10_xxxx) to avoid IOMMU conflicts
        // if L4's DmaBuffer drop didn't properly unmap. Page tables will identity-map
        // these IOVAs to the same physical addresses.
        let l5_gpfifo_iova: u64 = 0x10_1000;
        let l5_userd_iova: u64 = 0x10_2000;
        let l5_inst_iova: u64 = 0x10_3000;
        let l5_pd3_iova: u64 = 0x10_5000;
        let l5_pd2_iova: u64 = 0x10_6000;
        let l5_pd1_iova: u64 = 0x10_7000;
        let l5_pd0_iova: u64 = 0x10_8000;
        let l5_pt0_iova: u64 = 0x10_9000;
        let l5_runlist_iova: u64 = 0x10_4000;

        // Allocate fresh DMA buffers at L5-specific IOVAs
        let mut instance =
            DmaBuffer::new(self.container_fd, 4096, l5_inst_iova).map_err(|e| ProbeFailure {
                layer: "L5_CHANNEL",
                step: "alloc_instance",
                evidence: vec![],
                message: format!("DMA alloc instance: {e}"),
            })?;
        let mut gpfifo =
            DmaBuffer::new(self.container_fd, 4096, l5_gpfifo_iova).map_err(|e| ProbeFailure {
                layer: "L5_CHANNEL",
                step: "alloc_gpfifo",
                evidence: vec![],
                message: format!("DMA alloc gpfifo: {e}"),
            })?;
        let mut userd =
            DmaBuffer::new(self.container_fd, 4096, l5_userd_iova).map_err(|e| ProbeFailure {
                layer: "L5_CHANNEL",
                step: "alloc_userd",
                evidence: vec![],
                message: format!("DMA alloc userd: {e}"),
            })?;

        // Page table hierarchy — using L5 IOVAs
        let mut pd3 = DmaBuffer::new(self.container_fd, 4096, l5_pd3_iova).ok();
        let mut pd2 = DmaBuffer::new(self.container_fd, 4096, l5_pd2_iova).ok();
        let mut pd1 = DmaBuffer::new(self.container_fd, 4096, l5_pd1_iova).ok();
        let mut pd0 = DmaBuffer::new(self.container_fd, 4096, l5_pd0_iova).ok();
        let mut pt0 = DmaBuffer::new(self.container_fd, 4096, l5_pt0_iova).ok();

        let pt_ok =
            pd3.is_some() && pd2.is_some() && pd1.is_some() && pd0.is_some() && pt0.is_some();

        if pt_ok {
            crate::vfio::channel::page_tables::populate_page_tables_custom(
                pd3.as_mut().unwrap().as_mut_slice(),
                pd2.as_mut().unwrap().as_mut_slice(),
                pd1.as_mut().unwrap().as_mut_slice(),
                pd0.as_mut().unwrap().as_mut_slice(),
                pt0.as_mut().unwrap().as_mut_slice(),
                l5_pd2_iova,
                l5_pd1_iova,
                l5_pd0_iova,
                l5_pt0_iova,
            );
        }

        crate::vfio::channel::page_tables::populate_instance_block_custom(
            instance.as_mut_slice(),
            l5_gpfifo_iova,
            8,
            l5_userd_iova,
            channel_id,
            l5_pd3_iova,
        );

        // Write a NOP GPFIFO entry pointing to the GPFIFO buffer itself (self-referencing NOP)
        // GPFIFO entry format: [0] = (addr_lo & ~3) | control, [1] = (addr_hi) | (len << 10)
        // Simplest: zero-length NOP entry
        let gp = gpfifo.as_mut_slice();
        gp[0..4].copy_from_slice(&0u32.to_le_bytes()); // DW0: addr_lo = 0 (doesn't matter for empty)
        gp[4..8].copy_from_slice(&0u32.to_le_bytes()); // DW1: len=0

        // Write GP_PUT sentinel in USERD
        let us = userd.as_mut_slice();
        // ramuserd::GP_PUT (offset 35*4 = 0x8C)
        us[ramuserd::GP_PUT..ramuserd::GP_PUT + 4].copy_from_slice(&1u32.to_le_bytes());
        // Clear GP_GET
        us[ramuserd::GP_GET..ramuserd::GP_GET + 4].copy_from_slice(&0u32.to_le_bytes());

        // Flush CPU caches
        #[cfg(target_arch = "x86_64")]
        {
            for buf in [instance.as_slice(), gpfifo.as_slice(), userd.as_slice()] {
                let ptr = buf.as_ptr();
                let len = buf.len();
                let mut addr = ptr as usize & !63;
                let end = (ptr as usize + len + 63) & !63;
                while addr < end {
                    unsafe { std::arch::x86_64::_mm_clflush(addr as *const u8) };
                    addr += 64;
                }
            }
            if let Some(ref p) = pd3 {
                let mut a = p.as_slice().as_ptr() as usize & !63;
                while a < p.as_slice().as_ptr() as usize + p.as_slice().len() {
                    unsafe { std::arch::x86_64::_mm_clflush(a as *const u8) };
                    a += 64;
                }
            }
            unsafe { std::arch::x86_64::_mm_mfence() };
        }

        // TLB flush
        self.w(0x100CBC, 1);
        std::thread::sleep(std::time::Duration::from_millis(5));

        // Clean up any stale channel state
        self.w(pccsr::channel(channel_id), pccsr::CHANNEL_ENABLE_CLR);
        std::thread::sleep(std::time::Duration::from_millis(5));
        self.w(
            pccsr::channel(channel_id),
            pccsr::PBDMA_FAULTED_RESET | pccsr::ENG_FAULTED_RESET,
        );
        self.w(pccsr::inst(channel_id), 0);
        self.w(pfifo::INTR, 0xFFFF_FFFF);
        self.w(pbdma::intr(pbdma_id), 0xFFFF_FFFF);
        std::thread::sleep(std::time::Duration::from_millis(5));

        // Enable PFIFO interrupts
        self.w(pfifo::INTR_EN, 0x7FFF_FFFF);

        let mut working_target = 0u32;
        let mut scheduling_method = SchedulingMethod::None;
        let mut inst_bind_needed = true;
        let mut runlist_ack = false;
        let mut gp_get_advanced = false;
        let mut userd_written = false;

        // ── Strategy A: Full scheduler sequence (sysmem instance) ─────────
        // Order: enable interrupts → INST_BIND → enable channel → submit runlist → doorbell
        struct L5Attempt {
            label: &'static str,
            inst_target: u32,
            inst_addr_shr12: u32,
            submit_runlist: bool,
        }

        let inst_sysmem = (l5_inst_iova >> 12) as u32;

        let attempts = [
            L5Attempt {
                label: "sched_sysmem_coh",
                inst_target: TARGET_SYS_MEM_COHERENT,
                inst_addr_shr12: inst_sysmem,
                submit_runlist: true,
            },
            L5Attempt {
                label: "sched_sysmem_ncoh",
                inst_target: TARGET_SYS_MEM_NONCOHERENT,
                inst_addr_shr12: inst_sysmem,
                submit_runlist: true,
            },
        ];

        for attempt in &attempts {
            // Reset
            self.w(pccsr::channel(channel_id), pccsr::CHANNEL_ENABLE_CLR);
            std::thread::sleep(std::time::Duration::from_millis(5));
            self.w(
                pccsr::channel(channel_id),
                pccsr::PBDMA_FAULTED_RESET | pccsr::ENG_FAULTED_RESET,
            );
            self.w(pccsr::inst(channel_id), 0);
            self.w(pfifo::INTR, 0xFFFF_FFFF);
            self.w(pbdma::intr(pbdma_id), 0xFFFF_FFFF);
            std::thread::sleep(std::time::Duration::from_millis(5));

            // Write sentinels
            self.w(pb + pbdma::CTX_SIGNATURE, 0xBEEF_0010);
            self.w(pb + pbdma::CTX_GP_BASE_LO, 0xBEEF_0048);

            // Clear USERD GP_GET to detect writeback
            let us = userd.as_mut_slice();
            us[ramuserd::GP_GET..ramuserd::GP_GET + 4].copy_from_slice(&0xDEADu32.to_le_bytes());
            #[cfg(target_arch = "x86_64")]
            {
                let ptr = us.as_ptr() as usize + ramuserd::GP_GET;
                unsafe {
                    std::arch::x86_64::_mm_clflush(ptr as *const u8);
                    std::arch::x86_64::_mm_mfence();
                }
            }

            // INST_BIND
            let inst_val =
                attempt.inst_addr_shr12 | (attempt.inst_target << 28) | pccsr::INST_BIND_TRUE;
            self.w(pccsr::inst(channel_id), inst_val);
            std::thread::sleep(std::time::Duration::from_millis(5));

            // Enable channel
            self.w(pccsr::channel(channel_id), pccsr::CHANNEL_ENABLE_SET);
            std::thread::sleep(std::time::Duration::from_millis(5));

            if attempt.submit_runlist {
                // Build runlist in system memory DMA buffer
                let mut runlist = DmaBuffer::new(self.container_fd, 4096, l5_runlist_iova).ok();
                if let Some(ref mut rl) = runlist {
                    let rl_data = rl.as_mut_slice();
                    // GV100 runlist: TSG header (16B) + channel entry (8B)
                    let tsg_id: u32 = 0;
                    // DW0: (tsg_id << 3) | 4 (type=TSG)
                    rl_data[0..4].copy_from_slice(&((tsg_id << 3) | 4).to_le_bytes());
                    // DW1: timeslice
                    rl_data[4..8].copy_from_slice(&0x8000_0000u32.to_le_bytes());
                    // DW2: channel count
                    rl_data[8..12].copy_from_slice(&1u32.to_le_bytes());
                    // DW3: 0
                    rl_data[12..16].copy_from_slice(&0u32.to_le_bytes());
                    // Channel DW0: channel_id
                    rl_data[16..20].copy_from_slice(&channel_id.to_le_bytes());
                    // Channel DW1: 0
                    rl_data[20..24].copy_from_slice(&0u32.to_le_bytes());

                    #[cfg(target_arch = "x86_64")]
                    {
                        let ptr = rl_data.as_ptr();
                        let mut addr = ptr as usize & !63;
                        while addr < ptr as usize + 64 {
                            unsafe { std::arch::x86_64::_mm_clflush(addr as *const u8) };
                            addr += 64;
                        }
                        unsafe { std::arch::x86_64::_mm_mfence() };
                    }

                    let gr_rl = engines.gr_runlist.unwrap_or(1) as u32;
                    self.w(
                        pfifo::runlist_base(gr_rl),
                        pfifo::gv100_runlist_base_value(l5_runlist_iova),
                    );
                    self.w(
                        pfifo::runlist_submit(gr_rl),
                        pfifo::gv100_runlist_submit_value(l5_runlist_iova, 2),
                    );
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
            }

            // Ring the doorbell
            self.w(usermode::NOTIFY_CHANNEL_PENDING, channel_id);
            std::thread::sleep(std::time::Duration::from_millis(100));

            // Read back channel status
            let ctrl = self.r(pccsr::channel(channel_id));
            let status = pccsr::status(ctrl);
            let status_name = pccsr::status_name(ctrl);
            let sig = self.r(pb + pbdma::CTX_SIGNATURE);
            let gpbase = self.r(pb + pbdma::CTX_GP_BASE_LO);
            let gp_put = self.r(pb + pbdma::CTX_GP_PUT);
            let gp_get = self.r(pb + pbdma::CTX_GP_FETCH);
            let chsw = self.r(pfifo::CHSW_ERROR);
            let intr = self.r(pfifo::INTR);
            let pbdma_intr_val = self.r(pbdma::intr(pbdma_id));

            // Check USERD GP_GET for writeback
            #[cfg(target_arch = "x86_64")]
            {
                let ptr = userd.as_slice().as_ptr() as usize + ramuserd::GP_GET;
                unsafe {
                    std::arch::x86_64::_mm_clflush(ptr as *const u8);
                    std::arch::x86_64::_mm_mfence();
                }
            }
            let userd_gp_get = u32::from_le_bytes(
                userd.as_slice()[ramuserd::GP_GET..ramuserd::GP_GET + 4]
                    .try_into()
                    .unwrap(),
            );

            let context_loaded = sig != 0xBEEF_0010 && gpbase != 0xBEEF_0048;
            let sig_correct = sig == 0x0000_FACE;

            eprintln!(
                "║ L5 {}: st={status}({status_name}) SIG={sig:#x} GP_BASE={gpbase:#x}",
                attempt.label
            );
            eprintln!(
                "║   GP_GET={gp_get:#x} GP_PUT={gp_put:#x} CHSW={chsw:#x} INTR={intr:#x} \
                 PBDMA_INTR={pbdma_intr_val:#x} USERD_GP_GET={userd_gp_get:#x}"
            );

            if context_loaded && sig_correct {
                eprintln!("║   ✓ Context loaded via scheduler!");
                working_target = attempt.inst_target;
                scheduling_method = SchedulingMethod::HardwareScheduler;

                if gp_get != 0 {
                    gp_get_advanced = true;
                    eprintln!("║   ✓ GP_GET advanced to {gp_get}!");
                }

                if userd_gp_get != 0xDEAD && userd_gp_get != 0 {
                    userd_written = true;
                    eprintln!("║   ✓ USERD GP_GET written back: {userd_gp_get:#x}!");
                }

                runlist_ack = (intr & pfifo::INTR_RL_COMPLETE) != 0;
                break;
            } else if context_loaded {
                eprintln!("║   ~ Context loaded but SIG={sig:#x} (expected 0xFACE)");
                working_target = attempt.inst_target;
                scheduling_method = SchedulingMethod::HardwareScheduler;
                break;
            } else {
                eprintln!(
                    "║   ✗ Context not loaded (sentinels={} dead={})",
                    (sig >> 16) == 0xBEEF,
                    (sig >> 16) == 0xDEAD
                );
            }
        }

        // Also check MMU fault registers for clues
        let fault_status = self.r(mmu::FAULT_STATUS);
        if fault_status != 0 {
            let fault_addr_lo = self.r(mmu::FAULT_ADDR_LO);
            let fault_addr_hi = self.r(mmu::FAULT_ADDR_HI);
            let fault_inst_lo = self.r(mmu::FAULT_INST_LO);
            eprintln!(
                "║ L5 MMU_FAULT: status={fault_status:#x} addr={fault_addr_hi:#x}_{fault_addr_lo:#x} \
                 inst={fault_inst_lo:#x}"
            );
        }

        // Clean up
        self.w(pccsr::channel(channel_id), pccsr::CHANNEL_ENABLE_CLR);
        std::thread::sleep(std::time::Duration::from_millis(5));
        self.w(
            pccsr::channel(channel_id),
            pccsr::PBDMA_FAULTED_RESET | pccsr::ENG_FAULTED_RESET,
        );
        self.w(pccsr::inst(channel_id), 0);
        self.w(pfifo::INTR, 0xFFFF_FFFF);

        Ok(ChannelConfig {
            dma: dma.clone(),
            working_inst_target: working_target,
            working_userd_target: PBDMA_TARGET_SYS_MEM_COHERENT,
            instance_requires_vram: working_target == 0,
            userd_requires_vram: false,
            inst_bind_needed,
            runlist_ack_works: runlist_ack,
            scheduling_method,
        })
    }

    // ─── Layer 6: Dispatch Capability ────────────────────────────────────

    fn probe_dispatch(&self, ch: &ChannelConfig) -> Result<DispatchCapability, ProbeFailure> {
        eprintln!("╠══ L6: Dispatch — GPFIFO consumption + NOP execution ══╣");

        let mut blockers = Vec::new();

        if ch.scheduling_method == SchedulingMethod::None {
            blockers.push("L5 failed: no working scheduling method found".into());
        }

        Ok(DispatchCapability {
            channel: ch.clone(),
            gpfifo_consumed: ch.scheduling_method != SchedulingMethod::None,
            nop_executed: false,
            dispatch_ready: false,
            blockers,
        })
    }
}
