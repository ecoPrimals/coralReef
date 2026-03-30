// SPDX-License-Identifier: AGPL-3.0-only
//! Layer 7 diagnostic capture — FECS falcon, engine topology, PBDMA state.
//!
//! Captures comprehensive GPU state to diagnose why GPFIFO completions
//! stall on cold VFIO (FECS firmware not loaded → GR engine dead →
//! scheduler holds channel in PENDING → PBDMA never loads context).

use std::fmt;
use std::time::Instant;

use crate::vfio::channel::registers::{falcon, misc, pbdma, pccsr, pfifo};
use crate::vfio::device::MappedBar;

/// State snapshot of a single Falcon microcontroller.
#[derive(Debug, Clone)]
pub struct FalconState {
    /// Human-readable engine label (e.g. `FECS`, `GPCCS`, `PMU`, `SEC2`).
    pub name: &'static str,
    /// BAR0 byte offset of this falcon’s register block (`*_BASE`).
    pub base: usize,
    /// `CPUCTL`: halt/reset/start, tag invalidation, and run state bits.
    pub cpuctl: u32,
    /// `BOOTVEC`: IMEM entry address the falcon branches to after boot.
    pub bootvec: u32,
    /// `HWCFG`: IMEM/DMEM sizes, security mode, and falcon generation flags.
    pub hwcfg: u32,
    /// `DMACTL`: Falcon DMA engine control (requests, targeting).
    pub dmactl: u32,
    /// `IRQSTAT`: Pending interrupt status from this falcon.
    pub irqstat: u32,
    /// `MAILBOX0`: Host↔falcon message register (boot handshake / command status).
    pub mailbox0: u32,
    /// `MAILBOX1`: Secondary mailbox (extended protocol data).
    pub mailbox1: u32,
    /// `OS`: Falcon OS / supervisor state register.
    pub os: u32,
    /// `CURCTX`: Current GPU context handle the falcon is bound to.
    pub curctx: u32,
    /// `NXTCTX`: Next context to run (scheduler / context switch).
    pub nxtctx: u32,
    /// `DEBUG1`: Falcon debug / trap status.
    pub debug1: u32,
    /// `EXCI`: Exception info — `[31:16]`=cause, `[15:0]`=PC at fault.
    pub exci: u32,
    /// `PC`: Program counter snapshot (offset 0x030).
    pub pc: u32,
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
            exci: r(falcon::EXCI),
            pc: r(falcon::PC),
        }
    }

    /// Returns true if `CPUCTL` has `HALTED` asserted (bit 4 — firmware halted).
    pub fn is_in_reset(&self) -> bool {
        self.cpuctl & falcon::CPUCTL_HALTED != 0
    }

    /// Returns true if `CPUCTL` reports stopped (bit 5) or the read failed (`0xDEAD_DEAD`).
    pub fn is_halted(&self) -> bool {
        self.cpuctl & falcon::CPUCTL_STOPPED != 0 || self.cpuctl == 0xDEAD_DEAD
    }

    /// Returns true if `HWCFG` requires signed (ACR) firmware for this falcon.
    pub fn requires_signed_firmware(&self) -> bool {
        self.hwcfg & falcon::HWCFG_SECURITY_MODE != 0
    }

    /// Instruction memory capacity in bytes derived from `HWCFG`.
    pub fn imem_size(&self) -> u32 {
        falcon::imem_size_bytes(self.hwcfg)
    }

    /// Data memory capacity in bytes derived from `HWCFG`.
    pub fn dmem_size(&self) -> u32 {
        falcon::dmem_size_bytes(self.hwcfg)
    }

    fn state_label(&self) -> &'static str {
        if self.cpuctl == 0xDEAD_DEAD {
            "UNREACHABLE"
        } else if self.is_in_reset() && self.is_halted() {
            "HALTED+STOPPED"
        } else if self.is_in_reset() {
            "HALTED"
        } else if self.is_halted() {
            "STOPPED"
        } else if self.exci != 0 {
            "FAULTED (exci != 0)"
        } else if self.pc == 0 && self.mailbox0 == 0 && self.mailbox1 == 0 {
            "STALLED (PC=0, no mailbox)"
        } else if self.mailbox0 != 0 || self.mailbox1 != 0 {
            "RUNNING (mailbox active)"
        } else {
            "IDLE (no mailbox)"
        }
    }
}

