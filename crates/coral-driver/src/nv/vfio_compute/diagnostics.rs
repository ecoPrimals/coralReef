// SPDX-License-Identifier: AGPL-3.0-only
//! Layer 7 diagnostic capture — FECS falcon, engine topology, PBDMA state.
//!
//! Captures comprehensive GPU state to diagnose why GPFIFO completions
//! stall on cold VFIO (FECS firmware not loaded → GR engine dead →
//! scheduler holds channel in PENDING → PBDMA never loads context).

use std::fmt;
use std::time::Instant;

use crate::vfio::channel::registers::{falcon, pbdma, pccsr, pfifo};
use crate::vfio::device::MappedBar;

/// State snapshot of a single Falcon microcontroller.
#[derive(Debug, Clone)]
pub struct FalconState {
    pub name: &'static str,
    pub base: usize,
    pub cpuctl: u32,
    pub bootvec: u32,
    pub hwcfg: u32,
    pub dmactl: u32,
    pub irqstat: u32,
    pub mailbox0: u32,
    pub mailbox1: u32,
    pub os: u32,
    pub curctx: u32,
    pub nxtctx: u32,
    pub debug1: u32,
}

impl FalconState {
    /// Capture falcon state from BAR0.
    pub fn capture(bar0: &MappedBar, name: &'static str, base: usize) -> Self {
        let r = |off: usize| bar0.read_u32(base + off).unwrap_or(0xDEAD_DEAD);
        Self {
            name,
            base,
            cpuctl: r(falcon::CPUCTL),
            bootvec: r(falcon::BOOTVEC),
            hwcfg: r(falcon::HWCFG),
            dmactl: r(falcon::DMACTL),
            irqstat: r(falcon::IRQSTAT),
            mailbox0: r(falcon::MAILBOX0),
            mailbox1: r(falcon::MAILBOX1),
            os: r(falcon::OS),
            curctx: r(falcon::CURCTX),
            nxtctx: r(falcon::NXTCTX),
            debug1: r(falcon::DEBUG1),
        }
    }

    pub fn is_in_reset(&self) -> bool {
        self.cpuctl & falcon::CPUCTL_HRESET != 0
    }

    pub fn is_halted(&self) -> bool {
        self.cpuctl & falcon::CPUCTL_HALTED != 0 || self.cpuctl == 0xDEAD_DEAD
    }

    pub fn requires_signed_firmware(&self) -> bool {
        self.hwcfg & falcon::HWCFG_SECURITY_MODE != 0
    }

    pub fn imem_size(&self) -> u32 {
        falcon::imem_size_bytes(self.hwcfg)
    }

    pub fn dmem_size(&self) -> u32 {
        falcon::dmem_size_bytes(self.hwcfg)
    }

    fn state_label(&self) -> &'static str {
        if self.cpuctl == 0xDEAD_DEAD {
            "UNREACHABLE"
        } else if self.is_in_reset() && self.is_halted() {
            "HRESET+HALTED"
        } else if self.is_in_reset() {
            "HRESET"
        } else if self.is_halted() {
            "HALTED"
        } else if self.mailbox0 != 0 || self.mailbox1 != 0 {
            "RUNNING (mailbox active)"
        } else {
            "IDLE (no mailbox)"
        }
    }
}

impl fmt::Display for FalconState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "  {} @ {:#010x}: {}", self.name, self.base, self.state_label())?;
        writeln!(f, "    cpuctl={:#010x} bootvec={:#010x} hwcfg={:#010x}", self.cpuctl, self.bootvec, self.hwcfg)?;
        writeln!(f, "    imem={}B dmem={}B secure={}", self.imem_size(), self.dmem_size(), self.requires_signed_firmware())?;
        writeln!(f, "    irqstat={:#010x} dmactl={:#010x} debug1={:#010x}", self.irqstat, self.dmactl, self.debug1)?;
        writeln!(f, "    mailbox0={:#010x} mailbox1={:#010x}", self.mailbox0, self.mailbox1)?;
        write!(f, "    os={:#010x} curctx={:#010x} nxtctx={:#010x}", self.os, self.curctx, self.nxtctx)
    }
}

/// Decoded engine topology entry from `ENGN_TABLE` (BAR0 + 0x22700).
#[derive(Debug, Clone)]
pub struct EngineEntry {
    pub index: u32,
    pub raw: u32,
    pub engine_type: u32,
    pub runlist: u32,
    pub is_final: bool,
}

