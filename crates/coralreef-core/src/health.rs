// SPDX-License-Identifier: AGPL-3.0-only
//! Primal health reporting — standalone, modeled on sourDough patterns.
//!
//! Each primal is self-describing: it knows its own health, reports its own
//! status, and exposes readiness via IPC. No compile-time coupling to other
//! primals.

use std::collections::BTreeMap;
use std::fmt;
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

use crate::lifecycle::PrimalError;

/// Overall health of a primal.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum HealthStatus {
    /// Healthy and ready to serve requests.
    Healthy,
    /// Degraded — still serving but at reduced capacity.
    Degraded {
        /// Reason for degradation.
        reason: String,
    },
    /// Unhealthy — not serving requests.
    Unhealthy {
        /// Reason for being unhealthy.
        reason: String,
    },
    /// Status not yet determined.
    Unknown,
}

impl HealthStatus {
    /// Whether the status is `Healthy`.
    #[must_use]
    pub const fn is_healthy(&self) -> bool {
        matches!(self, Self::Healthy)
    }

    /// Whether the primal can serve requests (`Healthy` or `Degraded`).
    #[must_use]
    pub const fn is_serving(&self) -> bool {
        matches!(self, Self::Healthy | Self::Degraded { .. })
    }
}

impl fmt::Display for HealthStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Healthy => f.write_str("healthy"),
            Self::Degraded { reason } => write!(f, "degraded: {reason}"),
            Self::Unhealthy { reason } => write!(f, "unhealthy: {reason}"),
            Self::Unknown => f.write_str("unknown"),
        }
    }
}

/// Nanosecond-precision timestamp for health reports.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Timestamp {
    /// Seconds since Unix epoch.
    pub secs: u64,
    /// Nanoseconds within the second.
    pub nanos: u32,
}

impl Timestamp {
    /// Current wall-clock time.
    ///
    /// # Errors
    ///
    /// Returns an error if the system clock is before the Unix epoch.
    pub fn now() -> Result<Self, std::time::SystemTimeError> {
        let d = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH)?;
        Ok(Self {
            secs: d.as_secs(),
            nanos: d.subsec_nanos(),
        })
    }
}

/// Self-describing health report emitted by a primal.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HealthReport {
    /// Primal name (from `CARGO_PKG_NAME`).
    pub name: String,
    /// Primal version (from `CARGO_PKG_VERSION`).
    pub version: String,
    /// Overall health status.
    pub status: HealthStatus,
    /// Timestamp of the report.
    pub timestamp: Timestamp,
    /// Arbitrary key-value details (`BTreeMap` for deterministic serialization).
    pub details: BTreeMap<String, String>,
}

impl HealthReport {
    /// Create a new report with `Unknown` status.
    ///
    /// # Errors
    ///
    /// Returns an error if the system clock is before the Unix epoch.
    pub fn new(
        name: impl Into<String>,
        version: impl Into<String>,
    ) -> Result<Self, std::time::SystemTimeError> {
        Ok(Self {
            name: name.into(),
            version: version.into(),
            status: HealthStatus::Unknown,
            timestamp: Timestamp::now()?,
            details: BTreeMap::new(),
        })
    }

    /// Set the overall status.
    #[must_use]
    pub fn with_status(mut self, status: HealthStatus) -> Self {
        self.status = status;
        self
    }

    /// Add a detail entry.
    #[must_use]
    pub fn with_detail(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.details.insert(key.into(), value.into());
        self
    }
}

/// Health trait — primals self-report their health.
pub trait PrimalHealth: Send + Sync {
    /// Current health status (cheap, synchronous check).
    fn health_status(&self) -> HealthStatus;

    /// Full health check (may be async, e.g. probe dependencies).
    fn health_check(
        &self,
    ) -> impl std::future::Future<Output = Result<HealthReport, PrimalError>> + Send;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn healthy_is_serving() {
        assert!(HealthStatus::Healthy.is_serving());
        assert!(HealthStatus::Healthy.is_healthy());
    }

