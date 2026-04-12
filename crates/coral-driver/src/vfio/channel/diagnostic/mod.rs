// SPDX-License-Identifier: AGPL-3.0-only
//! Hardware bring-up diagnostic experiment matrix for VFIO channel creation.

pub mod boot_follower;
mod experiments;
pub mod interpreter;
mod matrix;
pub mod replay;
mod runner;
pub mod subsystem_validator;
mod types;

pub use experiments::context::GpuCapabilities;
pub use matrix::{build_experiment_matrix, build_metal_discovery_matrix};
pub use runner::diagnostic_matrix;
pub use types::{ExperimentConfig, ExperimentOrdering, ExperimentResult};