impl fmt::Display for FalconState {
    /// Writes falcon BAR0 register snapshot as multi-line text.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "  {} @ {:#010x}: {}",
            self.name,
            self.base,
            self.state_label()
        )?;
        writeln!(
            f,
            "    cpuctl={:#010x} bootvec={:#010x} hwcfg={:#010x}",
            self.cpuctl, self.bootvec, self.hwcfg
        )?;
        writeln!(f, "    pc={:#06x} exci={:#010x}", self.pc, self.exci)?;
        writeln!(
            f,
            "    imem={}B dmem={}B secure={}",
            self.imem_size(),
            self.dmem_size(),
            self.requires_signed_firmware()
        )?;
        writeln!(
            f,
            "    irqstat={:#010x} dmactl={:#010x} debug1={:#010x}",
            self.irqstat, self.dmactl, self.debug1
        )?;
        writeln!(
            f,
            "    mailbox0={:#010x} mailbox1={:#010x}",
            self.mailbox0, self.mailbox1
        )?;
        write!(
            f,
            "    os={:#010x} curctx={:#010x} nxtctx={:#010x}",
            self.os, self.curctx, self.nxtctx
        )
    }
}

/// Decoded engine topology entry from `ENGN_TABLE` (BAR0 + 0x22700).
#[derive(Debug, Clone)]
pub struct EngineEntry {
    /// Row index in `ENGN_TABLE` (0..32).
    pub index: u32,
    /// Raw `ENGN_TABLE` dword for this row (kind bits, type, runlist, FINAL).
    pub raw: u32,
    /// Decoded engine class (`GR`, `CE`, `NVDEC`, …) from accumulated type rows.
    pub engine_type: u32,
    /// Runlist ID this engine instance is bound to (from accumulated runlist rows).
    pub runlist: u32,
    /// Set when this row’s FINAL bit marks the end of one logical engine entry.
    pub is_final: bool,
}

impl fmt::Display for EngineEntry {
    /// Writes decoded topology row (type name, runlist, FINAL flag).
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
            f,
            "  ENGN[{:2}]: {:#010x} type={} ({}) runlist={}{}",
            self.index,
            self.raw,
            self.engine_type,
            type_name,
            self.runlist,
            if self.is_final { " (FINAL)" } else { "" }
        )
    }
}

/// Point-in-time snapshot of a PBDMA's operational registers.
#[derive(Debug, Clone)]
pub struct PbdmaSnapshot {
    /// PBDMA instance index (0..32) within `PBDMA_MAP`.
    pub pbdma_id: usize,
    /// Microseconds since capture start (monotonic stamp for ordering samples).
    pub timestamp_us: u64,
    /// `GP_BASE_LO`: low 32 bits of GPFIFO base address in GPU VA.
    pub gp_base_lo: u32,
    /// `GP_BASE_HI`: high 32 bits of GPFIFO base address in GPU VA.
    pub gp_base_hi: u32,
    /// `GP_PUT`: host-written GPFIFO put pointer (work available to GPU).
    pub gp_put: u32,
    /// `GP_FETCH`: GPU fetch pointer into the GPFIFO ring.
    pub gp_fetch: u32,
    /// `GP_STATE`: GPFIFO engine state (idle, running, fault, etc.).
    pub gp_state: u32,
    /// `USERD_LO`: low 32 bits of user channel doorbell / `USERD` block address.
    pub userd_lo: u32,
    /// `USERD_HI`: high 32 bits of `USERD` address.
    pub userd_hi: u32,
    /// `CHANNEL_STATE`: channel bind state and scheduler linkage for this PBDMA.
    pub channel_state: u32,
    /// `METHOD0`: current or last method header being decoded on the PBDMA.
    pub method0: u32,
    /// `DATA0`: first inline method data dword associated with `METHOD0`.
    pub data0: u32,
    /// PBDMA interrupt status register for this instance (`INTR` bank).
    pub intr: u32,
    /// `CHANNEL_INFO`: channel ID and metadata tied to the loaded context.
    pub channel_info: u32,
    /// `SIGNATURE`: PBDMA sanity / identity signature dword (hardware-specific).
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

    /// Returns true if `GP_BASE` is non-zero (GPFIFO context bound to this PBDMA).
    pub fn has_channel_loaded(&self) -> bool {
        self.gp_base_lo != 0 || self.gp_base_hi != 0
    }
}

