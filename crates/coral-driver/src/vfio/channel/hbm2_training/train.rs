// SPDX-License-Identifier: AGPL-3.0-only
//! Convenience function for full HBM2 training sequence.

use crate::vfio::device::MappedBar;

use super::backend::TrainingBackend;
use super::constants::volta_hbm2;
use super::controller::Hbm2Controller;
use super::types::Verified;
use super::types::{Hbm2TrainingError, TrainingLog};

/// Attempt the full HBM2 training sequence: Untrained → Verified.
///
/// Returns the verified controller on success, or the error and partial log on failure.
pub fn train_hbm2<'a>(
    bar0: &'a MappedBar,
    bdf: Option<&str>,
    backend: Option<TrainingBackend>,
) -> Result<Hbm2Controller<'a, Verified>, (Hbm2TrainingError, TrainingLog)> {
    let mut ctrl = Hbm2Controller::new(bar0, bdf, volta_hbm2::FBPA_COUNT);
    if let Some(be) = backend {
        ctrl = ctrl.with_backend(be);
    }

    let ctrl = ctrl.enable_phy().map_err(|e| (e, TrainingLog::default()))?;

    let ctrl = ctrl
        .train_links()
        .map_err(|e| (e, TrainingLog::default()))?;

    let ctrl = ctrl.init_dram().map_err(|e| (e, TrainingLog::default()))?;

    ctrl.verify_vram().map_err(|e| (e, TrainingLog::default()))
}
