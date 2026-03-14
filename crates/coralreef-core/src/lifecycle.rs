// SPDX-License-Identifier: AGPL-3.0-only
//! Primal lifecycle state machine — standalone, modeled on sourDough patterns.
//!
//! Each primal manages its own state transitions. No compile-time coupling to
//! other primals; discovery happens at runtime via capability-based IPC.

use std::borrow::Cow;
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
    Lifecycle(Cow<'static, str>),

    /// Health check failure.
    #[error("health error: {0}")]
    Health(Cow<'static, str>),

    /// I/O failure.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// System clock before Unix epoch.
    #[error("system clock error: {0}")]
    SystemTime(#[from] std::time::SystemTimeError),

    /// Internal error.
    #[error("internal error: {0}")]
    Internal(Cow<'static, str>),
}

impl PrimalError {
    /// Create a lifecycle error.
    pub fn lifecycle(msg: impl Into<Cow<'static, str>>) -> Self {
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
    fn is_running() {
        assert!(PrimalState::Running.is_running());
        assert!(!PrimalState::Created.is_running());
        assert!(!PrimalState::Stopped.is_running());
        assert!(!PrimalState::Failed.is_running());
    }

    #[test]
    fn state_display() {
        assert_eq!(PrimalState::Created.to_string(), "created");
        assert_eq!(PrimalState::Running.to_string(), "running");
        assert_eq!(PrimalState::Stopped.to_string(), "stopped");
        assert_eq!(PrimalState::Failed.to_string(), "failed");
    }

    #[test]
    fn error_lifecycle() {
        let e = PrimalError::lifecycle("cannot start");
        assert!(e.to_string().contains("lifecycle"));
        assert!(e.to_string().contains("cannot start"));
    }

    #[test]
    fn error_lifecycle_static_is_zero_alloc() {
        let e = PrimalError::Lifecycle("static message".into());
        assert!(e.to_string().contains("static message"));
    }

    #[test]
    fn error_health() {
        let e = PrimalError::Health("check failed".into());
        assert!(e.to_string().contains("health"));
        assert!(e.to_string().contains("check failed"));
    }

    #[test]
    fn error_internal() {
        let e = PrimalError::Internal("unexpected state".into());
        assert!(e.to_string().contains("internal"));
        assert!(e.to_string().contains("unexpected state"));
    }

    #[test]
    fn error_io() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file missing");
        let e: PrimalError = io_err.into();
        assert!(e.to_string().contains("file missing"));
    }

    #[test]
    fn error_is_std_error() {
        let e: Box<dyn std::error::Error> = Box::new(PrimalError::lifecycle("test"));
        assert!(e.to_string().contains("test"));
    }

    #[test]
    fn state_can_start_from_created() {
        assert!(PrimalState::Created.can_start());
        assert!(!PrimalState::Created.can_stop());
    }

    #[test]
    fn state_can_start_from_stopped() {
        assert!(PrimalState::Stopped.can_start());
        assert!(!PrimalState::Stopped.can_stop());
    }

    #[test]
    fn state_can_start_from_failed() {
        assert!(PrimalState::Failed.can_start());
        assert!(!PrimalState::Failed.can_stop());
    }

    #[test]
    fn state_running_cannot_start() {
        assert!(!PrimalState::Running.can_start());
        assert!(PrimalState::Running.can_stop());
    }

    #[test]
    fn error_system_time() {
        use std::time::Duration;
        let now = std::time::SystemTime::now();
        let past = now - Duration::from_secs(1);
        let err = past.duration_since(now);
        assert!(err.is_err());
        let e: PrimalError = err.unwrap_err().into();
        assert!(e.to_string().to_lowercase().contains("system"));
    }

    #[test]
    fn state_failed_can_start() {
        assert!(PrimalState::Failed.can_start());
        assert!(!PrimalState::Failed.can_stop());
    }

    #[test]
    fn error_lifecycle_owned_string() {
        let e = PrimalError::lifecycle(String::from("dynamic msg"));
        assert!(e.to_string().contains("dynamic msg"));
    }

    #[test]
    fn error_health_owned_string() {
        let e = PrimalError::Health(String::from("health failure").into());
        assert!(e.to_string().contains("health failure"));
    }
}