impl fmt::Display for EngineEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let type_name = match self.engine_type {
            0 => "GR",
            1 => "CE",
            2 => "NVDEC",
            3 => "NVENC",
            4 => "SEC2",
            8 => "NVJPG",
            _ => "UNKNOWN",
        };
        write!(
            f, "  ENGN[{:2}]: {:#010x} type={} ({}) runlist={}{}",
            self.index, self.raw, self.engine_type, type_name, self.runlist,
            if self.is_final { " (FINAL)" } else { "" }
        )
    }
}

/// Point-in-time snapshot of a PBDMA's operational registers.
#[derive(Debug, Clone)]
pub struct PbdmaSnapshot {
    pub pbdma_id: usize,
    pub timestamp_us: u64,
    pub gp_base_lo: u32,
    pub gp_base_hi: u32,
    pub gp_put: u32,
    pub gp_fetch: u32,
    pub gp_state: u32,
    pub userd_lo: u32,
    pub userd_hi: u32,
    pub channel_state: u32,
    pub method0: u32,
    pub data0: u32,
    pub intr: u32,
    pub channel_info: u32,
    pub signature: u32,
}

impl PbdmaSnapshot {
    /// Capture PBDMA state from BAR0.
    pub fn capture(bar0: &MappedBar, pbdma_id: usize, start: Instant) -> Self {
        let base = pbdma::BASE + pbdma_id * pbdma::STRIDE;
        let r = |off: usize| bar0.read_u32(base + off).unwrap_or(0xDEAD_DEAD);
        Self {
            pbdma_id,
            timestamp_us: start.elapsed().as_micros() as u64,
            gp_base_lo: r(pbdma::GP_BASE_LO),
            gp_base_hi: r(pbdma::GP_BASE_HI),
            gp_put: r(pbdma::GP_PUT),
            gp_fetch: r(pbdma::GP_FETCH),
            gp_state: r(pbdma::GP_STATE),
            userd_lo: r(pbdma::USERD_LO),
            userd_hi: r(pbdma::USERD_HI),
            channel_state: r(pbdma::CHANNEL_STATE),
            method0: r(pbdma::METHOD0),
            data0: r(pbdma::DATA0),
            intr: r(pbdma::intr(pbdma_id)),
            channel_info: r(pbdma::CHANNEL_INFO),
            signature: r(pbdma::SIGNATURE),
        }
    }

    pub fn has_channel_loaded(&self) -> bool {
        self.gp_base_lo != 0 || self.gp_base_hi != 0
    }
}

impl fmt::Display for PbdmaSnapshot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "  PBDMA[{}] @ t={}us:", self.pbdma_id, self.timestamp_us)?;
        writeln!(f, "    gp_base={:#010x}:{:#010x} gp_put={:#010x} gp_fetch={:#010x} gp_state={:#010x}",
            self.gp_base_hi, self.gp_base_lo, self.gp_put, self.gp_fetch, self.gp_state)?;
        writeln!(f, "    userd={:#010x}:{:#010x} chan_state={:#010x} chan_info={:#010x}",
            self.userd_hi, self.userd_lo, self.channel_state, self.channel_info)?;
        write!(f, "    method0={:#010x} data0={:#010x} intr={:#010x} sig={:#010x}",
            self.method0, self.data0, self.intr, self.signature)
    }
}

/// PCCSR (per-channel control/status) snapshot.
#[derive(Debug, Clone)]
pub struct PccsrSnapshot {
    pub channel_id: u32,
    pub inst: u32,
    pub channel: u32,
}

impl PccsrSnapshot {
    /// Capture PCCSR state for a channel.
    pub fn capture(bar0: &MappedBar, channel_id: u32) -> Self {
        Self {
            channel_id,
            inst: bar0.read_u32(pccsr::inst(channel_id)).unwrap_or(0xDEAD_DEAD),
            channel: bar0.read_u32(pccsr::channel(channel_id)).unwrap_or(0xDEAD_DEAD),
        }
    }

    pub fn status(&self) -> u32 {
        pccsr::status(self.channel)
    }

    pub fn status_name(&self) -> &'static str {
        pccsr::status_name(self.channel)
    }

    pub fn is_enabled(&self) -> bool {
        self.channel & 1 != 0
    }

    pub fn is_busy(&self) -> bool {
        self.channel & (1 << 28) != 0
    }

    pub fn pbdma_faulted(&self) -> bool {
        self.channel & (1 << 22) != 0
    }

    pub fn eng_faulted(&self) -> bool {
        self.channel & (1 << 23) != 0
    }
}

