// SPDX-License-Identifier: AGPL-3.0-or-later
//! Dispatch capability — Layer 6 probing.

use super::super::layers::*;

/// Layer 6: Dispatch capability — GPFIFO consumption + NOP execution.
pub fn probe_dispatch(ch: &ChannelConfig) -> Result<DispatchCapability, ProbeFailure> {
    tracing::info!("L6: Dispatch — GPFIFO consumption + NOP execution");

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
