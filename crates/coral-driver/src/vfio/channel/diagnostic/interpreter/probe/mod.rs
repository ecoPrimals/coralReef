// SPDX-License-Identifier: AGPL-3.0-only
#![expect(missing_docs, reason = "probe interpreter; full docs planned")]
//! Probe interpreter — chains layer probes and collects a full report.

mod channel;
mod dispatch;
mod dma;
mod domain;
mod report;

use std::os::fd::OwnedFd;
use std::sync::Arc;
use std::time::Instant;

use crate::vfio::channel::glowplug::GlowPlug;
use crate::vfio::device::MappedBar;

use super::layers::*;
use super::memory_probe;

use channel::probe_channel;
use dispatch::probe_dispatch;
use dma::probe_dma;
use domain::{probe_bar, probe_engines, probe_identity, probe_power};

pub use report::ProbeReport;

/// The probe interpreter — runs layered discovery on a VFIO GPU.
pub struct ProbeInterpreter<'a> {
    bar0: &'a MappedBar,
    container: Arc<OwnedFd>,
}

impl<'a> ProbeInterpreter<'a> {
    pub fn new(bar0: &'a MappedBar, container: Arc<OwnedFd>) -> Self {
        Self { bar0, container }
    }

    fn r(&self, reg: usize) -> u32 {
        self.bar0.read_u32(reg).unwrap_or(0xDEAD_DEAD)
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
        let bar = probe_bar(self.bar0);
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
        let identity = probe_identity(self.bar0, bar);
        report.identity = Some(identity.clone());

        // Layer 2: Power
        match probe_power(self.bar0, identity) {
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

                let power = if matches!(power.method, PowerMethod::GlowPlug | PowerMethod::Failed) {
                    tracing::debug!(
                        method = ?power.method,
                        "L2.5: invoking GlowPlug.full_init"
                    );
                    let gp = GlowPlug::new(self.bar0, Arc::clone(&self.container));
                    let warm_result = gp.full_init();
                    for msg in &warm_result.log {
                        tracing::debug!(%msg, "GlowPlug");
                    }
                    if let Some(mem) = &warm_result.memory {
                        mem.print_summary();
                    }

                    let pmc_final = self.r(crate::vfio::channel::registers::pmc::ENABLE);
                    let pfifo_final = self.r(crate::vfio::channel::registers::pfifo::ENABLE);
                    let pbdma_after = self.r(crate::vfio::channel::registers::pfifo::PBDMA_MAP);
                    let ptimer = {
                        let t0 = self.r(0x009400);
                        std::thread::sleep(std::time::Duration::from_millis(5));
                        let t1 = self.r(0x009400);
                        t0 != t1 && t0 != 0xDEAD_DEAD && t0 != 0xBAD0_DA00
                    };
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
                match probe_engines(self.bar0, Arc::clone(&self.container), power) {
                    Ok(engines) => {
                        report.engines = Some(engines.clone());

                        let mem_topo = memory_probe::discover_memory_topology(
                            self.bar0,
                            Arc::clone(&self.container),
                        );
                        mem_topo.print_summary();
                        report.memory = Some(mem_topo);

                        match probe_dma(self.bar0, Arc::clone(&self.container), engines.clone()) {
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

                                if dma.instance_block_accessible {
                                    match probe_channel(
                                        self.bar0,
                                        Arc::clone(&self.container),
                                        &dma,
                                    ) {
                                        Ok(ch) => {
                                            report.channel = Some(ch.clone());
                                            match probe_dispatch(&ch) {
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
}