impl fmt::Display for PccsrSnapshot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f, "  PCCSR[{}]: inst={:#010x} chan={:#010x} status={} en={} busy={} pbdma_fault={} eng_fault={}",
            self.channel_id, self.inst, self.channel,
            self.status_name(), self.is_enabled(), self.is_busy(),
            self.pbdma_faulted(), self.eng_faulted()
        )
    }
}

/// PFIFO scheduler state snapshot.
#[derive(Debug, Clone)]
pub struct PfifoSnapshot {
    pub intr: u32,
    pub intr_en: u32,
    pub sched_disable: u32,
    pub chsw_error: u32,
    pub pbdma_map: u32,
}

impl fmt::Display for PfifoSnapshot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let rl_complete = self.intr & pfifo::INTR_RL_COMPLETE != 0;
        let chsw_err = self.intr & pfifo::INTR_CHSW_ERROR != 0;
        let pbdma_agg = self.intr & pfifo::INTR_PBDMA != 0;
        writeln!(f, "  PFIFO:")?;
        writeln!(f, "    intr={:#010x} (rl_complete={} chsw_err={} pbdma_agg={})",
            self.intr, rl_complete, chsw_err, pbdma_agg)?;
        writeln!(f, "    intr_en={:#010x} sched_disable={:#010x} chsw_error={:#010x}",
            self.intr_en, self.sched_disable, self.chsw_error)?;
        write!(f, "    pbdma_map={:#010x} (active PBDMAs: {:?})",
            self.pbdma_map, active_pbdma_ids(self.pbdma_map))
    }
}

/// Full Layer 7 diagnostic capture.
#[derive(Debug, Clone)]
pub struct Layer7Diagnostics {
    pub label: String,
    pub fecs: FalconState,
    pub gpccs: FalconState,
    pub pmu: FalconState,
    pub sec2: FalconState,
    pub engine_topology: Vec<EngineEntry>,
    pub engine_status: Vec<(u32, u32)>,
    pub pccsr: PccsrSnapshot,
    pub pfifo: PfifoSnapshot,
    pub pbdma_snapshots: Vec<PbdmaSnapshot>,
    pub pmc_enable: u32,
    pub pgraph_status: u32,
}

impl Layer7Diagnostics {
    /// Capture full Layer 7 diagnostic state.
    pub fn capture(bar0: &MappedBar, label: &str, channel_id: u32) -> Self {
        let r = |off: usize| bar0.read_u32(off).unwrap_or(0xDEAD_DEAD);
        let start = Instant::now();

        let fecs = FalconState::capture(bar0, "FECS", falcon::FECS_BASE);
        let gpccs = FalconState::capture(bar0, "GPCCS", falcon::GPCCS_BASE);
        let pmu = FalconState::capture(bar0, "PMU", falcon::PMU_BASE);
        let sec2 = FalconState::capture(bar0, "SEC2", falcon::SEC2_BASE);

        let engine_topology = parse_engine_topology(bar0);

        let mut engine_status = Vec::new();
        for idx in 0..8_u32 {
            let status = r(pfifo::ENGN_STATUS + (idx as usize) * 4);
            if status != 0 {
                engine_status.push((idx, status));
            }
        }

        let pccsr_snap = PccsrSnapshot::capture(bar0, channel_id);

        let pfifo_snap = PfifoSnapshot {
            intr: r(pfifo::INTR),
            intr_en: r(pfifo::INTR_EN),
            sched_disable: r(pfifo::SCHED_DISABLE),
            chsw_error: r(pfifo::CHSW_ERROR),
            pbdma_map: r(pfifo::PBDMA_MAP),
        };

        let gr_runlist_pbdmas = find_pbdmas_for_runlist(pfifo_snap.pbdma_map, bar0, 1);
        let pbdma_snapshots: Vec<PbdmaSnapshot> = gr_runlist_pbdmas
            .iter()
            .map(|&id| PbdmaSnapshot::capture(bar0, id, start))
            .collect();

        Self {
            label: label.to_string(),
            fecs,
            gpccs,
            pmu,
            sec2,
            engine_topology,
            engine_status,
            pccsr: pccsr_snap,
            pfifo: pfifo_snap,
            pbdma_snapshots,
            pmc_enable: r(0x200),
            pgraph_status: r(0x40_0700),
        }
    }
}

