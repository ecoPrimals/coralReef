// SPDX-License-Identifier: AGPL-3.0-only
//! Ring buffer system for ordered, timed GPU command submission.
//!
//! Models the hardware ring buffer pattern (GPFIFO / PBDMA / command ring):
//! a producer pushes entries, a consumer processes them in FIFO order,
//! and both sides track position via read/write pointers. This enables
//! testing of command ordering, timing, and multi-ring topologies.
//!
//! # Architecture
//!
//! ```text
//!  write_ptr ──►┌───┬───┬───┬───┬───┬───┬───┬───┐
//!               │ 5 │ 6 │ 7 │   │   │   │ 3 │ 4 │
//!               └───┴───┴───┴───┴───┴───┴───┴───┘◄── read_ptr
//!                           ▲ available space     ▲ pending entries
//! ```
//!
//! # Multi-ring
//!
//! [`MultiRing`] manages named rings for different GPU engines or priority
//! levels, allowing independent submission and consumption per channel.

use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// Global ring entry sequence counter.
static RING_SEQ: AtomicU64 = AtomicU64::new(1);

/// Opaque identifier for a ring entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EntryId(u64);

impl EntryId {
    /// Raw numeric value for serialization.
    #[must_use]
    pub fn raw(self) -> u64 {
        self.0
    }
}

impl std::fmt::Display for EntryId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ring:{}", self.0)
    }
}

/// Payload submitted to a ring. Opaque bytes with metadata.
#[derive(Debug, Clone)]
pub struct RingPayload {
    /// Method name for semantic routing (e.g. `"gpfifo.submit"`, `"sec2.boot"`).
    pub method: String,
    /// Opaque command data (register writes, DMA descriptors, etc.).
    pub data: Vec<u8>,
}

/// A timestamped entry in a ring buffer.
#[derive(Debug, Clone)]
pub struct RingEntry {
    /// Unique entry identifier (monotonically increasing across all rings).
    pub id: EntryId,
    /// The submitted payload.
    pub payload: RingPayload,
    /// When the entry was submitted.
    pub submitted_at: Instant,
    /// When the entry was consumed (None if still pending).
    pub consumed_at: Option<Instant>,
    /// Fence value associated with this entry (for synchronization).
    pub fence: u64,
}

impl RingEntry {
    /// Submission-to-consumption latency, or time-since-submission if still pending.
    #[must_use]
    pub fn latency(&self) -> Duration {
        self.consumed_at
            .unwrap_or_else(Instant::now)
            .duration_since(self.submitted_at)
    }
}

/// Errors from ring operations.
#[derive(Debug, thiserror::Error)]
pub enum RingError {
    /// Ring is full — consumer must advance before more entries can be submitted.
    #[error("ring '{name}' is full ({capacity} entries)")]
    Full {
        /// Ring name.
        name: String,
        /// Maximum entry count.
        capacity: usize,
    },

    /// No pending entries to consume.
    #[error("ring '{name}' has no pending entries")]
    Empty {
        /// Ring name.
        name: String,
    },

    /// Entry not found (already consumed or never submitted).
    #[error("entry {id} not found in ring '{name}'")]
    NotFound {
        /// Ring name.
        name: String,
        /// The missing entry.
        id: EntryId,
    },
}

/// Fixed-capacity ring buffer with FIFO ordering and timing data.
///
/// Entries are submitted at the write end and consumed from the read end.
/// The ring tracks both pending (submitted, not yet consumed) and recently
/// consumed entries for diagnostic inspection.
#[derive(Debug)]
pub struct Ring {
    name: String,
    capacity: usize,
    /// Pending entries awaiting consumption (FIFO).
    pending: VecDeque<RingEntry>,
    /// Recently consumed entries (bounded history for diagnostics).
    history: VecDeque<RingEntry>,
    history_limit: usize,
    /// Monotonically increasing fence counter for this ring.
    next_fence: u64,
    total_submitted: u64,
    total_consumed: u64,
}

impl Ring {
    /// Default number of consumed entries retained in history.
    const DEFAULT_HISTORY_LIMIT: usize = 256;

    /// Create a ring with the given name and entry capacity.
    #[must_use]
    pub fn new(name: impl Into<String>, capacity: usize) -> Self {
        Self {
            name: name.into(),
            capacity,
            pending: VecDeque::with_capacity(capacity),
            history: VecDeque::with_capacity(Self::DEFAULT_HISTORY_LIMIT),
            history_limit: Self::DEFAULT_HISTORY_LIMIT,
            next_fence: 1,
            total_submitted: 0,
            total_consumed: 0,
        }
    }

