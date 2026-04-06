// SPDX-License-Identifier: AGPL-3.0-or-later
//! RM Protocol Observer — captures RM operations for the virtual GSP knowledge base.
//!
//! Every successful RM operation (alloc, control, free) is recorded as a
//! [`RmRecord`]. A session of records forms a [`RmProtocolLog`] that can be
//! serialized and fed into [`GpuKnowledge`](super::knowledge::GpuKnowledge) for cross-generation learning.
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
    /// RM status code returned by kernel (0 = `NV_OK`).
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

/// Structured event for an RM allocation operation.
///
/// Replaces a 7-parameter positional argument list with a named-field struct,
/// improving readability at call sites and satisfying `clippy::too_many_arguments`.
#[derive(Debug, Clone)]
pub struct RmAllocEvent {
    /// RM root client handle.
    pub h_root: u32,
    /// Parent object handle.
    pub h_parent: u32,
    /// Newly allocated object handle.
    pub h_new: u32,
    /// RM class ID of the allocated object.
    pub h_class: u32,
    /// Allocation parameter struct size (0 = kernel-inferred / simple alloc).
    pub params_size: u32,
    /// RM status code returned by kernel (0 = `NV_OK`).
    pub status: u32,
    /// Elapsed wall-clock time for the ioctl round-trip.
    pub elapsed: std::time::Duration,
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
    pub const fn new() -> Self {
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
    pub const fn len(&self) -> usize {
        self.records.len()
    }

    /// Whether the log is empty.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
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

/// Observation hook that [`RmClient`](crate::nv::uvm::RmClient) calls on each RM operation.
///
/// Implementations can record to an [`RmProtocolLog`], emit metrics,
/// or feed a live learning system.
pub trait RmObserver: Send {
    /// Called after an RM alloc (typed or simple) completes.
    fn on_alloc(&mut self, event: &RmAllocEvent);

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

    const fn next_seq(&mut self) -> u32 {
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
    pub const fn log(&self) -> &RmProtocolLog {
        &self.log
    }
}

impl Default for LoggingObserver {
    fn default() -> Self {
        Self::new()
    }
}

impl RmObserver for LoggingObserver {
    fn on_alloc(&mut self, event: &RmAllocEvent) {
        let op = if event.params_size > 0 {
            RmOp::AllocTyped
        } else {
            RmOp::AllocSimple
        };
        let seq = self.next_seq();
        self.log.record(RmRecord {
            seq,
            op,
            h_root: event.h_root,
            h_parent: event.h_parent,
            h_object: event.h_new,
            class_or_cmd: event.h_class,
            params_size: event.params_size,
            status: event.status,
            elapsed_us: u64::try_from(event.elapsed.as_micros()).unwrap_or(u64::MAX),
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
            elapsed_us: u64::try_from(elapsed.as_micros()).unwrap_or(u64::MAX),
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

        let alloc = |h_root, h_parent, h_new, h_class, params_size, status, us| RmAllocEvent {
            h_root,
            h_parent,
            h_new,
            h_class,
            params_size,
            status,
            elapsed: std::time::Duration::from_micros(us),
        };
        obs.on_alloc(&alloc(1, 0, 1, 0x0041, 0, 0, 50));
        obs.on_alloc(&alloc(1, 1, 2, 0x0080, 56, 0, 30));
        obs.on_alloc(&alloc(1, 2, 3, 0x2080, 4, 0, 20));
        obs.on_control(1, 3, 0x2080_014A, 0, std::time::Duration::from_micros(100));
        obs.on_alloc(&alloc(1, 2, 4, 0x90F1, 0, 0, 25));
        obs.on_alloc(&alloc(1, 2, 5, 0xA06C, 32, 0, 35));
        obs.on_alloc(&alloc(1, 2, 6, 0x003E, 128, 0, 40));
        obs.on_alloc(&alloc(1, 5, 7, 0xC56F, 0, 0, 60));
        obs.on_alloc(&alloc(1, 7, 8, 0xC7C0, 0, 0, 15));

        let log = obs.into_log();
        assert_eq!(log.len(), 9);
        assert_eq!(log.sm, Some(86));

        let classes = log.successful_classes();
        assert_eq!(
            classes,
            vec![
                0x0041, 0x0080, 0x2080, 0x90F1, 0xA06C, 0x003E, 0xC56F, 0xC7C0
            ]
        );

        let recipe = log.allocation_recipe();
        assert_eq!(recipe.len(), 8);
        assert_eq!(recipe[0].class, 0x0041);
        assert!(!recipe[0].has_params);
        assert_eq!(recipe[1].class, 0x0080);
        assert!(recipe[1].has_params);
    }

    #[test]
    fn logging_observer_failed_allocs_excluded_from_recipe() {
        let mut obs = LoggingObserver::new();
        obs.on_alloc(&RmAllocEvent {
            h_root: 1,
            h_parent: 0,
            h_new: 1,
            h_class: 0x0041,
            params_size: 0,
            status: 0,
            elapsed: std::time::Duration::from_micros(10),
        });
        obs.on_alloc(&RmAllocEvent {
            h_root: 1,
            h_parent: 1,
            h_new: 2,
            h_class: 0x0080,
            params_size: 56,
            status: 0x103, // NV_ERR_* failure
            elapsed: std::time::Duration::from_micros(5),
        });
        obs.on_alloc(&RmAllocEvent {
            h_root: 1,
            h_parent: 1,
            h_new: 3,
            h_class: 0x2080,
            params_size: 0,
            status: 0,
            elapsed: std::time::Duration::from_micros(20),
        });

        let log = obs.into_log();
        assert_eq!(log.len(), 3);
        let recipe = log.allocation_recipe();
        assert_eq!(recipe.len(), 2, "failed alloc should be excluded");
        assert_eq!(recipe[0].class, 0x0041);
        assert_eq!(recipe[1].class, 0x2080);
        let classes = log.successful_classes();
        assert_eq!(classes, vec![0x0041, 0x2080]);
    }

    #[test]
    fn logging_observer_on_free_records() {
        let mut obs = LoggingObserver::for_gpu("ga102", 86);
        obs.on_alloc(&RmAllocEvent {
            h_root: 1,
            h_parent: 0,
            h_new: 1,
            h_class: 0x0041,
            params_size: 0,
            status: 0,
            elapsed: std::time::Duration::ZERO,
        });
        obs.on_free(1, 1, 0);

        let log = obs.into_log();
        assert_eq!(log.len(), 2);
        assert_eq!(log.records[1].op, RmOp::Free);
        assert_eq!(log.records[1].h_object, 1);
        assert_eq!(log.records[1].status, 0);
    }

    #[test]
    fn rm_protocol_log_empty_default() {
        let log = RmProtocolLog::new();
        assert!(log.is_empty());
        assert_eq!(log.len(), 0);
        assert!(log.successful_classes().is_empty());
        assert!(log.allocation_recipe().is_empty());
    }

    #[test]
    fn rm_protocol_log_for_gpu_sets_metadata() {
        let log = RmProtocolLog::for_gpu("gv100", 70);
        assert_eq!(log.chip.as_deref(), Some("gv100"));
        assert_eq!(log.sm, Some(70));
        assert!(log.records.is_empty());
    }

    #[test]
    fn log_serialization_roundtrip() -> Result<(), serde_json::Error> {
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
        let json = log.to_json()?;
        assert!(json.contains("\"class_or_cmd\": 65"));
        assert!(json.contains("gv100"));
        Ok(())
    }

    #[test]
    fn logging_observer_elapsed_us_saturates_to_u64_max() {
        let mut obs = LoggingObserver::new();
        obs.on_alloc(&RmAllocEvent {
            h_root: 1,
            h_parent: 0,
            h_new: 1,
            h_class: 0x0041,
            params_size: 0,
            status: 0,
            elapsed: std::time::Duration::MAX,
        });
        let log = obs.into_log();
        assert_eq!(log.records[0].elapsed_us, u64::MAX);
    }

    #[test]
    fn logging_observer_control_failure_excluded_from_recipe() {
        let mut obs = LoggingObserver::new();
        obs.on_alloc(&RmAllocEvent {
            h_root: 1,
            h_parent: 0,
            h_new: 1,
            h_class: 0x0041,
            params_size: 0,
            status: 0,
            elapsed: std::time::Duration::from_micros(1),
        });
        obs.on_control(
            1,
            1,
            0x2080_014A,
            0x103,
            std::time::Duration::from_micros(2),
        );
        let log = obs.into_log();
        assert_eq!(log.len(), 2);
        assert_eq!(log.records[1].op, RmOp::Control);
        assert_eq!(log.records[1].status, 0x103);
        assert!(log.allocation_recipe().len() == 1);
        assert_eq!(log.successful_classes(), vec![0x0041]);
    }

    #[test]
    fn logging_observer_free_does_not_add_to_allocation_recipe() {
        let mut obs = LoggingObserver::new();
        obs.on_alloc(&RmAllocEvent {
            h_root: 1,
            h_parent: 0,
            h_new: 1,
            h_class: 0x1111,
            params_size: 8,
            status: 0,
            elapsed: std::time::Duration::ZERO,
        });
        obs.on_free(1, 1, 0);
        let log = obs.into_log();
        let recipe = log.allocation_recipe();
        assert_eq!(recipe.len(), 1);
        assert_eq!(recipe[0].class, 0x1111);
        assert!(recipe[0].has_params);
    }
}
