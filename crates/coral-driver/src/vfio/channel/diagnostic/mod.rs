// SPDX-License-Identifier: AGPL-3.0-only
//! Hardware bring-up diagnostic experiment matrix for VFIO channel creation.

mod experiments;
mod matrix;
mod runner;
mod types;

pub use matrix::build_experiment_matrix;
pub use runner::diagnostic_matrix;
pub use types::{ExperimentConfig, ExperimentOrdering, ExperimentResult};