impl fmt::Display for PbdmaSnapshot {
    /// Writes PBDMA GPFIFO, `USERD`, method, and interrupt registers.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "  PBDMA[{}] @ t={}us:", self.pbdma_id, self.timestamp_us)?;
        writeln!(
            f,
            "    gp_base={:#010x}:{:#010x} gp_put={:#010x} gp_fetch={:#010x} gp_state={:#010x}",
            self.gp_base_hi, self.gp_base_lo, self.gp_put, self.gp_fetch, self.gp_state
        )?;
        writeln!(
            f,
            "    userd={:#010x}:{:#010x} chan_state={:#010x} chan_info={:#010x}",
            self.userd_hi, self.userd_lo, self.channel_state, self.channel_info
        )?;
        write!(
            f,
            "    method0={:#010x} data0={:#010x} intr={:#010x} sig={:#010x}",
            self.method0, self.data0, self.intr, self.signature
        )
    }
}

/// PCCSR (per-channel control/status) snapshot.
#[derive(Debug, Clone)]
pub struct PccsrSnapshot {
    /// Logical channel index used to index PCCSR register banks.
    pub channel_id: u32,
    /// `PCCSR_INST`: instance / bind register for this channel.
    pub inst: u32,
    /// `PCCSR_CHANNEL`: primary channel status dword (enable, busy, fault bits).
    pub channel: u32,
}

impl PccsrSnapshot {
    /// Capture PCCSR state for a channel.
    pub fn capture(bar0: &MappedBar, channel_id: u32) -> Self {
        Self {
            channel_id,
            inst: bar0
                .read_u32(pccsr::inst(channel_id))
                .unwrap_or(0xDEAD_DEAD),
            channel: bar0
                .read_u32(pccsr::channel(channel_id))
                .unwrap_or(0xDEAD_DEAD),
        }
    }

    /// Decoded scheduler status code from `PCCSR_CHANNEL` bitfields.
    pub fn status(&self) -> u32 {
        pccsr::status(self.channel)
    }

    /// Human-readable name for [`Self::status`].
    pub fn status_name(&self) -> &'static str {
        pccsr::status_name(self.channel)
    }

    /// Returns true if the channel enable bit is set in `PCCSR_CHANNEL`.
    pub fn is_enabled(&self) -> bool {
        self.channel & 1 != 0
    }

    /// Returns true if the busy bit is set (channel work in flight).
    pub fn is_busy(&self) -> bool {
        self.channel & (1 << 28) != 0
    }

    /// Returns true if PBDMA fault is reported for this channel.
    pub fn pbdma_faulted(&self) -> bool {
        self.channel & (1 << 22) != 0
    }

    /// Returns true if engine fault is reported for this channel.
    pub fn eng_faulted(&self) -> bool {
        self.channel & (1 << 23) != 0
    }
}

impl fmt::Display for PccsrSnapshot {
    /// Writes PCCSR instance, channel dword, and decoded fault flags.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "  PCCSR[{}]: inst={:#010x} chan={:#010x} status={} en={} busy={} pbdma_fault={} eng_fault={}",
            self.channel_id,
            self.inst,
            self.channel,
            self.status_name(),
            self.is_enabled(),
            self.is_busy(),
            self.pbdma_faulted(),
            self.eng_faulted()
        )
    }
}

/// PFIFO scheduler state snapshot.
#[derive(Debug, Clone)]
pub struct PfifoSnapshot {
    /// `PFIFO_INTR`: aggregated interrupt bits (runlist complete, CHSW error, PBDMA).
    pub intr: u32,
    /// `PFIFO_INTR_EN`: interrupt mask.
    pub intr_en: u32,
    /// `SCHED_DISABLE`: bits disabling scheduler entities (engines/channels).
    pub sched_disable: u32,
    /// `CHSW_ERROR`: channel switch error status / latch.
    pub chsw_error: u32,
    /// `PBDMA_MAP`: bitmask of which PBDMA units exist and are mapped to PFIFO.
    pub pbdma_map: u32,
}

impl fmt::Display for PfifoSnapshot {
    /// Writes PFIFO interrupt, scheduling, and PBDMA presence state.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let rl_complete = self.intr & pfifo::INTR_RL_COMPLETE != 0;
        let chsw_err = self.intr & pfifo::INTR_CHSW_ERROR != 0;
        let pbdma_agg = self.intr & pfifo::INTR_PBDMA != 0;
        writeln!(f, "  PFIFO:")?;
        writeln!(
            f,
            "    intr={:#010x} (rl_complete={} chsw_err={} pbdma_agg={})",
            self.intr, rl_complete, chsw_err, pbdma_agg
        )?;
        writeln!(
            f,
            "    intr_en={:#010x} sched_disable={:#010x} chsw_error={:#010x}",
            self.intr_en, self.sched_disable, self.chsw_error
        )?;
        write!(
            f,
            "    pbdma_map={:#010x} (active PBDMAs: {:?})",
            self.pbdma_map,
            active_pbdma_ids(self.pbdma_map)
        )
    }
}