    #[test]
    fn degraded_is_serving() {
        let d = HealthStatus::Degraded {
            reason: "high load".into(),
        };
        assert!(d.is_serving());
        assert!(!d.is_healthy());
    }

    #[test]
    fn unhealthy_not_serving() {
        let u = HealthStatus::Unhealthy {
            reason: "disk full".into(),
        };
        assert!(!u.is_serving());
        assert!(!u.is_healthy());
    }

    #[test]
    fn unknown_not_serving() {
        assert!(!HealthStatus::Unknown.is_serving());
        assert!(!HealthStatus::Unknown.is_healthy());
    }

    #[test]
    fn status_display_all_variants() {
        assert_eq!(HealthStatus::Healthy.to_string(), "healthy");
        assert_eq!(HealthStatus::Unknown.to_string(), "unknown");

        let degraded = HealthStatus::Degraded {
            reason: "thermal".into(),
        };
        let degraded_str = degraded.to_string();
        assert!(degraded_str.contains("degraded"));
        assert!(degraded_str.contains("thermal"));

        let unhealthy = HealthStatus::Unhealthy {
            reason: "oom".into(),
        };
        let unhealthy_str = unhealthy.to_string();
        assert!(unhealthy_str.contains("unhealthy"));
        assert!(unhealthy_str.contains("oom"));
    }

    #[test]
    fn health_status_is_healthy() {
        assert!(HealthStatus::Healthy.is_healthy());
        assert!(!HealthStatus::Degraded { reason: "x".into() }.is_healthy());
    }

    #[test]
    fn report_builder() {
        let r = HealthReport::new("test", "0.1.0")
            .unwrap()
            .with_status(HealthStatus::Healthy)
            .with_detail("gpu", "sm_89");
        assert_eq!(r.name, "test");
        assert_eq!(r.version, "0.1.0");
        assert!(r.status.is_healthy());
        assert_eq!(r.details.get("gpu").unwrap(), "sm_89");
    }

    #[test]
    fn report_multiple_details() {
        let r = HealthReport::new("test", "0.1.0")
            .unwrap()
            .with_detail("a", "1")
            .with_detail("b", "2");
        assert_eq!(r.details.len(), 2);
    }

    #[test]
    fn report_default_status_is_unknown() {
        let r = HealthReport::new("test", "0.1.0").unwrap();
        assert!(!r.status.is_healthy());
        assert!(!r.status.is_serving());
    }

    #[test]
    fn timestamp_is_recent() {
        let ts = Timestamp::now().unwrap();
        assert!(ts.secs > 1_700_000_000);
    }

    #[test]
    fn timestamp_debug() {
        let ts = Timestamp::now().unwrap();
        let debug = format!("{ts:?}");
        assert!(debug.contains("secs"));
    }

    #[test]
    fn report_with_status_overwrites() {
        let r = HealthReport::new("x", "1.0")
            .unwrap()
            .with_status(HealthStatus::Degraded {
                reason: "old".into(),
            })
            .with_status(HealthStatus::Healthy);
        assert!(r.status.is_healthy());
    }

    #[test]
    fn report_with_detail_overwrites_same_key() {
        let r = HealthReport::new("x", "1.0")
            .unwrap()
            .with_detail("k", "v1")
            .with_detail("k", "v2");
        assert_eq!(r.details.get("k").unwrap(), "v2");
    }

    #[test]
    fn report_unknown_status_display() {
        assert_eq!(HealthStatus::Unknown.to_string(), "unknown");
    }

    #[test]
    fn timestamp_ordering() {
        let ts1 = Timestamp::now().unwrap();
        let ts2 = Timestamp {
            secs: ts1.secs + 1,
            nanos: ts1.nanos,
        };
        assert!(ts2 > ts1);
    }
}
