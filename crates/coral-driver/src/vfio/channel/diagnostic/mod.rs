// SPDX-License-Identifier: AGPL-3.0-only
//! Hardware bring-up diagnostic experiment matrix for VFIO channel creation.

mod experiments;
pub mod interpreter;
mod matrix;
mod runner;
mod types;

pub use matrix::{build_experiment_matrix, build_metal_discovery_matrix};
pub use runner::diagnostic_matrix;
pub use types::{ExperimentConfig, ExperimentOrdering, ExperimentResult};
