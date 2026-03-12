// SPDX-License-Identifier: AGPL-3.0-only
//! RM Protocol Observer — captures RM operations for the virtual GSP knowledge base.
//!
//! Every successful RM operation (alloc, control, free) is recorded as a
//! [`RmRecord`]. A session of records forms a [`RmProtocolLog`] that can be
//! serialized and fed into [`GpuKnowledge`] for cross-generation learning.
//!
//! The observer answers: "What RM operations, in what order, with what
//! parameters, does a working GPU need to reach compute-ready state?"
//! A virtual GSP for an older GPU replays a compatible sequence.

use std::time::Instant;

/// One observed RM operation.
#[derive(Debug, Clone, serde::Serialize)]
pub struct RmRecord {
    /// Monotonic sequence number within the session.
    pub seq: u32,
    /// Operation kind.
    pub op: RmOp,
    /// RM root client handle.
    pub h_root: u32,
    /// Parent object handle.
    pub h_parent: u32,
    /// Target / new object handle.
    pub h_object: u32,
    /// RM class (for alloc) or control command (for control).
    pub class_or_cmd: u32,
    /// Allocation parameter size (0 = kernel-inferred).
    pub params_size: u32,
    /// RM status code returned by kernel (0 = NV_OK).
    pub status: u32,
    /// Elapsed microseconds for the ioctl round-trip.
    pub elapsed_us: u64,
    /// GPU SM version this record was captured on (if known).
    pub sm: Option<u32>,
}

/// Kind of RM operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum RmOp {
    /// `NV_ESC_RM_ALLOC` with typed allocation parameters.
    AllocTyped,
    /// `NV_ESC_RM_ALLOC` with no allocation parameters (class-only).
    AllocSimple,
    /// `NV_ESC_RM_CONTROL` — control call on an existing object.
    Control,
    /// `NV_ESC_RM_FREE` — object teardown.
    Free,
}

/// Accumulates [`RmRecord`]s for one GPU session.
///
/// A session starts when an `RmClient` is created and ends when it is
/// dropped. The log captures the full RM object tree construction
/// sequence that brought the GPU to compute-ready state.
#[derive(Debug, Clone, serde::Serialize)]
pub struct RmProtocolLog {
    /// GPU chip codename (e.g. "ga102") if known.
    pub chip: Option<String>,
    /// SM version (e.g. 86) if known.
    pub sm: Option<u32>,
    /// Driver version string if known.
    pub driver_version: Option<String>,
    /// Ordered records.
    pub records: Vec<RmRecord>,
}

impl RmProtocolLog {
    /// Create a new empty log.
    #[must_use]
    pub fn new() -> Self {
        Self {
            chip: None,
            sm: None,
            driver_version: None,
            records: Vec::new(),
        }
    }

    /// Create a log tagged with GPU identity.
    #[must_use]
    pub fn for_gpu(chip: impl Into<String>, sm: u32) -> Self {
        Self {
            chip: Some(chip.into()),
            sm: Some(sm),
            driver_version: None,
            records: Vec::new(),
        }
    }

    /// Record one RM operation.
    pub fn record(&mut self, rec: RmRecord) {
        tracing::trace!(
            seq = rec.seq,
            op = ?rec.op,
            class = format_args!("0x{:04X}", rec.class_or_cmd),
            status = format_args!("0x{:08X}", rec.status),
            elapsed_us = rec.elapsed_us,
            "RM protocol observed"
        );
        self.records.push(rec);
    }

    /// Number of recorded operations.
    #[must_use]
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// Whether the log is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// All successfully-allocated RM classes in order.
    #[must_use]
    pub fn successful_classes(&self) -> Vec<u32> {
        self.records
            .iter()
            .filter(|r| matches!(r.op, RmOp::AllocTyped | RmOp::AllocSimple) && r.status == 0)
            .map(|r| r.class_or_cmd)
            .collect()
    }

    /// Extract the allocation recipe — the ordered class sequence needed
    /// to bring a GPU to compute-ready state.
    #[must_use]
    pub fn allocation_recipe(&self) -> Vec<RmAllocStep> {
        self.records
            .iter()
            .filter(|r| matches!(r.op, RmOp::AllocTyped | RmOp::AllocSimple) && r.status == 0)
            .map(|r| RmAllocStep {
                class: r.class_or_cmd,
                has_params: r.op == RmOp::AllocTyped,
                params_size: r.params_size,
            })
            .collect()
    }

    /// Serialize to JSON for storage and cross-session learning.
    ///
    /// # Errors
    /// Returns error if serialization fails.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }
}

impl Default for RmProtocolLog {
    fn default() -> Self {
        Self::new()
    }
}

/// One step in an RM allocation recipe (class + whether it needs params).
#[derive(Debug, Clone, serde::Serialize)]
pub struct RmAllocStep {
    /// RM class ID.
    pub class: u32,
    /// Whether allocation used typed parameters.
    pub has_params: bool,
    /// Parameter struct size (0 = kernel-inferred).
    pub params_size: u32,
}

/// Observation hook that [`RmClient`] calls on each RM operation.
///
/// Implementations can record to an [`RmProtocolLog`], emit metrics,
/// or feed a live learning system.
pub trait RmObserver: Send {
    /// Called after an RM alloc (typed or simple) completes.
    fn on_alloc(
        &mut self,
        h_root: u32,
        h_parent: u32,
        h_new: u32,
        h_class: u32,
        params_size: u32,
        status: u32,
        elapsed: std::time::Duration,
    );

    /// Called after an RM control call completes.
    fn on_control(
        &mut self,
        h_client: u32,
        h_object: u32,
        cmd: u32,
        status: u32,
        elapsed: std::time::Duration,
    );

