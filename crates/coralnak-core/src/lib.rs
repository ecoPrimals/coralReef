// SPDX-License-Identifier: AGPL-3.0-only
//! # coralNak Core
//!
//! Core primal library for coralNak — a sovereign Rust NVIDIA shader compiler.
//!
//! coralNak is forked from Mesa's NAK compiler and evolved into a standalone
//! Rust crate.  It fixes f64 transcendental emission and operates independently
//! of the Mesa C build system.
//!
//! ## Sovereign Compute Evolution
//!
//! coralNak is Level 2-3 of the ecoPrimals Sovereign Compute roadmap:
//!
//! - **Level 2**: Fork NAK, fix f64 transcendental emission (exp, log, sin, cos)
//! - **Level 3**: Standalone Rust crate, remove all Mesa C dependencies
//!
//! ## Architecture
//!
//! ```text
//! SPIR-V / WGSL
//!       │
//!       ▼
//! ┌─────────────┐
//! │  coral-nak   │  Sovereign shader compiler
//! │  (Rust)      │
//! │              │  ┌───────────────┐
//! │  from_nir ───┼──│ coral-nak-stubs│  Mesa dependency replacements
//! │  legalize    │  └───────────────┘
//! │  opt_*       │
//! │  sm70_encode │  ┌───────────────┐
//! │              ├──│ coral-nak-isa  │  NVIDIA ISA instruction tables
//! └──────────────┘  └───────────────┘
//!       │
//!       ▼
//! Native GPU binary (SM70+)
//! ```

pub mod capability;
pub mod health;
pub mod lifecycle;

use health::{HealthReport, HealthStatus, PrimalHealth};
use lifecycle::{PrimalError, PrimalLifecycle, PrimalState};

/// coralNak primal — sovereign NVIDIA shader compiler.
pub struct CoralNakPrimal {
    state: PrimalState,
}

impl CoralNakPrimal {
    /// Create a new coralNak primal instance.
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: PrimalState::Created,
        }
    }
}

impl Default for CoralNakPrimal {
    fn default() -> Self {
        Self::new()
    }
}

impl PrimalLifecycle for CoralNakPrimal {
    fn state(&self) -> PrimalState {
        self.state
    }

    async fn start(&mut self) -> Result<(), PrimalError> {
        if !self.state.can_start() {
            return Err(PrimalError::lifecycle("Cannot start from current state"));
        }
        self.state = PrimalState::Running;
        Ok(())
    }

    async fn stop(&mut self) -> Result<(), PrimalError> {
        if !self.state.can_stop() {
            return Err(PrimalError::lifecycle("Cannot stop from current state"));
        }
        self.state = PrimalState::Stopped;
        Ok(())
    }
}

impl PrimalHealth for CoralNakPrimal {
    fn health_status(&self) -> HealthStatus {
        if self.state.is_running() {
            HealthStatus::Healthy
        } else {
            HealthStatus::Unknown
        }
    }

    async fn health_check(&self) -> Result<HealthReport, PrimalError> {
        Ok(
            HealthReport::new(env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"))
                .with_status(self.health_status()),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_lifecycle() {
        let mut primal = CoralNakPrimal::new();
        assert_eq!(primal.state(), PrimalState::Created);

        primal.start().await.unwrap();
        assert_eq!(primal.state(), PrimalState::Running);

        primal.stop().await.unwrap();
        assert_eq!(primal.state(), PrimalState::Stopped);
    }

    #[tokio::test]
    async fn test_health_running() {
        let mut primal = CoralNakPrimal::new();
        primal.start().await.unwrap();

        assert!(primal.health_status().is_healthy());

        let report = primal.health_check().await.unwrap();
        assert_eq!(report.name, env!("CARGO_PKG_NAME"));
    }

    #[tokio::test]
    async fn test_health_not_running() {
        let primal = CoralNakPrimal::new();
        assert_eq!(primal.health_status(), HealthStatus::Unknown);
    }

    #[tokio::test]
    async fn test_default() {
        let primal = CoralNakPrimal::default();
        assert_eq!(primal.state(), PrimalState::Created);
    }

    #[tokio::test]
    async fn test_restart_from_stopped() {
        let mut primal = CoralNakPrimal::new();
        primal.start().await.unwrap();
        primal.stop().await.unwrap();
        primal.start().await.unwrap();
        assert_eq!(primal.state(), PrimalState::Running);
    }

    #[tokio::test]
    async fn test_cannot_start_when_running() {
        let mut primal = CoralNakPrimal::new();
        primal.start().await.unwrap();
        assert!(primal.start().await.is_err());
    }

    #[tokio::test]
    async fn test_cannot_stop_when_created() {
        let mut primal = CoralNakPrimal::new();
        assert!(primal.stop().await.is_err());
    }

    #[tokio::test]
    async fn test_health_check_when_created() {
        let primal = CoralNakPrimal::new();
        let report = primal.health_check().await.unwrap();
        assert_eq!(report.name, env!("CARGO_PKG_NAME"));
    }
}