    /// Ring name (e.g. `"gpfifo"`, `"ce0"`, `"sec2"`).
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Number of entries waiting to be consumed.
    #[must_use]
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    /// Available capacity for new submissions.
    #[must_use]
    pub fn available(&self) -> usize {
        self.capacity.saturating_sub(self.pending.len())
    }

    /// Whether the ring has no pending entries.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }

    /// Current fence value (last allocated, not necessarily consumed).
    #[must_use]
    pub fn current_fence(&self) -> u64 {
        self.next_fence.saturating_sub(1)
    }

    /// Submit an entry to the ring. Returns the entry ID and fence value.
    ///
    /// # Errors
    ///
    /// Returns [`RingError::Full`] if the ring is at capacity.
    pub fn submit(&mut self, payload: RingPayload) -> Result<(EntryId, u64), RingError> {
        if self.pending.len() >= self.capacity {
            return Err(RingError::Full {
                name: self.name.clone(),
                capacity: self.capacity,
            });
        }

        let id = EntryId(RING_SEQ.fetch_add(1, Ordering::Relaxed));
        let fence = self.next_fence;
        self.next_fence += 1;

        self.pending.push_back(RingEntry {
            id,
            payload,
            submitted_at: Instant::now(),
            consumed_at: None,
            fence,
        });
        self.total_submitted += 1;

        Ok((id, fence))
    }

    /// Consume the next pending entry (FIFO). Returns the consumed entry.
    ///
    /// # Errors
    ///
    /// Returns [`RingError::Empty`] if no entries are pending.
    pub fn consume(&mut self) -> Result<RingEntry, RingError> {
        let mut entry = self.pending.pop_front().ok_or_else(|| RingError::Empty {
            name: self.name.clone(),
        })?;
        entry.consumed_at = Some(Instant::now());
        self.total_consumed += 1;

        if self.history.len() >= self.history_limit {
            self.history.pop_front();
        }
        let returned = entry.clone();
        self.history.push_back(entry);

        Ok(returned)
    }

    /// Peek at the next pending entry without consuming it.
    #[must_use]
    pub fn peek(&self) -> Option<&RingEntry> {
        self.pending.front()
    }

    /// Consume all entries up to and including the given fence value.
    ///
    /// Returns the consumed entries in order.
    pub fn consume_through_fence(&mut self, fence: u64) -> Vec<RingEntry> {
        let mut consumed = Vec::new();
        while let Some(front) = self.pending.front() {
            if front.fence > fence {
                break;
            }
            match self.consume() {
                Ok(entry) => consumed.push(entry),
                Err(_) => break,
            }
        }
        consumed
    }

    /// Look up an entry by ID in pending or history.
    #[must_use]
    pub fn find(&self, id: EntryId) -> Option<&RingEntry> {
        self.pending
            .iter()
            .chain(self.history.iter())
            .find(|e| e.id == id)
    }

    /// Diagnostic snapshot of timing data for the last N consumed entries.
    #[must_use]
    pub fn recent_latencies(&self, count: usize) -> Vec<(EntryId, Duration)> {
        self.history
            .iter()
            .rev()
            .take(count)
            .map(|e| (e.id, e.latency()))
            .collect()
    }

    /// Summary statistics.
    #[must_use]
    pub fn stats(&self) -> RingStats {
        RingStats {
            name: self.name.clone(),
            capacity: self.capacity,
            pending: self.pending.len(),
            current_fence: self.current_fence(),
            total_submitted: self.total_submitted,
            total_consumed: self.total_consumed,
        }
    }
}

/// Snapshot of ring statistics for diagnostics / RPC.
#[derive(Debug, Clone, serde::Serialize)]
pub struct RingStats {
    /// Ring name.
    pub name: String,
    /// Maximum concurrent pending entries.
    pub capacity: usize,
    /// Currently pending entries.
    pub pending: usize,
    /// Current (latest allocated) fence value.
    pub current_fence: u64,
    /// Total entries ever submitted.
    pub total_submitted: u64,
    /// Total entries consumed.
    pub total_consumed: u64,
}

/// Multi-ring manager for a single device.
///
/// Each ring represents an independent submission channel (e.g. per GPU engine,
/// per priority level, or per firmware endpoint). Rings are ordered by name
/// for deterministic iteration.
#[derive(Debug)]
pub struct MultiRing {
    rings: BTreeMap<String, Ring>,
}

impl MultiRing {
    /// Create an empty multi-ring set.
    #[must_use]
    pub fn new() -> Self {
        Self {
            rings: BTreeMap::new(),
        }
    }