    /// Called after an RM free completes.
    fn on_free(&mut self, h_root: u32, h_object: u32, status: u32);

    /// Downcast to `Any` for type recovery (e.g., extracting `LoggingObserver`).
    fn into_any(self: Box<Self>) -> Box<dyn std::any::Any>;
}

/// Default observer that records into an [`RmProtocolLog`].
pub struct LoggingObserver {
    log: RmProtocolLog,
    next_seq: u32,
    #[expect(dead_code, reason = "retained for future session timing and profiling")]
    session_start: Instant,
}

impl LoggingObserver {
    /// Create a new logging observer.
    #[must_use]
    pub fn new() -> Self {
        Self {
            log: RmProtocolLog::new(),
            next_seq: 0,
            session_start: Instant::now(),
        }
    }

    /// Create a logging observer tagged with GPU identity.
    #[must_use]
    pub fn for_gpu(chip: impl Into<String>, sm: u32) -> Self {
        Self {
            log: RmProtocolLog::for_gpu(chip, sm),
            next_seq: 0,
            session_start: Instant::now(),
        }
    }

    fn next_seq(&mut self) -> u32 {
        let s = self.next_seq;
        self.next_seq += 1;
        s
    }

    /// Consume the observer and return the accumulated protocol log.
    #[must_use]
    pub fn into_log(self) -> RmProtocolLog {
        self.log
    }

    /// Borrow the accumulated log.
    #[must_use]
    pub fn log(&self) -> &RmProtocolLog {
        &self.log
    }
}

impl Default for LoggingObserver {
    fn default() -> Self {
        Self::new()
    }
}

impl RmObserver for LoggingObserver {
    fn on_alloc(
        &mut self,
        h_root: u32,
        h_parent: u32,
        h_new: u32,
        h_class: u32,
        params_size: u32,
        status: u32,
        elapsed: std::time::Duration,
    ) {
        let op = if params_size > 0 {
            RmOp::AllocTyped
        } else {
            RmOp::AllocSimple
        };
        let seq = self.next_seq();
        self.log.record(RmRecord {
            seq,
            op,
            h_root,
            h_parent,
            h_object: h_new,
            class_or_cmd: h_class,
            params_size,
            status,
            elapsed_us: elapsed.as_micros() as u64,
            sm: self.log.sm,
        });
    }

    fn on_control(
        &mut self,
        h_client: u32,
        h_object: u32,
        cmd: u32,
        status: u32,
        elapsed: std::time::Duration,
    ) {
        let seq = self.next_seq();
        self.log.record(RmRecord {
            seq,
            op: RmOp::Control,
            h_root: h_client,
            h_parent: h_client,
            h_object,
            class_or_cmd: cmd,
            params_size: 0,
            status,
            elapsed_us: elapsed.as_micros() as u64,
            sm: self.log.sm,
        });
    }

    fn on_free(&mut self, h_root: u32, h_object: u32, status: u32) {
        let seq = self.next_seq();
        self.log.record(RmRecord {
            seq,
            op: RmOp::Free,
            h_root,
            h_parent: h_root,
            h_object,
            class_or_cmd: 0,
            params_size: 0,
            status,
            elapsed_us: 0,
            sm: self.log.sm,
        });
    }

    fn into_any(self: Box<Self>) -> Box<dyn std::any::Any> {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logging_observer_captures_session() {
        let mut obs = LoggingObserver::for_gpu("ga102", 86);

        obs.on_alloc(1, 0, 1, 0x0041, 0, 0, std::time::Duration::from_micros(50));
        obs.on_alloc(1, 1, 2, 0x0080, 56, 0, std::time::Duration::from_micros(30));
        obs.on_alloc(1, 2, 3, 0x2080, 4, 0, std::time::Duration::from_micros(20));
        obs.on_control(1, 3, 0x2080_014A, 0, std::time::Duration::from_micros(100));
        obs.on_alloc(1, 2, 4, 0x90F1, 0, 0, std::time::Duration::from_micros(25));
        obs.on_alloc(1, 2, 5, 0xA06C, 32, 0, std::time::Duration::from_micros(35));
        obs.on_alloc(1, 2, 6, 0x003E, 128, 0, std::time::Duration::from_micros(40));
        obs.on_alloc(1, 5, 7, 0xC56F, 0, 0, std::time::Duration::from_micros(60));
        obs.on_alloc(1, 7, 8, 0xC7C0, 0, 0, std::time::Duration::from_micros(15));

        let log = obs.into_log();
        assert_eq!(log.len(), 9);
        assert_eq!(log.sm, Some(86));

        let classes = log.successful_classes();
        assert_eq!(
            classes,
            vec![0x0041, 0x0080, 0x2080, 0x90F1, 0xA06C, 0x003E, 0xC56F, 0xC7C0]
        );

        let recipe = log.allocation_recipe();
        assert_eq!(recipe.len(), 8);
        assert_eq!(recipe[0].class, 0x0041);
        assert!(!recipe[0].has_params);
        assert_eq!(recipe[1].class, 0x0080);
        assert!(recipe[1].has_params);
    }

    #[test]
    fn log_serialization_roundtrip() {
        let mut log = RmProtocolLog::for_gpu("gv100", 70);
        log.records.push(RmRecord {
            seq: 0,
            op: RmOp::AllocTyped,
            h_root: 1,
            h_parent: 0,
            h_object: 1,
            class_or_cmd: 0x0041,
            params_size: 0,
            status: 0,
            elapsed_us: 42,
            sm: Some(70),
        });
        let json = log.to_json().expect("serialize");
        assert!(json.contains("\"class_or_cmd\": 65"));
        assert!(json.contains("gv100"));
    }
}
