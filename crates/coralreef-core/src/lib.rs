// SPDX-License-Identifier: AGPL-3.0-only
#![deny(unsafe_code)]
//! # coralReef Core
//!
//! Core primal library for coralReef — a sovereign Rust GPU compiler.
//!
//! coralReef is evolved from upstream sources into a standalone multi-vendor
//! Rust crate. It fixes f64 transcendental emission and operates independently
//! with zero C dependencies.
//!
//! ## Architecture
//!
//! ```text
//! SPIR-V / WGSL
//!       │
//!       ▼
//! ┌─────────────┐
//! │  coral-reef   │  Sovereign GPU compiler
//! │  (Rust)      │
//! │              │  ┌───────────────┐
//! │  naga_translate──│ coral-reef-stubs│  Pure-Rust dependency replacements
//! │  legalize    │  └───────────────┘
//! │  opt_*       │
//! │  nv/encode   │  ┌───────────────┐
//! │              ├──│ coral-reef-isa  │  ISA instruction tables
//! └──────────────┘  └───────────────┘
//!       │
//!       ▼
//! Native GPU binary
//! ```

pub mod capability;
pub mod commands;
pub mod config;
pub mod health;
pub mod lifecycle;

use health::{HealthReport, HealthStatus, PrimalHealth};
use lifecycle::{PrimalError, PrimalLifecycle, PrimalState};

/// coralReef primal — sovereign GPU compiler.
pub struct CoralReefPrimal {
    state: PrimalState,
}

impl CoralReefPrimal {
    /// Create a new coralReef primal instance.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            state: PrimalState::Created,
        }
    }
}

impl Default for CoralReefPrimal {
    fn default() -> Self {
        Self::new()
    }
}

impl PrimalLifecycle for CoralReefPrimal {
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

impl PrimalHealth for CoralReefPrimal {
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
        let mut primal = CoralReefPrimal::new();
        assert_eq!(primal.state(), PrimalState::Created);

        primal.start().await.unwrap();
        assert_eq!(primal.state(), PrimalState::Running);

        primal.stop().await.unwrap();
        assert_eq!(primal.state(), PrimalState::Stopped);
    }

    #[tokio::test]
    async fn test_health_running() {
        let mut primal = CoralReefPrimal::new();
        primal.start().await.unwrap();

        assert!(primal.health_status().is_healthy());

        let report = primal.health_check().await.unwrap();
        assert_eq!(report.name, env!("CARGO_PKG_NAME"));
    }

    #[tokio::test]
    async fn test_health_not_running() {
        let primal = CoralReefPrimal::new();
        assert_eq!(primal.health_status(), HealthStatus::Unknown);
    }

    #[tokio::test]
    async fn test_default() {
        let primal = CoralReefPrimal::default();
        assert_eq!(primal.state(), PrimalState::Created);
    }

    #[tokio::test]
    async fn test_restart_from_stopped() {
        let mut primal = CoralReefPrimal::new();
        primal.start().await.unwrap();
        primal.stop().await.unwrap();
        primal.start().await.unwrap();
        assert_eq!(primal.state(), PrimalState::Running);
    }

    #[tokio::test]
    async fn test_cannot_start_when_running() {
        let mut primal = CoralReefPrimal::new();
        primal.start().await.unwrap();
        assert!(primal.start().await.is_err());
    }

    #[tokio::test]
    async fn test_cannot_stop_when_created() {
        let mut primal = CoralReefPrimal::new();
        assert!(primal.stop().await.is_err());
    }

    #[tokio::test]
    async fn test_health_check_when_created() {
        let primal = CoralReefPrimal::new();
        let report = primal.health_check().await.unwrap();
        assert_eq!(report.name, env!("CARGO_PKG_NAME"));
    }
}