    /// Add a ring. Replaces any existing ring with the same name.
    pub fn add(&mut self, ring: Ring) {
        self.rings.insert(ring.name.clone(), ring);
    }

    /// Remove a ring by name, returning it if it existed.
    pub fn remove(&mut self, name: &str) -> Option<Ring> {
        self.rings.remove(name)
    }

    /// Look up a ring by name.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&Ring> {
        self.rings.get(name)
    }

    /// Look up a ring by name (mutable).
    pub fn get_mut(&mut self, name: &str) -> Option<&mut Ring> {
        self.rings.get_mut(name)
    }

    /// Number of rings.
    #[must_use]
    pub fn ring_count(&self) -> usize {
        self.rings.len()
    }

    /// Total pending entries across all rings.
    #[must_use]
    pub fn total_pending(&self) -> usize {
        self.rings.values().map(Ring::pending_count).sum()
    }

    /// Statistics for all rings.
    #[must_use]
    pub fn all_stats(&self) -> Vec<RingStats> {
        self.rings.values().map(Ring::stats).collect()
    }

    /// Iterate over rings in name order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &Ring)> {
        self.rings.iter().map(|(k, v)| (k.as_str(), v))
    }

    /// Mutable iteration over rings in name order.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (&str, &mut Ring)> {
        self.rings.iter_mut().map(|(k, v)| (k.as_str(), v))
    }
}

impl Default for MultiRing {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn payload(method: &str) -> RingPayload {
        RingPayload {
            method: method.into(),
            data: vec![0xDE, 0xAD],
        }
    }

    #[test]
    fn submit_and_consume_fifo_order() {
        let mut ring = Ring::new("gpfifo", 8);
        let (id1, f1) = ring.submit(payload("cmd.a")).expect("submit 1");
        let (id2, f2) = ring.submit(payload("cmd.b")).expect("submit 2");

        assert!(f2 > f1);
        assert_eq!(ring.pending_count(), 2);

        let e1 = ring.consume().expect("consume 1");
        assert_eq!(e1.id, id1);
        assert!(e1.consumed_at.is_some());

        let e2 = ring.consume().expect("consume 2");
        assert_eq!(e2.id, id2);
        assert!(ring.is_empty());
    }

    #[test]
    fn full_ring_rejects_submit() {
        let mut ring = Ring::new("ce0", 2);
        ring.submit(payload("a")).expect("submit 1");
        ring.submit(payload("b")).expect("submit 2");
        let err = ring.submit(payload("c")).expect_err("full");
        assert!(matches!(err, RingError::Full { capacity: 2, .. }));
    }

    #[test]
    fn consume_empty_ring_returns_error() {
        let mut ring = Ring::new("ce0", 4);
        let err = ring.consume().expect_err("empty");
        assert!(matches!(err, RingError::Empty { .. }));
    }

    #[test]
    fn peek_returns_front_without_consuming() {
        let mut ring = Ring::new("gpfifo", 4);
        let (id, _) = ring.submit(payload("peek_test")).expect("submit");
        let peeked = ring.peek().expect("peek");
        assert_eq!(peeked.id, id);
        assert_eq!(ring.pending_count(), 1, "peek must not consume");
    }

    #[test]
    fn consume_through_fence_consumes_up_to_fence() {
        let mut ring = Ring::new("gpfifo", 8);
        let (_, f1) = ring.submit(payload("a")).expect("submit 1");
        let (_, _f2) = ring.submit(payload("b")).expect("submit 2");
        let (_, _f3) = ring.submit(payload("c")).expect("submit 3");

        let consumed = ring.consume_through_fence(f1);
        assert_eq!(consumed.len(), 1);
        assert_eq!(ring.pending_count(), 2);
    }

    #[test]
    fn consume_through_fence_consumes_multiple() {
        let mut ring = Ring::new("gpfifo", 8);
        ring.submit(payload("a")).expect("submit 1");
        let (_, f2) = ring.submit(payload("b")).expect("submit 2");
        ring.submit(payload("c")).expect("submit 3");

        let consumed = ring.consume_through_fence(f2);
        assert_eq!(consumed.len(), 2);
        assert_eq!(ring.pending_count(), 1);
    }

    #[test]
    fn find_locates_in_pending_and_history() {
        let mut ring = Ring::new("gpfifo", 4);
        let (id1, _) = ring.submit(payload("a")).expect("submit");
        let (id2, _) = ring.submit(payload("b")).expect("submit");

        ring.consume().expect("consume first");

        assert!(ring.find(id1).is_some(), "should be in history");
        assert!(ring.find(id2).is_some(), "should be in pending");
    }