/// Full Layer 7 diagnostic capture.
#[derive(Debug, Clone)]
pub struct Layer7Diagnostics {
    /// Caller label (e.g. VFIO device or test name) for log correlation.
    pub label: String,
    /// FECS falcon snapshot (front-end context scheduling firmware).
    pub fecs: FalconState,
    /// GPCCS falcon snapshot (GPC context scheduling).
    pub gpccs: FalconState,
    /// PMU falcon snapshot (power management microcontroller).
    pub pmu: FalconState,
    /// SEC2 falcon snapshot (security / video offload engine falcon).
    pub sec2: FalconState,
    /// Parsed `ENGN_TABLE` entries (engine type and runlist binding).
    pub engine_topology: Vec<EngineEntry>,
    /// Non-zero `ENGN_STATUS` rows: `(engine_index, raw_status_dword)`.
    pub engine_status: Vec<(u32, u32)>,
    /// PCCSR snapshot for the channel under test.
    pub pccsr: PccsrSnapshot,
    /// PFIFO interrupt and scheduler state.
    pub pfifo: PfifoSnapshot,
    /// PBDMA snapshots for units serving the GR runlist (see `find_pbdmas_for_runlist`).
    pub pbdma_snapshots: Vec<PbdmaSnapshot>,
    /// `PMC_ENABLE` at BAR0+0x200: master clock/enable bits (includes GR enable).
    pub pmc_enable: u32,
    /// `PGRAPH` status at BAR0+0x400700: graphics engine idle/fault summary.
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
            pmc_enable: r(misc::PMC_ENABLE),
            pgraph_status: r(misc::PGRAPH_STATUS),
        }
    }
}

impl fmt::Display for Layer7Diagnostics {
    /// Writes the full Layer 7 report (Falcons, topology, PCCSR, PFIFO, PBDMA).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "╔══ Layer 7 Diagnostics: {} ══════════════════════════════╗",
            self.label
        )?;
        writeln!(
            f,
            "  pmc_enable={:#010x} pgraph_status={:#010x}",
            self.pmc_enable, self.pgraph_status
        )?;
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
        write!(
            f,
            "╚═══════════════════════════════════════════════════════════╝"
        )
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
pub fn find_pbdmas_for_runlist(
    pbdma_map: u32,
    bar0: &MappedBar,
    target_runlist: u32,
) -> Vec<usize> {
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
    /// Phase label (e.g. `pre-doorbell`, `t+500us`).
    pub label: &'static str,
    /// Elapsed time in microseconds from a reference event (doorbell).
    pub delay_us: u64,
    /// PCCSR snapshot at this timestep.
    pub pccsr: PccsrSnapshot,
    /// PBDMA snapshots for the same GR runlist PBDMAs at this timestep.
    pub pbdma_snapshots: Vec<PbdmaSnapshot>,
}

