// SPDX-License-Identifier: AGPL-3.0-only
//! Primal lifecycle state machine — standalone, modeled on sourDough patterns.
//!
//! Each primal manages its own state transitions. No compile-time coupling to
//! other primals; discovery happens at runtime via capability-based IPC.

use std::fmt;

use serde::{Deserialize, Serialize};

/// State of a primal in its lifecycle.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum PrimalState {
    /// Not yet started.
    Created,
    /// Running normally.
    Running,
    /// Stopped.
    Stopped,
    /// Failed.
    Failed,
}

impl PrimalState {
    /// Whether the primal is running.
    #[must_use]
    pub const fn is_running(self) -> bool {
        matches!(self, Self::Running)
    }

    /// Whether the primal can transition to `Running`.
    #[must_use]
    pub const fn can_start(self) -> bool {
        matches!(self, Self::Created | Self::Stopped | Self::Failed)
    }

    /// Whether the primal can transition to `Stopped`.
    #[must_use]
    pub const fn can_stop(self) -> bool {
        matches!(self, Self::Running)
    }
}

impl fmt::Display for PrimalState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Created => f.write_str("created"),
            Self::Running => f.write_str("running"),
            Self::Stopped => f.write_str("stopped"),
            Self::Failed => f.write_str("failed"),
        }
    }
}

/// Primal lifecycle error.
#[derive(Debug, thiserror::Error)]
pub enum PrimalError {
    /// Invalid state transition.
    #[error("lifecycle error: {0}")]
    Lifecycle(String),

    /// Health check failure.
    #[error("health error: {0}")]
    Health(String),

    /// I/O failure.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Internal error.
    #[error("internal error: {0}")]
    Internal(String),
}

impl PrimalError {
    /// Create a lifecycle error.
    pub fn lifecycle(msg: impl Into<String>) -> Self {
        Self::Lifecycle(msg.into())
    }
}

/// Lifecycle trait — primals manage their own start/stop transitions.
pub trait PrimalLifecycle: Send + Sync {
    /// Current state.
    fn state(&self) -> PrimalState;

    /// Start the primal.
    fn start(&mut self) -> impl std::future::Future<Output = Result<(), PrimalError>> + Send;

    /// Stop the primal.
    fn stop(&mut self) -> impl std::future::Future<Output = Result<(), PrimalError>> + Send;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_transitions() {
        assert!(PrimalState::Created.can_start());
        assert!(PrimalState::Stopped.can_start());
        assert!(PrimalState::Failed.can_start());
        assert!(!PrimalState::Running.can_start());

        assert!(PrimalState::Running.can_stop());
        assert!(!PrimalState::Created.can_stop());
        assert!(!PrimalState::Stopped.can_stop());
    }

    #[test]
    fn state_display() {
        assert_eq!(PrimalState::Created.to_string(), "created");
        assert_eq!(PrimalState::Running.to_string(), "running");
        assert_eq!(PrimalState::Stopped.to_string(), "stopped");
        assert_eq!(PrimalState::Failed.to_string(), "failed");
    }

    #[test]
    fn error_display() {
        let e = PrimalError::lifecycle("cannot start");
        assert!(e.to_string().contains("lifecycle"));
        assert!(e.to_string().contains("cannot start"));
    }
}