impl fmt::Display for Layer7Diagnostics {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "╔══ Layer 7 Diagnostics: {} ══════════════════════════════╗", self.label)?;
        writeln!(f, "  pmc_enable={:#010x} pgraph_status={:#010x}", self.pmc_enable, self.pgraph_status)?;
        let gr_en = self.pmc_enable & (1 << 12) != 0;
        writeln!(f, "  GR engine enabled in PMC: {gr_en}")?;
        writeln!(f)?;
        writeln!(f, "── Falcon States ──")?;
        writeln!(f, "{}", self.fecs)?;
        writeln!(f, "{}", self.gpccs)?;
        writeln!(f, "{}", self.pmu)?;
        writeln!(f, "{}", self.sec2)?;
        writeln!(f)?;
        writeln!(f, "── Engine Topology (ENGN_TABLE) ──")?;
        for e in &self.engine_topology {
            writeln!(f, "{e}")?;
        }
        if !self.engine_status.is_empty() {
            writeln!(f, "── Engine Status (ENGN_STATUS) ──")?;
            for &(idx, status) in &self.engine_status {
                let rl = (status >> 12) & 0xF;
                writeln!(f, "  ENGN[{idx}]: {status:#010x} runlist_bits={rl}")?;
            }
        }
        writeln!(f)?;
        writeln!(f, "── PCCSR Channel ──")?;
        writeln!(f, "{}", self.pccsr)?;
        writeln!(f)?;
        writeln!(f, "{}", self.pfifo)?;
        writeln!(f)?;
        writeln!(f, "── PBDMA Snapshots (GR runlist) ──")?;
        for snap in &self.pbdma_snapshots {
            writeln!(f, "{snap}")?;
        }
        write!(f, "╚═══════════════════════════════════════════════════════════╝")
    }
}

/// Parse engine topology table from BAR0.
pub fn parse_engine_topology(bar0: &MappedBar) -> Vec<EngineEntry> {
    let r = |off: usize| bar0.read_u32(off).unwrap_or(0);
    let mut entries = Vec::new();
    let mut cur_type: u32 = 0xFFFF;
    let mut cur_rl: u32 = 0xFFFF;

    for i in 0..32_u32 {
        let data = r(pfifo::ENGN_TABLE + (i as usize) * 4);
        if data == 0 {
            break;
        }
        let kind = data & 3;
        match kind {
            1 => cur_type = (data >> 2) & 0x3F,
            3 => cur_rl = (data >> 11) & 0x1F,
            _ => {}
        }
        let is_final = data & (1 << 31) != 0;
        if is_final {
            entries.push(EngineEntry {
                index: i,
                raw: data,
                engine_type: cur_type,
                runlist: cur_rl,
                is_final: true,
            });
        }
    }
    entries
}

/// Find PBDMA IDs assigned to a specific runlist.
pub fn find_pbdmas_for_runlist(pbdma_map: u32, bar0: &MappedBar, target_runlist: u32) -> Vec<usize> {
    let mut result = Vec::new();
    let mut seq = 0_usize;
    for pid in 0..32_usize {
        if pbdma_map & (1 << pid) == 0 {
            continue;
        }
        let rl = bar0.read_u32(0x2390 + seq * 4).unwrap_or(0xFFFF);
        if rl == target_runlist {
            result.push(pid);
        }
        seq += 1;
    }
    result
}

/// Extract active PBDMA IDs from the PBDMA_MAP bitmask.
pub fn active_pbdma_ids(pbdma_map: u32) -> Vec<usize> {
    (0..32).filter(|&i| pbdma_map & (1 << i) != 0).collect()
}

/// Timed diagnostic capture: PBDMA + PCCSR snapshots at fixed intervals after doorbell.
#[derive(Debug, Clone)]
pub struct TimedCapture {
    pub label: &'static str,
    pub delay_us: u64,
    pub pccsr: PccsrSnapshot,
    pub pbdma_snapshots: Vec<PbdmaSnapshot>,
}

impl fmt::Display for TimedCapture {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "── {} (t+{}us) ──", self.label, self.delay_us)?;
        writeln!(f, "{}", self.pccsr)?;
        for snap in &self.pbdma_snapshots {
            writeln!(f, "{snap}")?;
        }
        Ok(())
    }
}