impl fmt::Display for TimedCapture {
    /// Writes timed PCCSR and PBDMA blocks with phase label.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "── {} (t+{}us) ──", self.label, self.delay_us)?;
        writeln!(f, "{}", self.pccsr)?;
        for snap in &self.pbdma_snapshots {
            writeln!(f, "{snap}")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_falcon(name: &'static str, cpuctl: u32, hwcfg: u32) -> FalconState {
        FalconState {
            name,
            base: 0,
            cpuctl,
            bootvec: 0,
            hwcfg,
            dmactl: 0,
            irqstat: 0,
            mailbox0: 0,
            mailbox1: 0,
            os: 0,
            curctx: 0,
            nxtctx: 0,
            debug1: 0,
            exci: 0,
            pc: 0,
        }
    }

    #[test]
    fn falcon_state_reset_and_signed_firmware() {
        let s = sample_falcon("FECS", falcon::CPUCTL_HALTED, falcon::HWCFG_SECURITY_MODE);
        assert!(s.is_in_reset());
        assert!(s.requires_signed_firmware());
    }

    #[test]
    fn falcon_state_imem_dmem_from_hwcfg() {
        let hwcfg = (4 << 9) | 5;
        let s = sample_falcon("FECS", 0, hwcfg);
        assert_eq!(s.imem_size(), 5 * 256);
        assert_eq!(s.dmem_size(), 4 * 256);
    }

    #[test]
    fn falcon_display_contains_labels() {
        let s = sample_falcon("SEC2", 0x12, 0);
        let text = format!("{s}");
        assert!(text.contains("SEC2"));
        assert!(text.contains("cpuctl="));
        assert!(text.contains("imem="));
    }

    #[test]
    fn engine_entry_display() {
        let e = EngineEntry {
            index: 3,
            raw: 0x8000_0000,
            engine_type: 1,
            runlist: 2,
            is_final: true,
        };
        let text = format!("{e}");
        assert!(text.contains("ENGN[ 3]"));
        assert!(text.contains("CE"));
        assert!(text.contains("(FINAL)"));
    }

    #[test]
    fn pbdma_snapshot_has_channel_and_display() {
        let p = PbdmaSnapshot {
            pbdma_id: 1,
            timestamp_us: 42,
            gp_base_lo: 0x1000,
            gp_base_hi: 0,
            gp_put: 0,
            gp_fetch: 0,
            gp_state: 0,
            userd_lo: 0,
            userd_hi: 0,
            channel_state: 0,
            method0: 0,
            data0: 0,
            intr: 0,
            channel_info: 0,
            signature: 0,
        };
        assert!(p.has_channel_loaded());
        let text = format!("{p}");
        assert!(text.contains("PBDMA[1]"));
        assert!(text.contains("t=42us"));
    }

    #[test]
    fn pccsr_snapshot_status_and_flags() {
        let channel = (1 << 28) | (1 << 22) | 1;
        let snap = PccsrSnapshot {
            channel_id: 0,
            inst: 0,
            channel,
        };
        assert!(snap.is_enabled());
        assert!(snap.is_busy());
        assert!(snap.pbdma_faulted());
        assert!(!snap.eng_faulted());
        let text = format!("{snap}");
        assert!(text.contains("PCCSR[0]"));
        assert!(text.contains("busy=true"));
    }

    #[test]
    fn active_pbdma_ids_collects_set_bits() {
        assert_eq!(active_pbdma_ids(0b1010), vec![1, 3]);
        assert!(active_pbdma_ids(0).is_empty());
    }

    #[test]
    fn pfifo_snapshot_display_decodes_intr() {
        let snap = PfifoSnapshot {
            intr: pfifo::INTR_RL_COMPLETE | pfifo::INTR_CHSW_ERROR,
            intr_en: 0xFFFF_FFFF,
            sched_disable: 0,
            chsw_error: 0,
            pbdma_map: 0x3,
        };
        let text = format!("{snap}");
        assert!(text.contains("rl_complete=true"));
        assert!(text.contains("chsw_err=true"));
        assert!(text.contains("pbdma_map="));
    }

    #[test]
    fn timed_capture_display_includes_label() {
        let cap = TimedCapture {
            label: "pre-doorbell",
            delay_us: 500,
            pccsr: PccsrSnapshot {
                channel_id: 0,
                inst: 0,
                channel: 0,
            },
            pbdma_snapshots: vec![],
        };
        let text = format!("{cap}");
        assert!(text.contains("pre-doorbell"));
        assert!(text.contains("500"));
    }

    #[test]
    fn layer7_diagnostics_display_frame() {
        let diag = Layer7Diagnostics {
            label: "unit".to_string(),
            fecs: sample_falcon("FECS", 0, 0),
            gpccs: sample_falcon("GPCCS", 0, 0),
            pmu: sample_falcon("PMU", 0, 0),
            sec2: sample_falcon("SEC2", 0, 0),
            engine_topology: vec![],
            engine_status: vec![],
            pccsr: PccsrSnapshot {
                channel_id: 0,
                inst: 0,
                channel: 0,
            },
            pfifo: PfifoSnapshot {
                intr: 0,
                intr_en: 0,
                sched_disable: 0,
                chsw_error: 0,
                pbdma_map: 0,
            },
            pbdma_snapshots: vec![],
            pmc_enable: 0,
            pgraph_status: 0,
        };
        let text = format!("{diag}");
        assert!(text.contains("Layer 7 Diagnostics: unit"));
        assert!(text.contains("── Falcon States ──"));
    }
}
