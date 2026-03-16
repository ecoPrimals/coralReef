// SPDX-License-Identifier: AGPL-3.0-only
#![allow(missing_docs)]

pub(super) mod context;
mod direct_pbdma;
mod dispatch;
mod reinit;
mod runlist_ack;
mod sched_doorbell;
mod scheduler;
mod vram;

use super::types::ExperimentOrdering;
use crate::error::DriverResult;

use context::ExperimentContext;
use direct_pbdma::{direct_pbdma_activate, direct_pbdma_programming, direct_pbdma_sched_doorbell};
use dispatch::{
    full_dispatch_with_inst_bind, full_dispatch_with_preempt, scheduled_plus_direct_pbdma,
};
use reinit::{full_pfifo_reinit, no_pmc_reset_fast_poll, pfifo_reset_init};
use runlist_ack::{inst_bind_with_runlist_ack, preempt_inst_bind_ack, runlist_ack_protocol};
use sched_doorbell::{
    both_paths_sched_doorbell, clean_sched_no_work, ramfc_mirror_sched_doorbell,
    sched_with_nop_pushbuf, scheduler_path_only,
};
use scheduler::{
    bind_enable_runlist, bind_runlist_enable, bind_with_inst_bind_enable_runlist,
    runlist_bind_enable,
};
use vram::{all_vram, all_vram_direct_pbdma, vram_full_dispatch, vram_instance_bind};

/// Run a single experiment based on its ordering.
pub(super) fn run_experiment(ctx: &mut ExperimentContext<'_>) -> DriverResult<()> {
    match ctx.cfg.ordering {
        ExperimentOrdering::BindEnableRunlist => bind_enable_runlist(ctx),
        ExperimentOrdering::BindRunlistEnable => bind_runlist_enable(ctx),
        ExperimentOrdering::RunlistBindEnable => runlist_bind_enable(ctx),
        ExperimentOrdering::BindWithInstBindEnableRunlist => {
            bind_with_inst_bind_enable_runlist(ctx)
        }
        ExperimentOrdering::DirectPbdmaProgramming
        | ExperimentOrdering::DirectPbdmaWithInstBind => direct_pbdma_programming(ctx),
        ExperimentOrdering::DirectPbdmaActivate
        | ExperimentOrdering::DirectPbdmaActivateDoorbell
        | ExperimentOrdering::DirectPbdmaActivateScheduled => direct_pbdma_activate(ctx),
        ExperimentOrdering::DirectPbdmaSchedDoorbell => direct_pbdma_sched_doorbell(ctx),
        ExperimentOrdering::VramInstanceBind => vram_instance_bind(ctx),
        ExperimentOrdering::AllVram => all_vram(ctx),
        ExperimentOrdering::AllVramDirectPbdma => all_vram_direct_pbdma(ctx),
        ExperimentOrdering::VramFullDispatch => vram_full_dispatch(ctx),
        ExperimentOrdering::PfifoResetInit => pfifo_reset_init(ctx),
        ExperimentOrdering::FullDispatchWithInstBind => full_dispatch_with_inst_bind(ctx),
        ExperimentOrdering::FullDispatchWithPreempt => full_dispatch_with_preempt(ctx),
        ExperimentOrdering::ScheduledPlusDirectPbdma => scheduled_plus_direct_pbdma(ctx),
        ExperimentOrdering::RamfcMirrorSchedDoorbell => ramfc_mirror_sched_doorbell(ctx),
        ExperimentOrdering::BothPathsSchedDoorbell => both_paths_sched_doorbell(ctx),
        ExperimentOrdering::CleanSchedNoWork => clean_sched_no_work(ctx),
        ExperimentOrdering::SchedWithNopPushbuf => sched_with_nop_pushbuf(ctx),
        ExperimentOrdering::SchedulerPathOnly => scheduler_path_only(ctx),
        ExperimentOrdering::RunlistAckProtocol => runlist_ack_protocol(ctx),
        ExperimentOrdering::InstBindWithRunlistAck => inst_bind_with_runlist_ack(ctx),
        ExperimentOrdering::PreemptInstBindAck => preempt_inst_bind_ack(ctx),
        ExperimentOrdering::NoPmcResetFastPoll => no_pmc_reset_fast_poll(ctx),
        ExperimentOrdering::FullPfifoReinitDispatch
        | ExperimentOrdering::FullPfifoReinitDirectPbdma => full_pfifo_reinit(ctx),

        // Metal capability discovery experiments — handled separately by
        // the bar_cartography and gpu_vendor systems, not PFIFO dispatch.
        ExperimentOrdering::PowerStateSweep
        | ExperimentOrdering::RegisterCartography
        | ExperimentOrdering::MemoryPathMatrix
        | ExperimentOrdering::ClockDomainSweep
        | ExperimentOrdering::EngineProbe => Ok(()),

        // HBM2 training experiments — handled by the hbm2_training module
        // and exposed as hardware tests, not PFIFO dispatch experiments.
        ExperimentOrdering::Hbm2PhyProbe
        | ExperimentOrdering::Hbm2TimingCapture
        | ExperimentOrdering::Hbm2TrainingAttempt
        | ExperimentOrdering::Hbm2MinimalSet => Ok(()),
    }
}
