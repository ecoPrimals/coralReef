// SPDX-License-Identifier: AGPL-3.0-only
#![expect(missing_docs, reason = "probe report types; full docs planned")]
//! Probe report — result container and analysis.

use std::fmt::Write as FmtWrite;

use crate::vfio::memory::MemoryTopology;

use super::super::layers::*;

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

    /// Emit a human-readable probe report via `tracing`.
    pub fn print_summary(&self) {
        let mut s = String::new();
        writeln!(
            &mut s,
            "╔══ INTERPRETER PROBE REPORT ════════════════════════════════╗"
        )
        .expect("writing to String is infallible");
        writeln!(
            &mut s,
            "║ Depth: Layer {}/8 in {}ms",
            self.depth(),
            self.elapsed_ms
        )
        .expect("writing to String is infallible");

        if let Some(bar) = &self.bar {
            writeln!(
                &mut s,
                "║ L0 BAR:     read={} write={} d3hot={} BOOT0={:#010x}",
                bar.bar0_readable, bar.bar0_writable, bar.in_d3hot, bar.boot0_raw
            )
            .expect("writing to String is infallible");
        }
        if let Some(id) = &self.identity {
            writeln!(
                &mut s,
                "║ L1 ID:      {} impl={} rev={} BOOT0={:#010x}",
                id.architecture, id.implementation, id.revision, id.boot0
            )
            .expect("writing to String is infallible");
        }
        if let Some(pwr) = &self.power {
            writeln!(
                &mut s,
                "║ L2 POWER:   {:?} pfifo={} ptimer={} PMC={:#010x}",
                pwr.method, pwr.pfifo_enabled, pwr.ptimer_ticking, pwr.pmc_enable_final
            )
            .expect("writing to String is infallible");
        }
        if let Some(eng) = &self.engines {
            writeln!(
                &mut s,
                "║ L3 ENGINES: pbdma_map={:#010x} gr_rl={:?} gr_pbdma={:?}",
                eng.pbdma_map, eng.gr_runlist, eng.gr_pbdma
            )
            .expect("writing to String is infallible");
            writeln!(
                &mut s,
                "║             BAR1={:#010x} BAR2={:#010x} bar2_setup={}",
                eng.bar1_block, eng.bar2_block, eng.bar2_setup_needed
            )
            .expect("writing to String is infallible");
        }
        if let Some(mem) = &self.memory {
            let pramin_ok = mem.pramin_works();
            let dma_ok = mem.dma_works();
            let working = mem.paths.iter().filter(|p| p.status.is_working()).count();
            let total = mem.paths.len();
            writeln!(
                &mut s,
                "║ L3.5 MEM:   vram={} pramin={pramin_ok} dma={dma_ok} bar2={} paths={working}/{total}",
                mem.vram_accessible, mem.bar2_configured
            )
            .expect("writing to String is infallible");
        }
        if let Some(dma) = &self.dma {
            writeln!(
                &mut s,
                "║ L4 DMA:     read={} write={} iommu={} pt={} inst={}",
                dma.gpu_can_read_sysmem,
                dma.gpu_can_write_sysmem,
                dma.iommu_mapping_ok,
                dma.page_tables_ok,
                dma.instance_block_accessible
            )
            .expect("writing to String is infallible");
            if !dma.ctx_evidence.is_empty() {
                write!(&mut s, "║             CTX:").expect("writing to String is infallible");
                for (name, val) in &dma.ctx_evidence {
                    write!(&mut s, " {name}={val:#010x}").expect("writing to String is infallible");
                }
                writeln!(&mut s).expect("writing to String is infallible");
            }
        }
        if let Some(ch) = &self.channel {
            writeln!(
                &mut s,
                "║ L5 CHANNEL: inst_tgt={} userd_tgt={} vram_inst={} method={:?}",
                ch.working_inst_target,
                ch.working_userd_target,
                ch.instance_requires_vram,
                ch.scheduling_method
            )
            .expect("writing to String is infallible");
        }
        if let Some(disp) = &self.dispatch {
            writeln!(
                &mut s,
                "║ L6 DISPATCH: consumed={} nop={} ready={}",
                disp.gpfifo_consumed, disp.nop_executed, disp.dispatch_ready
            )
            .expect("writing to String is infallible");
            for b in &disp.blockers {
                writeln!(&mut s, "║   BLOCKER: {b}").expect("writing to String is infallible");
            }
        }

        if !self.failures.is_empty() {
            writeln!(
                &mut s,
                "╠══ FAILURES ═══════════════════════════════════════════════╣"
            )
            .expect("writing to String is infallible");
            for f in &self.failures {
                writeln!(&mut s, "║ {f}").expect("writing to String is infallible");
            }
        }
        writeln!(
            &mut s,
            "╚═══════════════════════════════════════════════════════════╝"
        )
        .expect("writing to String is infallible");
        tracing::info!(summary = %s, "interpreter probe report");
    }
}