    #[test]
    fn recent_latencies_returns_most_recent_first() {
        let mut ring = Ring::new("gpfifo", 8);
        for i in 0..5 {
            ring.submit(payload(&format!("cmd.{i}"))).expect("submit");
        }
        for _ in 0..5 {
            ring.consume().expect("consume");
        }

        let lats = ring.recent_latencies(3);
        assert_eq!(lats.len(), 3);
    }

    #[test]
    fn stats_reflect_operations() {
        let mut ring = Ring::new("gpfifo", 8);
        ring.submit(payload("a")).expect("submit");
        ring.submit(payload("b")).expect("submit");
        ring.consume().expect("consume");

        let stats = ring.stats();
        assert_eq!(stats.total_submitted, 2);
        assert_eq!(stats.total_consumed, 1);
        assert_eq!(stats.pending, 1);
    }

    #[test]
    fn entry_id_display() {
        let id = EntryId(99);
        assert_eq!(id.to_string(), "ring:99");
        assert_eq!(id.raw(), 99);
    }

    #[test]
    fn multi_ring_manages_independent_channels() {
        let mut mr = MultiRing::new();
        mr.add(Ring::new("gpfifo", 4));
        mr.add(Ring::new("ce0", 4));
        mr.add(Ring::new("sec2", 2));

        assert_eq!(mr.ring_count(), 3);
        assert!(mr.get("gpfifo").is_some());
        assert!(mr.get("nonexistent").is_none());

        mr.get_mut("gpfifo")
            .expect("gpfifo")
            .submit(payload("a"))
            .expect("submit to gpfifo");
        mr.get_mut("ce0")
            .expect("ce0")
            .submit(payload("b"))
            .expect("submit to ce0");

        assert_eq!(mr.total_pending(), 2);

        let stats = mr.all_stats();
        assert_eq!(stats.len(), 3);
    }

    #[test]
    fn multi_ring_remove_returns_ring() {
        let mut mr = MultiRing::new();
        mr.add(Ring::new("temp", 4));
        let removed = mr.remove("temp");
        assert!(removed.is_some());
        assert_eq!(mr.ring_count(), 0);
    }

    #[test]
    fn multi_ring_iter_is_sorted_by_name() {
        let mut mr = MultiRing::new();
        mr.add(Ring::new("z_ring", 2));
        mr.add(Ring::new("a_ring", 2));
        mr.add(Ring::new("m_ring", 2));

        let names: Vec<&str> = mr.iter().map(|(name, _)| name).collect();
        assert_eq!(names, vec!["a_ring", "m_ring", "z_ring"]);
    }

    #[test]
    fn multi_ring_default_is_empty() {
        let mr = MultiRing::default();
        assert_eq!(mr.ring_count(), 0);
        assert_eq!(mr.total_pending(), 0);
    }

    #[test]
    fn ring_available_capacity_decreases_with_submissions() {
        let mut ring = Ring::new("test", 4);
        assert_eq!(ring.available(), 4);
        ring.submit(payload("a")).expect("submit");
        assert_eq!(ring.available(), 3);
        ring.consume().expect("consume");
        assert_eq!(ring.available(), 4);
    }

    #[test]
    fn history_is_bounded() {
        let mut ring = Ring::new("test", 512);
        for i in 0..300 {
            ring.submit(payload(&format!("cmd.{i}"))).expect("submit");
        }
        for _ in 0..300 {
            ring.consume().expect("consume");
        }
        assert!(
            ring.history.len() <= Ring::DEFAULT_HISTORY_LIMIT,
            "history should be bounded"
        );
    }

    #[test]
    fn entry_latency_is_positive_after_consumption() {
        let mut ring = Ring::new("test", 4);
        ring.submit(payload("a")).expect("submit");
        std::thread::sleep(Duration::from_millis(1));
        let entry = ring.consume().expect("consume");
        assert!(
            entry.latency() >= Duration::from_millis(1),
            "latency should reflect wall-clock time"
        );
    }

    #[test]
    fn fence_values_are_monotonically_increasing() {
        let mut ring = Ring::new("test", 8);
        let mut prev_fence = 0;
        for _ in 0..5 {
            let (_, fence) = ring.submit(payload("a")).expect("submit");
            assert!(fence > prev_fence);
            prev_fence = fence;
        }
    }

    #[test]
    fn consume_frees_capacity_for_new_submissions() {
        let mut ring = Ring::new("test", 2);
        ring.submit(payload("a")).expect("submit 1");
        ring.submit(payload("b")).expect("submit 2");
        assert!(ring.submit(payload("c")).is_err());
        ring.consume().expect("consume");
        ring.submit(payload("c")).expect("submit 3 after consume");
    }
}
